use std::fmt;

use super::{CellAddress, CellRange};

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Number(f64),
    Reference(CellAddress),
    Range(CellRange),
    Unary { operator: UnaryOperator, operand: Box<Expr> },
    Binary { left: Box<Expr>, operator: BinaryOperator, right: Box<Expr> },
    Function { function: AggregateFunction, arguments: Vec<Expr> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOperator {
    Plus,
    Minus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOperator {
    Add,
    Subtract,
    Multiply,
    Divide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AggregateFunction {
    Sum,
    Average,
    Minimum,
    Maximum,
}

pub fn parse_formula(input: &str) -> Result<Expr, FormulaError> {
    let input = input.trim();
    let input = input.strip_prefix('=').unwrap_or(input);
    let mut parser = Parser::new(input);
    let expression = parser.parse_expression()?;
    parser.skip_whitespace();
    if parser.is_at_end() {
        Ok(expression)
    } else {
        Err(parser.error("unexpected trailing input"))
    }
}

struct Parser<'a> {
    source: &'a str,
    position: usize,
}

impl<'a> Parser<'a> {
    fn new(source: &'a str) -> Self {
        Self { source, position: 0 }
    }

    fn parse_expression(&mut self) -> Result<Expr, FormulaError> {
        let mut expression = self.parse_term()?;
        loop {
            self.skip_whitespace();
            let operator = match self.peek() {
                Some('+') => BinaryOperator::Add,
                Some('-') => BinaryOperator::Subtract,
                _ => break,
            };
            self.advance();
            let right = self.parse_term()?;
            expression = Expr::Binary { left: Box::new(expression), operator, right: Box::new(right) };
        }
        Ok(expression)
    }

    fn parse_term(&mut self) -> Result<Expr, FormulaError> {
        let mut expression = self.parse_unary()?;
        loop {
            self.skip_whitespace();
            let operator = match self.peek() {
                Some('*') => BinaryOperator::Multiply,
                Some('/') => BinaryOperator::Divide,
                _ => break,
            };
            self.advance();
            let right = self.parse_unary()?;
            expression = Expr::Binary { left: Box::new(expression), operator, right: Box::new(right) };
        }
        Ok(expression)
    }

    fn parse_unary(&mut self) -> Result<Expr, FormulaError> {
        self.skip_whitespace();
        let operator = match self.peek() {
            Some('+') => Some(UnaryOperator::Plus),
            Some('-') => Some(UnaryOperator::Minus),
            _ => None,
        };
        if let Some(operator) = operator {
            self.advance();
            return Ok(Expr::Unary { operator, operand: Box::new(self.parse_unary()?) });
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<Expr, FormulaError> {
        self.skip_whitespace();
        match self.peek() {
            Some('(') => {
                self.advance();
                let expression = self.parse_expression()?;
                self.expect(')')?;
                Ok(expression)
            }
            Some(character) if character.is_ascii_digit() || character == '.' => self.parse_number(),
            Some(character) if character.is_ascii_alphabetic() || character == '$' => self.parse_name_or_reference(),
            Some(_) => Err(self.error("expected a number, cell reference, function, or parenthesized expression")),
            None => Err(self.error("expected an expression")),
        }
    }

    fn parse_number(&mut self) -> Result<Expr, FormulaError> {
        let start = self.position;
        let mut has_dot = false;
        while let Some(character) = self.peek() {
            if character.is_ascii_digit() {
                self.advance();
            } else if character == '.' && !has_dot {
                has_dot = true;
                self.advance();
            } else {
                break;
            }
        }
        self.source[start..self.position]
            .parse::<f64>()
            .map(Expr::Number)
            .map_err(|_| self.error("invalid number"))
    }

    fn parse_name_or_reference(&mut self) -> Result<Expr, FormulaError> {
        let start = self.position;
        if self.peek() == Some('$') {
            self.advance();
        }
        while self.peek().is_some_and(|character| character.is_ascii_alphabetic()) {
            self.advance();
        }
        let name_end = self.position;
        self.skip_whitespace();
        if self.peek() == Some('(') {
            let name = self.source[start..name_end].to_owned();
            return self.parse_function(&name);
        }

        if self.peek() == Some('$') {
            self.advance();
        }
        while self.peek().is_some_and(|character| character.is_ascii_digit()) {
            self.advance();
        }
        let address = self.source[start..self.position]
            .parse::<CellAddress>()
            .map_err(|_| self.error("invalid cell reference"))?;
        self.skip_whitespace();
        if self.peek() != Some(':') {
            return Ok(Expr::Reference(address));
        }
        self.advance();
        self.skip_whitespace();
        let range_start = self.position;
        if self.peek() == Some('$') {
            self.advance();
        }
        while self.peek().is_some_and(|character| character.is_ascii_alphabetic()) {
            self.advance();
        }
        if self.peek() == Some('$') {
            self.advance();
        }
        while self.peek().is_some_and(|character| character.is_ascii_digit()) {
            self.advance();
        }
        let end = self.source[range_start..self.position]
            .parse::<CellAddress>()
            .map_err(|_| self.error("invalid range end"))?;
        Ok(Expr::Range(CellRange::new(address, end)))
    }

    fn parse_function(&mut self, name: &str) -> Result<Expr, FormulaError> {
        let function = match name.trim_start_matches('$').to_ascii_uppercase().as_str() {
            "SUM" => AggregateFunction::Sum,
            "AVG" | "AVERAGE" => AggregateFunction::Average,
            "MIN" => AggregateFunction::Minimum,
            "MAX" => AggregateFunction::Maximum,
            _ => return Err(self.error("unknown function")),
        };
        self.expect('(')?;
        let mut arguments = Vec::new();
        self.skip_whitespace();
        if self.peek() != Some(')') {
            loop {
                arguments.push(self.parse_expression()?);
                self.skip_whitespace();
                if self.peek() != Some(',') {
                    break;
                }
                self.advance();
            }
        }
        self.expect(')')?;
        Ok(Expr::Function { function, arguments })
    }

    fn expect(&mut self, expected: char) -> Result<(), FormulaError> {
        self.skip_whitespace();
        if self.peek() == Some(expected) {
            self.advance();
            Ok(())
        } else {
            Err(self.error(&format!("expected '{expected}'")))
        }
    }

    fn peek(&self) -> Option<char> {
        self.source[self.position..].chars().next()
    }

    fn advance(&mut self) {
        if let Some(character) = self.peek() {
            self.position += character.len_utf8();
        }
    }

    fn skip_whitespace(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.advance();
        }
    }

    fn is_at_end(&self) -> bool {
        self.position == self.source.len()
    }

    fn error(&self, message: &str) -> FormulaError {
        FormulaError { position: self.position, message: message.to_owned() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormulaError {
    pub position: usize,
    pub message: String,
}

impl fmt::Display for FormulaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} at byte {}", self.message, self.position)
    }
}

impl std::error::Error for FormulaError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_obeys_arithmetic_precedence() {
        let expression = parse_formula("=1 + 2 * 3").unwrap();
        assert!(matches!(expression, Expr::Binary { operator: BinaryOperator::Add, .. }));
        let Expr::Binary { right, .. } = expression else { unreachable!() };
        assert!(matches!(*right, Expr::Binary { operator: BinaryOperator::Multiply, .. }));
    }

    #[test]
    fn parser_accepts_ranges_and_case_insensitive_functions() {
        let expression = parse_formula("average(a1:B3, 10)").unwrap();
        let Expr::Function { function, arguments } = expression else { panic!("expected function") };
        assert_eq!(function, AggregateFunction::Average);
        assert!(matches!(arguments[0], Expr::Range(_)));
        assert_eq!(arguments[1], Expr::Number(10.0));
    }

    #[test]
    fn parser_rejects_unknown_functions_and_trailing_input() {
        assert!(parse_formula("NOPE(A1)").is_err());
        assert!(parse_formula("1 + 2 hello").is_err());
    }
}
