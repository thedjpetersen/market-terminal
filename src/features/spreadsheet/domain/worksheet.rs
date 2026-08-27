use std::{
    cell::RefCell,
    cmp::Ordering,
    collections::{HashMap, HashSet},
    fmt,
};

use super::{
    parse_formula, AggregateFunction, BinaryOperator, Cell, CellAddress, CellError, CellValue,
    Expr, UnaryOperator,
};

#[derive(Debug, Clone)]
pub struct Worksheet {
    name: String,
    cells: HashMap<CellAddress, Cell>,
    evaluation: RefCell<EvaluationState>,
}

#[derive(Debug, Clone, Default)]
struct EvaluationState {
    // Formula results are memoized across reads. Mutations evict only the
    // changed cell and its transitive dependents through the reverse index.
    cache: HashMap<CellAddress, CellValue>,
    dependencies: HashMap<CellAddress, HashSet<CellAddress>>,
    dependents: HashMap<CellAddress, HashSet<CellAddress>>,
}

impl PartialEq for Worksheet {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.cells == other.cells
    }
}

impl Worksheet {
    pub fn new(name: impl Into<String>) -> Result<Self, WorksheetError> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(WorksheetError::EmptyName);
        }
        Ok(Self {
            name: name.trim().to_owned(),
            cells: HashMap::new(),
            evaluation: RefCell::new(EvaluationState::default()),
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn rename(&mut self, name: impl Into<String>) -> Result<(), WorksheetError> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(WorksheetError::EmptyName);
        }
        self.name = name.trim().to_owned();
        Ok(())
    }

    pub fn set(&mut self, address: CellAddress, raw: impl Into<String>) {
        let raw = raw.into();
        if raw.is_empty() {
            self.cells.remove(&address);
        } else {
            self.cells.insert(address, Cell::new(raw.clone()));
        }
        self.update_dependencies(address, &raw);
        self.invalidate(address);
    }

    pub fn clear(&mut self, address: CellAddress) {
        self.cells.remove(&address);
        self.update_dependencies(address, "");
        self.invalidate(address);
    }

    pub fn clear_all(&mut self) {
        self.cells.clear();
        *self.evaluation.get_mut() = EvaluationState::default();
    }

    pub fn cell(&self, address: CellAddress) -> Option<&Cell> {
        self.cells.get(&address)
    }

    pub fn populated_cells(&self) -> impl Iterator<Item = (CellAddress, &Cell)> {
        self.cells.iter().map(|(address, cell)| (*address, cell))
    }

    pub fn value(&self, address: CellAddress) -> CellValue {
        let mut cache = std::mem::take(&mut self.evaluation.borrow_mut().cache);
        let value = self.evaluate_cell(address, &mut Vec::new(), &mut cache);
        self.evaluation.borrow_mut().cache = cache;
        value
    }

    pub fn values(&self) -> HashMap<CellAddress, CellValue> {
        let mut cache = std::mem::take(&mut self.evaluation.borrow_mut().cache);
        for address in self.cells.keys() {
            self.evaluate_cell(*address, &mut Vec::new(), &mut cache);
        }
        self.evaluation.borrow_mut().cache = cache.clone();
        cache
    }

    fn update_dependencies(&mut self, address: CellAddress, raw: &str) {
        let state = self.evaluation.get_mut();
        if let Some(previous) = state.dependencies.remove(&address) {
            for dependency in previous {
                if let Some(dependents) = state.dependents.get_mut(&dependency) {
                    dependents.remove(&address);
                    if dependents.is_empty() {
                        state.dependents.remove(&dependency);
                    }
                }
            }
        }

        let dependencies = raw
            .trim()
            .strip_prefix('=')
            .and_then(|formula| parse_formula(formula).ok())
            .map(|expression| {
                let mut dependencies = HashSet::new();
                collect_dependencies(&expression, &mut dependencies);
                dependencies
            })
            .unwrap_or_default();
        for dependency in &dependencies {
            state
                .dependents
                .entry(*dependency)
                .or_default()
                .insert(address);
        }
        if !dependencies.is_empty() {
            state.dependencies.insert(address, dependencies);
        }
    }

    fn invalidate(&mut self, changed: CellAddress) {
        let state = self.evaluation.get_mut();
        let mut pending = vec![changed];
        let mut invalidated = HashSet::new();
        while let Some(address) = pending.pop() {
            if !invalidated.insert(address) {
                continue;
            }
            state.cache.remove(&address);
            if let Some(dependents) = state.dependents.get(&address) {
                pending.extend(dependents.iter().copied());
            }
        }
    }

    fn evaluate_cell(
        &self,
        address: CellAddress,
        stack: &mut Vec<CellAddress>,
        cache: &mut HashMap<CellAddress, CellValue>,
    ) -> CellValue {
        if let Some(value) = cache.get(&address) {
            return value.clone();
        }
        if stack.contains(&address) {
            return CellValue::Error(CellError::CircularReference);
        }
        let Some(cell) = self.cells.get(&address) else {
            return CellValue::Blank;
        };
        let raw = cell.raw().trim();
        let value = if raw.is_empty() {
            CellValue::Blank
        } else if raw.starts_with('=') {
            stack.push(address);
            let evaluated = parse_formula(raw)
                .map_err(|error| {
                    if error.message == "unknown function" {
                        CellError::UnknownFunction
                    } else {
                        CellError::Parse
                    }
                })
                .and_then(|expression| self.evaluate_expression(&expression, stack, cache));
            stack.pop();
            evaluated.unwrap_or_else(CellValue::Error)
        } else if let Ok(number) = raw.parse::<f64>() {
            CellValue::Number(number)
        } else {
            CellValue::Text(cell.raw().to_owned())
        };
        cache.insert(address, value.clone());
        value
    }

    fn evaluate_expression(
        &self,
        expression: &Expr,
        stack: &mut Vec<CellAddress>,
        cache: &mut HashMap<CellAddress, CellValue>,
    ) -> Result<CellValue, CellError> {
        match expression {
            Expr::Number(number) => Ok(CellValue::Number(*number)),
            Expr::Text(text) => Ok(CellValue::Text(text.clone())),
            Expr::Reference(reference) => {
                if !self.reference_is_local(reference.sheet()) {
                    return Err(CellError::InvalidReference);
                }
                Ok(self.evaluate_cell(reference.cell().address(), stack, cache))
            }
            Expr::Range(_) => Err(CellError::InvalidValue),
            Expr::Unary { operator, operand } => {
                let number = self.number(self.evaluate_expression(operand, stack, cache)?)?;
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
                let left = self.evaluate_expression(left, stack, cache)?;
                let right = self.evaluate_expression(right, stack, cache)?;
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
                        let left = self.number(left)?;
                        let right = self.number(right)?;
                        let number = match operator {
                            BinaryOperator::Add => left + right,
                            BinaryOperator::Subtract => left - right,
                            BinaryOperator::Multiply => left * right,
                            BinaryOperator::Divide if right.abs() < f64::EPSILON => {
                                return Err(CellError::DivisionByZero);
                            }
                            BinaryOperator::Divide => left / right,
                            _ => unreachable!("comparison operators were handled separately"),
                        };
                        Ok(CellValue::Number(number))
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
            } => self.evaluate_function(*function, arguments, stack, cache),
        }
    }

    fn evaluate_function(
        &self,
        function: AggregateFunction,
        arguments: &[Expr],
        stack: &mut Vec<CellAddress>,
        cache: &mut HashMap<CellAddress, CellValue>,
    ) -> Result<CellValue, CellError> {
        match function {
            AggregateFunction::If => {
                expect_arity(arguments, 3, 3)?;
                let condition = self.evaluate_expression(&arguments[0], stack, cache)?;
                let branch = if truthy(&condition)? {
                    &arguments[1]
                } else {
                    &arguments[2]
                };
                self.evaluate_expression(branch, stack, cache)
            }
            AggregateFunction::IfError => {
                expect_arity(arguments, 2, 2)?;
                match self.evaluate_expression(&arguments[0], stack, cache) {
                    Ok(CellValue::Error(_)) | Err(_) => {
                        self.evaluate_expression(&arguments[1], stack, cache)
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
                    let value = self.evaluate_expression(argument, stack, cache)?;
                    if truthy(&value)? == seeking {
                        return Ok(boolean_value(seeking));
                    }
                }
                Ok(boolean_value(!seeking))
            }
            AggregateFunction::Not => {
                expect_arity(arguments, 1, 1)?;
                let value = self.evaluate_expression(&arguments[0], stack, cache)?;
                Ok(boolean_value(!truthy(&value)?))
            }
            AggregateFunction::Concat => {
                let values = self.evaluate_arguments(arguments, stack, cache)?;
                let mut text = String::new();
                for value in values {
                    text.push_str(&value_as_text(value)?);
                }
                Ok(CellValue::Text(text))
            }
            AggregateFunction::Length => {
                expect_arity(arguments, 1, 1)?;
                let value = self.evaluate_expression(&arguments[0], stack, cache)?;
                Ok(CellValue::Number(
                    value_as_text(value)?.chars().count() as f64
                ))
            }
            AggregateFunction::Lower | AggregateFunction::Upper | AggregateFunction::Trim => {
                expect_arity(arguments, 1, 1)?;
                let value = self.evaluate_expression(&arguments[0], stack, cache)?;
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
                let value = self.evaluate_expression(&arguments[0], stack, cache)?;
                let text = value_as_text(value)?;
                let count = if let Some(argument) = arguments.get(1) {
                    let value = self.evaluate_expression(argument, stack, cache)?;
                    character_count(self.number(value)?)?
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
                let value = self.evaluate_expression(&arguments[0], stack, cache)?;
                Ok(CellValue::Number(self.number(value)?.abs()))
            }
            AggregateFunction::Round => {
                expect_arity(arguments, 1, 2)?;
                let value = self.evaluate_expression(&arguments[0], stack, cache)?;
                let digits = if let Some(argument) = arguments.get(1) {
                    let value = self.evaluate_expression(argument, stack, cache)?;
                    self.number(value)?
                } else {
                    0.0
                };
                if digits.fract().abs() > f64::EPSILON || !(-15.0..=15.0).contains(&digits) {
                    return Err(CellError::InvalidValue);
                }
                let factor = 10_f64.powi(digits as i32);
                Ok(CellValue::Number(
                    (self.number(value)? * factor).round() / factor,
                ))
            }
            AggregateFunction::Power => {
                expect_arity(arguments, 2, 2)?;
                let base = self.evaluate_expression(&arguments[0], stack, cache)?;
                let exponent = self.evaluate_expression(&arguments[1], stack, cache)?;
                finite_number(self.number(base)?.powf(self.number(exponent)?))
                    .map(CellValue::Number)
            }
            AggregateFunction::SquareRoot => {
                expect_arity(arguments, 1, 1)?;
                let value = self.evaluate_expression(&arguments[0], stack, cache)?;
                let value = self.number(value)?;
                if value < 0.0 {
                    return Err(CellError::InvalidValue);
                }
                finite_number(value.sqrt()).map(CellValue::Number)
            }
            AggregateFunction::XLookup => self.evaluate_xlookup(arguments, stack, cache),
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
                let values = self.evaluate_arguments(arguments, stack, cache)?;
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
                self.aggregate(function, &numbers).map(CellValue::Number)
            }
        }
    }

    fn evaluate_arguments(
        &self,
        arguments: &[Expr],
        stack: &mut Vec<CellAddress>,
        cache: &mut HashMap<CellAddress, CellValue>,
    ) -> Result<Vec<CellValue>, CellError> {
        let mut values = Vec::new();
        for argument in arguments {
            if let Expr::Range(range) = argument {
                if !self.reference_is_local(range.sheet()) {
                    return Err(CellError::InvalidReference);
                }
                values.extend(
                    range
                        .addresses()
                        .map(|address| self.evaluate_cell(address, stack, cache)),
                );
            } else {
                values.push(self.evaluate_expression(argument, stack, cache)?);
            }
        }
        Ok(values)
    }

    fn evaluate_xlookup(
        &self,
        arguments: &[Expr],
        stack: &mut Vec<CellAddress>,
        cache: &mut HashMap<CellAddress, CellValue>,
    ) -> Result<CellValue, CellError> {
        expect_arity(arguments, 3, 4)?;
        let needle = self.evaluate_expression(&arguments[0], stack, cache)?;
        if let CellValue::Error(error) = &needle {
            return Err(error.clone());
        }
        let (Expr::Range(lookup), Expr::Range(results)) = (&arguments[1], &arguments[2]) else {
            return Err(CellError::InvalidValue);
        };
        if !self.reference_is_local(lookup.sheet()) || !self.reference_is_local(results.sheet()) {
            return Err(CellError::InvalidReference);
        }
        let lookup_addresses = lookup.addresses().collect::<Vec<_>>();
        let result_addresses = results.addresses().collect::<Vec<_>>();
        if lookup_addresses.len() != result_addresses.len() {
            return Err(CellError::InvalidValue);
        }
        for (lookup_address, result_address) in lookup_addresses.into_iter().zip(result_addresses) {
            let candidate = self.evaluate_cell(lookup_address, stack, cache);
            if let CellValue::Error(error) = &candidate {
                return Err(error.clone());
            }
            if values_equal(&needle, &candidate) {
                return Ok(self.evaluate_cell(result_address, stack, cache));
            }
        }
        if let Some(fallback) = arguments.get(3) {
            self.evaluate_expression(fallback, stack, cache)
        } else {
            Err(CellError::NotAvailable)
        }
    }

    fn number(&self, value: CellValue) -> Result<f64, CellError> {
        match value {
            CellValue::Number(number) => Ok(number),
            CellValue::Blank => Ok(0.0),
            CellValue::Text(_) => Err(CellError::InvalidValue),
            CellValue::Error(error) => Err(error),
        }
    }

    fn aggregate(&self, function: AggregateFunction, numbers: &[f64]) -> Result<f64, CellError> {
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

    fn reference_is_local(&self, sheet: Option<&str>) -> bool {
        sheet.is_none_or(|sheet| sheet.eq_ignore_ascii_case(&self.name))
    }
}

fn collect_dependencies(expression: &Expr, dependencies: &mut HashSet<CellAddress>) {
    match expression {
        Expr::Reference(reference) => {
            dependencies.insert(reference.cell().address());
        }
        Expr::Range(range) => dependencies.extend(range.addresses()),
        Expr::Unary { operand, .. } => collect_dependencies(operand, dependencies),
        Expr::Binary { left, right, .. } => {
            collect_dependencies(left, dependencies);
            collect_dependencies(right, dependencies);
        }
        Expr::Function { arguments, .. } => {
            for argument in arguments {
                collect_dependencies(argument, dependencies);
            }
        }
        Expr::Number(_) | Expr::Text(_) => {}
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorksheetError {
    EmptyName,
}

impl fmt::Display for WorksheetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyName => write!(formatter, "worksheet name cannot be empty"),
        }
    }
}

