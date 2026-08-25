use std::{collections::HashMap, fmt};

use super::{
    parse_formula, AggregateFunction, BinaryOperator, Cell, CellAddress, CellError, CellValue, Expr,
    UnaryOperator,
};

#[derive(Debug, Clone, PartialEq)]
pub struct Worksheet {
    name: String,
    cells: HashMap<CellAddress, Cell>,
}

impl Worksheet {
    pub fn new(name: impl Into<String>) -> Result<Self, WorksheetError> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(WorksheetError::EmptyName);
        }
        Ok(Self { name: name.trim().to_owned(), cells: HashMap::new() })
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
            self.cells.insert(address, Cell::new(raw));
        }
    }

    pub fn clear(&mut self, address: CellAddress) {
        self.cells.remove(&address);
    }

    pub fn clear_all(&mut self) {
        self.cells.clear();
    }

    pub fn cell(&self, address: CellAddress) -> Option<&Cell> {
        self.cells.get(&address)
    }

    pub fn populated_cells(&self) -> impl Iterator<Item = (CellAddress, &Cell)> {
        self.cells.iter().map(|(address, cell)| (*address, cell))
    }

    pub fn value(&self, address: CellAddress) -> CellValue {
        let mut cache = HashMap::new();
        self.evaluate_cell(address, &mut Vec::new(), &mut cache)
    }

    pub fn values(&self) -> HashMap<CellAddress, CellValue> {
        let mut cache = HashMap::new();
        for address in self.cells.keys() {
            self.evaluate_cell(*address, &mut Vec::new(), &mut cache);
        }
        cache
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
            Expr::Reference(address) => Ok(self.evaluate_cell(*address, stack, cache)),
            Expr::Range(_) => Err(CellError::InvalidValue),
            Expr::Unary { operator, operand } => {
                let number = self.number(self.evaluate_expression(operand, stack, cache)?)?;
                Ok(CellValue::Number(match operator {
                    UnaryOperator::Plus => number,
                    UnaryOperator::Minus => -number,
                }))
            }
            Expr::Binary { left, operator, right } => {
                let left = self.number(self.evaluate_expression(left, stack, cache)?)?;
                let right = self.number(self.evaluate_expression(right, stack, cache)?)?;
                let number = match operator {
                    BinaryOperator::Add => left + right,
                    BinaryOperator::Subtract => left - right,
                    BinaryOperator::Multiply => left * right,
                    BinaryOperator::Divide if right.abs() < f64::EPSILON => {
                        return Err(CellError::DivisionByZero);
                    }
                    BinaryOperator::Divide => left / right,
                };
                Ok(CellValue::Number(number))
            }
            Expr::Function { function, arguments } => {
                let mut numbers = Vec::new();
                for argument in arguments {
                    match argument {
                        Expr::Range(range) => {
                            for address in range.addresses() {
                                match self.evaluate_cell(address, stack, cache) {
                                    CellValue::Number(number) => numbers.push(number),
                                    CellValue::Error(error) => return Err(error),
                                    CellValue::Blank | CellValue::Text(_) => {}
                                }
                            }
                        }
                        Expr::Reference(address) => match self.evaluate_cell(*address, stack, cache) {
                            CellValue::Number(number) => numbers.push(number),
                            CellValue::Error(error) => return Err(error),
                            CellValue::Blank | CellValue::Text(_) => {}
                        },
                        _ => {
                            let value = self.evaluate_expression(argument, stack, cache)?;
                            numbers.push(self.number(value)?);
                        }
                    }
                }
                self.aggregate(*function, &numbers).map(CellValue::Number)
            }
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
            AggregateFunction::Minimum => numbers.iter().copied().reduce(f64::min).ok_or(CellError::InvalidValue),
            AggregateFunction::Maximum => numbers.iter().copied().reduce(f64::max).ok_or(CellError::InvalidValue),
        }
    }
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
        assert_eq!(sheet.value(address("A1")), CellValue::Error(CellError::DivisionByZero));
        assert_eq!(sheet.value(address("A2")), CellValue::Error(CellError::DivisionByZero));
        assert_eq!(sheet.value(address("A3")), CellValue::Error(CellError::Parse));
        assert_eq!(sheet.value(address("A4")), CellValue::Error(CellError::UnknownFunction));
    }

    #[test]
    fn detects_direct_and_indirect_cycles() {
        let mut sheet = Worksheet::new("Model").unwrap();
        sheet.set(address("A1"), "=A1");
        sheet.set(address("B1"), "=C1+1");
        sheet.set(address("C1"), "=B1+1");
        assert_eq!(sheet.value(address("A1")), CellValue::Error(CellError::CircularReference));
        assert_eq!(sheet.value(address("B1")), CellValue::Error(CellError::CircularReference));
        assert_eq!(sheet.value(address("C1")), CellValue::Error(CellError::CircularReference));
    }
}
