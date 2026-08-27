use std::{cmp::Ordering, collections::HashMap, fmt};

use chrono::{Datelike, NaiveDate};

use super::{
    parse_formula, AggregateFunction, BinaryOperator, CellAddress, CellError, CellValue, Expr,
    FormulaReference, UnaryOperator, Worksheet, WorksheetError,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct CellKey {
    sheet: usize,
    address: CellAddress,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Workbook {
    sheets: Vec<Worksheet>,
    active_sheet: usize,
}

impl Workbook {
    pub fn new() -> Self {
        Self {
            sheets: vec![Worksheet::new("Sheet1").expect("default sheet name is valid")],
            active_sheet: 0,
        }
    }

    pub fn from_sheets(sheets: Vec<Worksheet>, active_sheet: usize) -> Result<Self, WorkbookError> {
        if sheets.is_empty() {
            return Err(WorkbookError::EmptyWorkbook);
        }
        if active_sheet >= sheets.len() {
            return Err(WorkbookError::SheetIndexOutOfBounds(active_sheet));
        }
        for (index, sheet) in sheets.iter().enumerate() {
            if sheets[..index]
                .iter()
                .any(|candidate| candidate.name().eq_ignore_ascii_case(sheet.name()))
            {
                return Err(WorkbookError::DuplicateSheetName(sheet.name().to_owned()));
            }
        }
        Ok(Self {
            sheets,
            active_sheet,
        })
    }

    pub fn sheets(&self) -> &[Worksheet] {
        &self.sheets
    }

    pub fn active_sheet(&self) -> &Worksheet {
        &self.sheets[self.active_sheet]
    }

    pub const fn active_sheet_index(&self) -> usize {
        self.active_sheet
    }

    pub fn sheet_count(&self) -> usize {
        self.sheets.len()
    }

    pub fn active_sheet_mut(&mut self) -> &mut Worksheet {
        &mut self.sheets[self.active_sheet]
    }

    pub fn add_sheet(&mut self, name: impl Into<String>) -> Result<usize, WorkbookError> {
        let name = name.into();
        if self
            .sheets
            .iter()
            .any(|sheet| sheet.name().eq_ignore_ascii_case(name.trim()))
        {
            return Err(WorkbookError::DuplicateSheetName(name));
        }
        let sheet = Worksheet::new(name).map_err(WorkbookError::InvalidSheet)?;
        self.sheets.push(sheet);
        Ok(self.sheets.len() - 1)
    }

    pub fn select_sheet(&mut self, name: &str) -> Result<(), WorkbookError> {
        self.active_sheet = self
            .sheets
            .iter()
            .position(|sheet| sheet.name().eq_ignore_ascii_case(name))
            .ok_or_else(|| WorkbookError::SheetNotFound(name.to_owned()))?;
        Ok(())
    }

    pub fn select_sheet_at(&mut self, index: usize) -> Result<(), WorkbookError> {
        if index >= self.sheets.len() {
            return Err(WorkbookError::SheetIndexOutOfBounds(index));
        }
        self.active_sheet = index;
        Ok(())
    }

    pub fn select_next_sheet(&mut self) {
        self.active_sheet = (self.active_sheet + 1) % self.sheets.len();
    }

    pub fn select_previous_sheet(&mut self) {
        self.active_sheet = (self.active_sheet + self.sheets.len() - 1) % self.sheets.len();
    }

    pub fn rename_active_sheet(&mut self, name: impl Into<String>) -> Result<(), WorkbookError> {
        let name = name.into();
        let normalized = name.trim();
        if self.sheets.iter().enumerate().any(|(index, sheet)| {
            index != self.active_sheet && sheet.name().eq_ignore_ascii_case(normalized)
        }) {
            return Err(WorkbookError::DuplicateSheetName(name));
        }
        self.sheets[self.active_sheet]
            .rename(name)
            .map_err(WorkbookError::InvalidSheet)
    }

    pub fn remove_active_sheet(&mut self) -> Result<Worksheet, WorkbookError> {
        if self.sheets.len() == 1 {
            return Err(WorkbookError::CannotRemoveLastSheet);
        }
        let removed = self.sheets.remove(self.active_sheet);
        self.active_sheet = self.active_sheet.min(self.sheets.len() - 1);
        Ok(removed)
    }

    pub fn sheet(&self, name: &str) -> Option<&Worksheet> {
        self.sheets
            .iter()
            .find(|sheet| sheet.name().eq_ignore_ascii_case(name))
    }

    pub fn sheet_mut(&mut self, name: &str) -> Option<&mut Worksheet> {
        self.sheets
            .iter_mut()
            .find(|sheet| sheet.name().eq_ignore_ascii_case(name))
    }

    /// Evaluates the active sheet in workbook scope so qualified references,
    /// ranges, and cycles can cross worksheet boundaries.
    pub fn active_value(&self, address: CellAddress) -> CellValue {
        let mut cache = HashMap::new();
        self.evaluate_cell(
            CellKey {
                sheet: self.active_sheet,
                address,
            },
            &mut Vec::new(),
            &mut cache,
        )
    }

    pub fn active_values(&self) -> HashMap<CellAddress, CellValue> {
        let mut cache = HashMap::new();
        let mut stack = Vec::new();
        for address in self
            .active_sheet()
            .populated_cells()
            .map(|(address, _)| address)
        {
            self.evaluate_cell(
                CellKey {
                    sheet: self.active_sheet,
                    address,
                },
                &mut stack,
                &mut cache,
            );
        }
        cache
            .into_iter()
            .filter_map(|(key, value)| {
                (key.sheet == self.active_sheet).then_some((key.address, value))
            })
            .collect()
    }

    fn evaluate_cell(
        &self,
        key: CellKey,
        stack: &mut Vec<CellKey>,
        cache: &mut HashMap<CellKey, CellValue>,
    ) -> CellValue {
        if let Some(value) = cache.get(&key) {
            return value.clone();
        }
        if stack.contains(&key) {
            return CellValue::Error(CellError::CircularReference);
        }
        let Some(cell) = self.sheets[key.sheet].cell(key.address) else {
            return CellValue::Blank;
        };
        let raw = cell.raw().trim();
        let value = if raw.is_empty() {
            CellValue::Blank
        } else if raw.starts_with('=') {
            stack.push(key);
            let evaluated = parse_formula(raw)
                .map_err(|error| {
                    if error.message == "unknown function" {
                        CellError::UnknownFunction
                    } else {
                        CellError::Parse
                    }
                })
                .and_then(|expression| {
                    self.evaluate_expression(&expression, key.sheet, stack, cache)
                });
            stack.pop();
            evaluated.unwrap_or_else(CellValue::Error)
        } else if let Ok(number) = raw.parse::<f64>() {
            CellValue::Number(number)
        } else {
            CellValue::Text(cell.raw().to_owned())
        };
        cache.insert(key, value.clone());
        value
    }

    fn evaluate_expression(
        &self,
        expression: &Expr,
        current_sheet: usize,
        stack: &mut Vec<CellKey>,
        cache: &mut HashMap<CellKey, CellValue>,
    ) -> Result<CellValue, CellError> {
        match expression {
            Expr::Number(number) => Ok(CellValue::Number(*number)),
            Expr::Text(text) => Ok(CellValue::Text(text.clone())),
            Expr::Reference(reference) => {
                let key = self.resolve_reference(current_sheet, reference)?;
                Ok(self.evaluate_cell(key, stack, cache))
            }
            Expr::Range(_) => Err(CellError::InvalidValue),
            Expr::Unary { operator, operand } => {
                let value = self.evaluate_expression(operand, current_sheet, stack, cache)?;
                let number = number(value)?;
                Ok(CellValue::Number(match operator {
                    UnaryOperator::Plus => number,
                    UnaryOperator::Minus => -number,
                }))
            }
            Expr::Binary {
                left,
                operator,
                right,
            } => {
                let left = self.evaluate_expression(left, current_sheet, stack, cache)?;
                let right = self.evaluate_expression(right, current_sheet, stack, cache)?;
                if let CellValue::Error(error) = &left {
                    return Err(error.clone());
                }
                if let CellValue::Error(error) = &right {
                    return Err(error.clone());
                }
                match operator {
                    BinaryOperator::Add
                    | BinaryOperator::Subtract
                    | BinaryOperator::Multiply
                    | BinaryOperator::Divide => {
                        let left = number(left)?;
                        let right = number(right)?;
                        let result = match operator {
                            BinaryOperator::Add => left + right,
                            BinaryOperator::Subtract => left - right,
                            BinaryOperator::Multiply => left * right,
                            BinaryOperator::Divide if right.abs() < f64::EPSILON => {
                                return Err(CellError::DivisionByZero)
                            }
                            BinaryOperator::Divide => left / right,
                            _ => unreachable!("comparison operators were handled separately"),
                        };
                        Ok(CellValue::Number(result))
                    }
                    BinaryOperator::Equal => Ok(boolean_value(values_equal(&left, &right))),
                    BinaryOperator::NotEqual => Ok(boolean_value(!values_equal(&left, &right))),
                    BinaryOperator::LessThan
                    | BinaryOperator::LessThanOrEqual
                    | BinaryOperator::GreaterThan
                    | BinaryOperator::GreaterThanOrEqual => {
                        let ordering =
                            compare_values(&left, &right).ok_or(CellError::InvalidValue)?;
                        let matches = match operator {
                            BinaryOperator::LessThan => ordering == Ordering::Less,
                            BinaryOperator::LessThanOrEqual => ordering != Ordering::Greater,
                            BinaryOperator::GreaterThan => ordering == Ordering::Greater,
                            BinaryOperator::GreaterThanOrEqual => ordering != Ordering::Less,
                            _ => unreachable!("equality operators were handled separately"),
                        };
                        Ok(boolean_value(matches))
                    }
                }
            }
            Expr::Function {
                function,
                arguments,
            } => self.evaluate_function(*function, arguments, current_sheet, stack, cache),
        }
    }

    fn evaluate_function(
        &self,
        function: AggregateFunction,
        arguments: &[Expr],
        current_sheet: usize,
        stack: &mut Vec<CellKey>,
        cache: &mut HashMap<CellKey, CellValue>,
    ) -> Result<CellValue, CellError> {
        match function {
            AggregateFunction::If => {
                expect_arity(arguments, 3, 3)?;
                let condition =
                    self.evaluate_expression(&arguments[0], current_sheet, stack, cache)?;
                let branch = if truthy(&condition)? {
                    &arguments[1]
                } else {
                    &arguments[2]
                };
                self.evaluate_expression(branch, current_sheet, stack, cache)
            }
            AggregateFunction::IfError => {
                expect_arity(arguments, 2, 2)?;
                match self.evaluate_expression(&arguments[0], current_sheet, stack, cache) {
                    Ok(CellValue::Error(_)) | Err(_) => {
                        self.evaluate_expression(&arguments[1], current_sheet, stack, cache)
                    }
                    Ok(value) => Ok(value),
                }
            }
            AggregateFunction::And | AggregateFunction::Or => {
                if arguments.is_empty() {
                    return Err(CellError::InvalidValue);
                }
                let seeking = function == AggregateFunction::Or;
                for argument in arguments {
                    let value = self.evaluate_expression(argument, current_sheet, stack, cache)?;
                    if truthy(&value)? == seeking {
                        return Ok(boolean_value(seeking));
                    }
                }
                Ok(boolean_value(!seeking))
            }
            AggregateFunction::Not => {
                expect_arity(arguments, 1, 1)?;
                let value = self.evaluate_expression(&arguments[0], current_sheet, stack, cache)?;
                Ok(boolean_value(!truthy(&value)?))
            }
            AggregateFunction::Concat => {
                let values = self.evaluate_arguments(arguments, current_sheet, stack, cache)?;
                let mut text = String::new();
                for value in values {
                    text.push_str(&value_as_text(value)?);
                }
                Ok(CellValue::Text(text))
            }
            AggregateFunction::Length => {
                expect_arity(arguments, 1, 1)?;
                let value = self.evaluate_expression(&arguments[0], current_sheet, stack, cache)?;
                Ok(CellValue::Number(
                    value_as_text(value)?.chars().count() as f64
                ))
            }
            AggregateFunction::Lower | AggregateFunction::Upper | AggregateFunction::Trim => {
                expect_arity(arguments, 1, 1)?;
                let value = self.evaluate_expression(&arguments[0], current_sheet, stack, cache)?;
                let text = value_as_text(value)?;
                Ok(CellValue::Text(match function {
                    AggregateFunction::Lower => text.to_lowercase(),
                    AggregateFunction::Upper => text.to_uppercase(),
                    AggregateFunction::Trim => {
                        text.split_whitespace().collect::<Vec<_>>().join(" ")
                    }
                    _ => unreachable!("text transform was matched above"),
                }))
            }
            AggregateFunction::Left | AggregateFunction::Right => {
                expect_arity(arguments, 1, 2)?;
                let value = self.evaluate_expression(&arguments[0], current_sheet, stack, cache)?;
                let text = value_as_text(value)?;
                let count = if let Some(argument) = arguments.get(1) {
                    let value = self.evaluate_expression(argument, current_sheet, stack, cache)?;
                    character_count(number(value)?)?
                } else {
                    1
                };
                let characters = text.chars().collect::<Vec<_>>();
                let count = count.min(characters.len());
                let selected = if function == AggregateFunction::Left {
                    &characters[..count]
                } else {
                    &characters[characters.len() - count..]
                };
                Ok(CellValue::Text(selected.iter().collect()))
            }
            AggregateFunction::Absolute => {
                expect_arity(arguments, 1, 1)?;
                let value = self.evaluate_expression(&arguments[0], current_sheet, stack, cache)?;
                Ok(CellValue::Number(number(value)?.abs()))
            }
            AggregateFunction::Round => {
                expect_arity(arguments, 1, 2)?;
                let value = self.evaluate_expression(&arguments[0], current_sheet, stack, cache)?;
                let digits = if let Some(argument) = arguments.get(1) {
                    let value = self.evaluate_expression(argument, current_sheet, stack, cache)?;
                    number(value)?
                } else {
                    0.0
                };
                if digits.fract().abs() > f64::EPSILON || !(-15.0..=15.0).contains(&digits) {
                    return Err(CellError::InvalidValue);
                }
                let factor = 10_f64.powi(digits as i32);
                Ok(CellValue::Number(
                    (number(value)? * factor).round() / factor,
                ))
            }
            AggregateFunction::Power => {
                expect_arity(arguments, 2, 2)?;
                let base = self.evaluate_expression(&arguments[0], current_sheet, stack, cache)?;
                let exponent =
                    self.evaluate_expression(&arguments[1], current_sheet, stack, cache)?;
                finite_number(number(base)?.powf(number(exponent)?)).map(CellValue::Number)
            }
            AggregateFunction::SquareRoot => {
                expect_arity(arguments, 1, 1)?;
                let value = self.evaluate_expression(&arguments[0], current_sheet, stack, cache)?;
                let value = number(value)?;
                if value < 0.0 {
                    return Err(CellError::InvalidValue);
                }
                finite_number(value.sqrt()).map(CellValue::Number)
            }
            AggregateFunction::Date => {
                expect_arity(arguments, 3, 3)?;
                let year = self.evaluate_expression(&arguments[0], current_sheet, stack, cache)?;
                let month = self.evaluate_expression(&arguments[1], current_sheet, stack, cache)?;
                let day = self.evaluate_expression(&arguments[2], current_sheet, stack, cache)?;
                date_value(number(year)?, number(month)?, number(day)?)
            }
            AggregateFunction::Year | AggregateFunction::Month | AggregateFunction::Day => {
                expect_arity(arguments, 1, 1)?;
                let value = self.evaluate_expression(&arguments[0], current_sheet, stack, cache)?;
                date_part(function, value_as_text(value)?)
            }
            AggregateFunction::XLookup => {
                self.evaluate_xlookup(arguments, current_sheet, stack, cache)
            }
            AggregateFunction::PriceLast
            | AggregateFunction::PriceChange
            | AggregateFunction::History
            | AggregateFunction::Fundamental => Err(CellError::NotAvailable),
            AggregateFunction::Sum
            | AggregateFunction::Average
            | AggregateFunction::Minimum
            | AggregateFunction::Maximum
            | AggregateFunction::Count
            | AggregateFunction::CountA => {
                let values = self.evaluate_arguments(arguments, current_sheet, stack, cache)?;
                if function == AggregateFunction::CountA {
                    return Ok(CellValue::Number(
                        values
                            .iter()
                            .filter(|value| !matches!(value, CellValue::Blank))
                            .count() as f64,
                    ));
                }
                let numbers = values
                    .into_iter()
                    .filter_map(|value| match value {
                        CellValue::Number(number) => Some(Ok(number)),
                        CellValue::Error(error) => Some(Err(error)),
                        CellValue::Blank | CellValue::Text(_) => None,
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                if function == AggregateFunction::Count {
                    return Ok(CellValue::Number(numbers.len() as f64));
                }
                aggregate(function, &numbers).map(CellValue::Number)
            }
        }
    }

    fn evaluate_arguments(
        &self,
        arguments: &[Expr],
        current_sheet: usize,
        stack: &mut Vec<CellKey>,
        cache: &mut HashMap<CellKey, CellValue>,
    ) -> Result<Vec<CellValue>, CellError> {
        let mut values = Vec::new();
        for argument in arguments {
            if let Expr::Range(range) = argument {
                let sheet = self.resolve_sheet(current_sheet, range.sheet())?;
                values.extend(
                    range.addresses().map(|address| {
                        self.evaluate_cell(CellKey { sheet, address }, stack, cache)
                    }),
                );
            } else {
                values.push(self.evaluate_expression(argument, current_sheet, stack, cache)?);
            }
        }
        Ok(values)
    }

    fn evaluate_xlookup(
        &self,
        arguments: &[Expr],
        current_sheet: usize,
        stack: &mut Vec<CellKey>,
        cache: &mut HashMap<CellKey, CellValue>,
    ) -> Result<CellValue, CellError> {
        expect_arity(arguments, 3, 4)?;
        let needle = self.evaluate_expression(&arguments[0], current_sheet, stack, cache)?;
        if let CellValue::Error(error) = &needle {
            return Err(error.clone());
        }
        let (Expr::Range(lookup), Expr::Range(results)) = (&arguments[1], &arguments[2]) else {
            return Err(CellError::InvalidValue);
        };
        let lookup_sheet = self.resolve_sheet(current_sheet, lookup.sheet())?;
        let results_sheet = self.resolve_sheet(current_sheet, results.sheet())?;
        let lookup_addresses = lookup.addresses().collect::<Vec<_>>();
        let result_addresses = results.addresses().collect::<Vec<_>>();
        if lookup_addresses.len() != result_addresses.len() {
            return Err(CellError::InvalidValue);
        }
        for (lookup_address, result_address) in lookup_addresses.into_iter().zip(result_addresses) {
            let candidate = self.evaluate_cell(
                CellKey {
                    sheet: lookup_sheet,
                    address: lookup_address,
                },
                stack,
                cache,
            );
            if let CellValue::Error(error) = &candidate {
                return Err(error.clone());
            }
            if values_equal(&needle, &candidate) {
                return Ok(self.evaluate_cell(
                    CellKey {
                        sheet: results_sheet,
                        address: result_address,
                    },
                    stack,
                    cache,
                ));
            }
        }
        if let Some(fallback) = arguments.get(3) {
            self.evaluate_expression(fallback, current_sheet, stack, cache)
        } else {
            Err(CellError::NotAvailable)
        }
    }

    fn resolve_reference(
        &self,
        current_sheet: usize,
        reference: &FormulaReference,
    ) -> Result<CellKey, CellError> {
        Ok(CellKey {
            sheet: self.resolve_sheet(current_sheet, reference.sheet())?,
            address: reference.cell().address(),
        })
    }

    fn resolve_sheet(&self, current_sheet: usize, sheet: Option<&str>) -> Result<usize, CellError> {
        match sheet {
            None => Ok(current_sheet),
            Some(name) => self
                .sheets
                .iter()
                .position(|candidate| candidate.name().eq_ignore_ascii_case(name))
                .ok_or(CellError::InvalidReference),
        }
    }
}

fn expect_arity(arguments: &[Expr], minimum: usize, maximum: usize) -> Result<(), CellError> {
    if (minimum..=maximum).contains(&arguments.len()) {
        Ok(())
    } else {
        Err(CellError::InvalidValue)
    }
}

fn truthy(value: &CellValue) -> Result<bool, CellError> {
    match value {
        CellValue::Blank => Ok(false),
        CellValue::Number(number) => Ok(number.abs() >= f64::EPSILON),
        CellValue::Text(text) => Ok(!text.is_empty()),
        CellValue::Error(error) => Err(error.clone()),
    }
}

fn boolean_value(value: bool) -> CellValue {
    CellValue::Number(if value { 1.0 } else { 0.0 })
}

fn values_equal(left: &CellValue, right: &CellValue) -> bool {
    match (left, right) {
        (CellValue::Blank, CellValue::Blank) => true,
        (CellValue::Number(left), CellValue::Number(right)) => left == right,
        (CellValue::Text(left), CellValue::Text(right)) => left == right,
        (CellValue::Error(left), CellValue::Error(right)) => left == right,
        _ => false,
    }
}

fn compare_values(left: &CellValue, right: &CellValue) -> Option<Ordering> {
    match (left, right) {
        (CellValue::Number(left), CellValue::Number(right)) => left.partial_cmp(right),
        (CellValue::Text(left), CellValue::Text(right)) => Some(left.cmp(right)),
        (CellValue::Blank, CellValue::Blank) => Some(Ordering::Equal),
        _ => None,
    }
}

fn value_as_text(value: CellValue) -> Result<String, CellError> {
    match value {
        CellValue::Blank => Ok(String::new()),
        CellValue::Number(number) => Ok(number.to_string()),
        CellValue::Text(text) => Ok(text),
        CellValue::Error(error) => Err(error),
    }
}

fn number(value: CellValue) -> Result<f64, CellError> {
    match value {
        CellValue::Number(number) => Ok(number),
        CellValue::Blank => Ok(0.0),
        CellValue::Text(_) => Err(CellError::InvalidValue),
        CellValue::Error(error) => Err(error),
    }
}

fn character_count(value: f64) -> Result<usize, CellError> {
    if value < 0.0 || value.fract().abs() > f64::EPSILON || value > usize::MAX as f64 {
        return Err(CellError::InvalidValue);
    }
    Ok(value as usize)
}

fn finite_number(value: f64) -> Result<f64, CellError> {
    value
        .is_finite()
        .then_some(value)
        .ok_or(CellError::InvalidValue)
}

fn date_value(year: f64, month: f64, day: f64) -> Result<CellValue, CellError> {
    let year = whole_number(year)?;
    let month = u32::try_from(whole_number(month)?).map_err(|_| CellError::InvalidValue)?;
    let day = u32::try_from(whole_number(day)?).map_err(|_| CellError::InvalidValue)?;
    NaiveDate::from_ymd_opt(year, month, day)
        .map(|date| CellValue::Text(date.format("%Y-%m-%d").to_string()))
        .ok_or(CellError::InvalidValue)
}

fn date_part(function: AggregateFunction, value: String) -> Result<CellValue, CellError> {
    let date =
        NaiveDate::parse_from_str(&value, "%Y-%m-%d").map_err(|_| CellError::InvalidValue)?;
    Ok(CellValue::Number(match function {
        AggregateFunction::Year => f64::from(date.year()),
        AggregateFunction::Month => f64::from(date.month()),
        AggregateFunction::Day => f64::from(date.day()),
        _ => unreachable!("date part was matched above"),
    }))
}

fn whole_number(value: f64) -> Result<i32, CellError> {
    if !value.is_finite()
        || value.fract().abs() > f64::EPSILON
        || value < f64::from(i32::MIN)
        || value > f64::from(i32::MAX)
    {
        return Err(CellError::InvalidValue);
    }
    Ok(value as i32)
}

fn aggregate(function: AggregateFunction, numbers: &[f64]) -> Result<f64, CellError> {
    match function {
        AggregateFunction::Sum => Ok(numbers.iter().sum()),
        AggregateFunction::Average if numbers.is_empty() => Err(CellError::DivisionByZero),
        AggregateFunction::Average => Ok(numbers.iter().sum::<f64>() / numbers.len() as f64),
        AggregateFunction::Minimum => numbers
            .iter()
            .copied()
            .reduce(f64::min)
            .ok_or(CellError::InvalidValue),
        AggregateFunction::Maximum => numbers
            .iter()
            .copied()
            .reduce(f64::max)
            .ok_or(CellError::InvalidValue),
        _ => Err(CellError::InvalidValue),
    }
}

impl Default for Workbook {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkbookError {
    InvalidSheet(WorksheetError),
    EmptyWorkbook,
    DuplicateSheetName(String),
    SheetNotFound(String),
    SheetIndexOutOfBounds(usize),
    CannotRemoveLastSheet,
}

impl fmt::Display for WorkbookError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSheet(error) => error.fmt(formatter),
            Self::EmptyWorkbook => write!(formatter, "a workbook must contain a worksheet"),
            Self::DuplicateSheetName(name) => {
                write!(formatter, "worksheet '{name}' already exists")
            }
            Self::SheetNotFound(name) => write!(formatter, "worksheet '{name}' was not found"),
            Self::SheetIndexOutOfBounds(index) => {
                write!(formatter, "worksheet index {index} is out of bounds")
            }
            Self::CannotRemoveLastSheet => {
                write!(formatter, "a workbook must contain at least one worksheet")
            }
        }
    }
}

impl std::error::Error for WorkbookError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manages_multiple_named_sheets() {
        let mut workbook = Workbook::new();
        workbook.add_sheet("Valuation").unwrap();
        workbook.select_sheet("valuation").unwrap();
        assert_eq!(workbook.active_sheet().name(), "Valuation");
        assert!(matches!(
            workbook.add_sheet("VALUATION"),
            Err(WorkbookError::DuplicateSheetName(_))
        ));
    }

    #[test]
    fn cycles_renames_and_removes_sheets_without_losing_a_valid_selection() {
        let mut workbook = Workbook::new();
        workbook.add_sheet("Valuation").unwrap();
        workbook.add_sheet("Scenarios").unwrap();

        workbook.select_previous_sheet();
        assert_eq!(workbook.active_sheet().name(), "Scenarios");
        workbook.select_next_sheet();
        assert_eq!(workbook.active_sheet().name(), "Sheet1");
        workbook.rename_active_sheet("Inputs").unwrap();
        assert_eq!(workbook.active_sheet().name(), "Inputs");

        workbook.select_sheet("Scenarios").unwrap();
        assert_eq!(workbook.remove_active_sheet().unwrap().name(), "Scenarios");
        assert_eq!(workbook.active_sheet().name(), "Valuation");
        assert_eq!(workbook.sheet_count(), 2);
    }

    #[test]
    fn protects_sheet_names_and_the_last_remaining_sheet() {
        let mut workbook = Workbook::new();
        assert_eq!(
            workbook.remove_active_sheet(),
            Err(WorkbookError::CannotRemoveLastSheet)
        );
        workbook.add_sheet("Model").unwrap();
        workbook.select_sheet("Model").unwrap();
        assert!(matches!(
            workbook.rename_active_sheet("sheet1"),
            Err(WorkbookError::DuplicateSheetName(_))
        ));
        assert_eq!(
            workbook.select_sheet_at(2),
            Err(WorkbookError::SheetIndexOutOfBounds(2))
        );
    }

    #[test]
    fn evaluates_references_ranges_and_cycles_across_sheets() {
        let mut workbook = Workbook::new();
        workbook.rename_active_sheet("Model").unwrap();
        workbook.add_sheet("Base Case").unwrap();
        workbook
            .sheet_mut("Base Case")
            .unwrap()
            .set("A1".parse().unwrap(), "10");
        workbook
            .sheet_mut("Base Case")
            .unwrap()
            .set("A2".parse().unwrap(), "20");
        workbook
            .sheet_mut("Model")
            .unwrap()
            .set("A1".parse().unwrap(), "=SUM('Base Case'!A1:A2)");
        assert_eq!(
            workbook.active_value("A1".parse().unwrap()),
            CellValue::Number(30.0)
        );

        workbook
            .sheet_mut("Model")
            .unwrap()
            .set("B1".parse().unwrap(), "='Base Case'!B1");
        workbook
            .sheet_mut("Base Case")
            .unwrap()
            .set("B1".parse().unwrap(), "=Model!B1");
        assert_eq!(
            workbook.active_value("B1".parse().unwrap()),
            CellValue::Error(CellError::CircularReference)
        );
        workbook
            .sheet_mut("Model")
            .unwrap()
            .set("C1".parse().unwrap(), "=Missing!A1");
        assert_eq!(
            workbook.active_value("C1".parse().unwrap()),
            CellValue::Error(CellError::InvalidReference)
        );
    }
}