impl std::error::Error for WorksheetError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn address(value: &str) -> CellAddress {
        value.parse().unwrap()
    }

    #[test]
    fn stores_only_populated_cells() {
        let mut sheet = Worksheet::new("Model").unwrap();
        sheet.set(address("A1"), "10");
        sheet.set(address("Z100"), "last");
        assert_eq!(sheet.populated_cells().count(), 2);
        sheet.clear(address("A1"));
        assert_eq!(sheet.populated_cells().count(), 1);
    }

    #[test]
    fn evaluates_arithmetic_references_and_parentheses() {
        let mut sheet = Worksheet::new("Model").unwrap();
        sheet.set(address("A1"), "10");
        sheet.set(address("A2"), "=A1 * (2 + 3)");
        sheet.set(address("A3"), "=-A2 / 5");
        assert_eq!(sheet.value(address("A2")), CellValue::Number(50.0));
        assert_eq!(sheet.value(address("A3")), CellValue::Number(-10.0));
    }

    #[test]
    fn evaluates_aggregate_functions_over_ranges_and_arguments() {
        let mut sheet = Worksheet::new("Model").unwrap();
        sheet.set(address("A1"), "2");
        sheet.set(address("A2"), "4");
        sheet.set(address("A3"), "ignored");
        sheet.set(address("B1"), "=SUM(A1:A3, 4)");
        sheet.set(address("B2"), "=AVG(A1:A3)");
        sheet.set(address("B3"), "=MIN(A1:A2, -1)");
        sheet.set(address("B4"), "=MAX(A1:A2, 12)");
        sheet.set(address("B5"), "=SUM(A3)");
        assert_eq!(sheet.value(address("B1")), CellValue::Number(10.0));
        assert_eq!(sheet.value(address("B2")), CellValue::Number(3.0));
        assert_eq!(sheet.value(address("B3")), CellValue::Number(-1.0));
        assert_eq!(sheet.value(address("B4")), CellValue::Number(12.0));
        assert_eq!(sheet.value(address("B5")), CellValue::Number(0.0));
    }

    #[test]
    fn reports_formula_errors_as_cell_values() {
        let mut sheet = Worksheet::new("Model").unwrap();
        sheet.set(address("A1"), "=1/0");
        sheet.set(address("A2"), "=A1+1");
        sheet.set(address("A3"), "=SUM(");
        sheet.set(address("A4"), "=MISSING(A1)");
        assert_eq!(
            sheet.value(address("A1")),
            CellValue::Error(CellError::DivisionByZero)
        );
        assert_eq!(
            sheet.value(address("A2")),
            CellValue::Error(CellError::DivisionByZero)
        );
        assert_eq!(
            sheet.value(address("A3")),
            CellValue::Error(CellError::Parse)
        );
        assert_eq!(
            sheet.value(address("A4")),
            CellValue::Error(CellError::UnknownFunction)
        );
    }

    #[test]
    fn detects_direct_and_indirect_cycles() {
        let mut sheet = Worksheet::new("Model").unwrap();
        sheet.set(address("A1"), "=A1");
        sheet.set(address("B1"), "=C1+1");
        sheet.set(address("C1"), "=B1+1");
        assert_eq!(
            sheet.value(address("A1")),
            CellValue::Error(CellError::CircularReference)
        );
        assert_eq!(
            sheet.value(address("B1")),
            CellValue::Error(CellError::CircularReference)
        );
        assert_eq!(
            sheet.value(address("C1")),
            CellValue::Error(CellError::CircularReference)
        );
    }

    #[test]
    fn comparisons_and_if_are_typed_and_lazy() {
        let mut sheet = Worksheet::new("Model").unwrap();
        sheet.set(address("A1"), "12");
        sheet.set(address("B1"), "=IF(A1 >= 10, \"large\", \"small\")");
        sheet.set(address("B2"), "=IF(A1 = 12, 42, 1 / 0)");
        sheet.set(address("B3"), "=A1 <> 12");
        sheet.set(address("B4"), "=\"alpha\" < \"beta\"");
        assert_eq!(
            sheet.value(address("B1")),
            CellValue::Text("large".to_owned())
        );
        assert_eq!(sheet.value(address("B2")), CellValue::Number(42.0));
        assert_eq!(sheet.value(address("B3")), CellValue::Number(0.0));
        assert_eq!(sheet.value(address("B4")), CellValue::Number(1.0));
    }

    #[test]
    fn evaluates_text_conditional_count_and_numeric_functions() {
        let mut sheet = Worksheet::new("Model").unwrap();
        sheet.set(address("A1"), "north");
        sheet.set(address("A2"), "3.14159");
        sheet.set(address("A3"), "");
        sheet.set(address("A4"), "-4");
        sheet.set(address("B1"), "=CONCAT(A1, \"-\", ROUND(A2, 2))");
        sheet.set(address("B2"), "=LEN(B1)");
        sheet.set(address("B3"), "=ABS(A4)");
        sheet.set(address("B4"), "=COUNT(A1:A4)");
        sheet.set(address("B5"), "=COUNTA(A1:A4)");
        sheet.set(address("B6"), "=AND(A2 > 3, NOT(A4 > 0))");
        assert_eq!(
            sheet.value(address("B1")),
            CellValue::Text("north-3.14".to_owned())
        );
        assert_eq!(sheet.value(address("B2")), CellValue::Number(10.0));
        assert_eq!(sheet.value(address("B3")), CellValue::Number(4.0));
        assert_eq!(sheet.value(address("B4")), CellValue::Number(2.0));
        assert_eq!(sheet.value(address("B5")), CellValue::Number(3.0));
        assert_eq!(sheet.value(address("B6")), CellValue::Number(1.0));
    }

    #[test]
    fn evaluates_extended_text_math_and_error_functions() {
        let mut sheet = Worksheet::new("Model").unwrap();
        sheet.set(address("A1"), "  Ibm Research  ");
        sheet.set(address("B1"), "=UPPER(TRIM(A1))");
        sheet.set(address("B2"), "=LOWER(LEFT(TRIM(A1), 3))");
        sheet.set(address("B3"), "=RIGHT(TRIM(A1), 8)");
        sheet.set(address("B4"), "=POWER(2, 8) + SQRT(81)");
        sheet.set(address("B5"), "=IFERROR(1 / 0, 42)");
        sheet.set(address("B6"), "=IFERROR(7, 1 / 0)");

        assert_eq!(
            sheet.value(address("B1")),
            CellValue::Text("IBM RESEARCH".to_owned())
        );
        assert_eq!(
            sheet.value(address("B2")),
            CellValue::Text("ibm".to_owned())
        );
        assert_eq!(
            sheet.value(address("B3")),
            CellValue::Text("Research".to_owned())
        );
        assert_eq!(sheet.value(address("B4")), CellValue::Number(265.0));
        assert_eq!(sheet.value(address("B5")), CellValue::Number(42.0));
        assert_eq!(sheet.value(address("B6")), CellValue::Number(7.0));
    }

    #[test]
    fn xlookup_matches_exact_values_and_supports_a_lazy_fallback() {
        let mut sheet = Worksheet::new("Model").unwrap();
        sheet.set(address("A1"), "MSFT");
        sheet.set(address("A2"), "META");
        sheet.set(address("A3"), "AMZN");
        sheet.set(address("B1"), "420");
        sheet.set(address("B2"), "780");
        sheet.set(address("B3"), "230");
        sheet.set(address("C1"), "=XLOOKUP(\"META\", A1:A3, B1:B3)");
        sheet.set(
            address("C2"),
            "=XLOOKUP(\"NVDA\", A1:A3, B1:B3, \"missing\")",
        );
        sheet.set(address("C3"), "=XLOOKUP(\"NVDA\", A1:A3, B1:B3)");
        assert_eq!(sheet.value(address("C1")), CellValue::Number(780.0));
        assert_eq!(
            sheet.value(address("C2")),
            CellValue::Text("missing".to_owned())
        );
        assert_eq!(
            sheet.value(address("C3")),
            CellValue::Error(CellError::NotAvailable)
        );
    }

    #[test]
    fn edits_invalidate_only_the_changed_cell_and_transitive_dependents() {
        let mut sheet = Worksheet::new("Model").unwrap();
        sheet.set(address("A1"), "10");
        sheet.set(address("B1"), "=A1 * 2");
        sheet.set(address("C1"), "99");
        sheet.values();
        assert_eq!(sheet.evaluation.borrow().cache.len(), 3);

        sheet.set(address("A1"), "20");
        let evaluation = sheet.evaluation.borrow();
        let cache = &evaluation.cache;
        assert!(!cache.contains_key(&address("A1")));
        assert!(!cache.contains_key(&address("B1")));
        assert_eq!(cache.get(&address("C1")), Some(&CellValue::Number(99.0)));
        drop(evaluation);

        assert_eq!(sheet.value(address("B1")), CellValue::Number(40.0));
        assert_eq!(sheet.value(address("C1")), CellValue::Number(99.0));
    }

    #[test]
    fn changing_a_formula_rewires_its_dependency_graph() {
        let mut sheet = Worksheet::new("Model").unwrap();
        sheet.set(address("A1"), "10");
        sheet.set(address("D1"), "40");
        sheet.set(address("B1"), "=A1");
        sheet.set(address("C1"), "=B1 + 1");
        sheet.values();

        sheet.set(address("B1"), "=D1");
        assert_eq!(sheet.value(address("C1")), CellValue::Number(41.0));
        sheet.set(address("A1"), "20");
        assert_eq!(sheet.value(address("C1")), CellValue::Number(41.0));
        sheet.set(address("D1"), "50");
        assert_eq!(sheet.value(address("C1")), CellValue::Number(51.0));
    }

    #[test]
    fn breaking_a_cached_cycle_invalidates_every_member() {
        let mut sheet = Worksheet::new("Model").unwrap();
        sheet.set(address("A1"), "=B1");
        sheet.set(address("B1"), "=A1");
        assert_eq!(
            sheet.value(address("A1")),
            CellValue::Error(CellError::CircularReference)
        );

        sheet.set(address("B1"), "5");
        assert_eq!(sheet.value(address("A1")), CellValue::Number(5.0));
        assert_eq!(sheet.value(address("B1")), CellValue::Number(5.0));
    }
}
