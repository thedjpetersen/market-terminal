use std::fmt;

use super::{AddressError, CellAddress, CellRange, CellReference};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FormulaReference {
    sheet: Option<String>,
    cell: CellReference,
}

impl FormulaReference {
    pub fn new(sheet: Option<String>, cell: CellReference) -> Self {
        Self { sheet, cell }
    }

    pub fn sheet(&self) -> Option<&str> {
        self.sheet.as_deref()
    }

    pub const fn cell(&self) -> CellReference {
        self.cell
    }

    fn translated(&self, column_delta: i16, row_delta: i32) -> Result<Self, AddressError> {
        Ok(Self {
            sheet: self.sheet.clone(),
            cell: self.cell.translated(column_delta, row_delta)?,
        })
    }
}

impl fmt::Display for FormulaReference {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(sheet) = &self.sheet {
            write_sheet_name(formatter, sheet)?;
            write!(formatter, "!")?;
        }
        self.cell.fmt(formatter)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormulaRange {
    sheet: Option<String>,
    start: CellReference,
    end: CellReference,
}

impl FormulaRange {
    pub fn new(sheet: Option<String>, start: CellReference, end: CellReference) -> Self {
        Self { sheet, start, end }
    }

    pub fn sheet(&self) -> Option<&str> {
        self.sheet.as_deref()
    }

    pub const fn start(&self) -> CellReference {
        self.start
    }

    pub const fn end(&self) -> CellReference {
        self.end
    }

    pub fn addresses(&self) -> impl Iterator<Item = CellAddress> {
        CellRange::new(self.start.address(), self.end.address()).addresses()
    }

    fn translated(&self, column_delta: i16, row_delta: i32) -> Result<Self, AddressError> {
        Ok(Self {
            sheet: self.sheet.clone(),
            start: self.start.translated(column_delta, row_delta)?,
            end: self.end.translated(column_delta, row_delta)?,
        })
    }
}

impl fmt::Display for FormulaRange {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(sheet) = &self.sheet {
            write_sheet_name(formatter, sheet)?;
            write!(formatter, "!")?;
        }
        write!(formatter, "{}:{}", self.start, self.end)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Number(f64),
    Text(String),
    Reference(FormulaReference),
    Range(FormulaRange),
    Unary {
        operator: UnaryOperator,
        operand: Box<Expr>,
    },
    Binary {
        left: Box<Expr>,
        operator: BinaryOperator,
        right: Box<Expr>,
    },
    Function {
        function: AggregateFunction,
        arguments: Vec<Expr>,
    },
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
    Equal,
    NotEqual,
    LessThan,
    LessThanOrEqual,
    GreaterThan,
    GreaterThanOrEqual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AggregateFunction {
    Sum,
    Average,
    Minimum,
    Maximum,
    Count,
    CountA,
    If,
    And,
    Or,
    Not,
    Concat,
    Length,
    Lower,
    Upper,
    Trim,
    Left,
    Right,
    Absolute,
    Round,
    Power,
    SquareRoot,
    IfError,
    Date,
    Year,
    Month,
    Day,
    XLookup,
    PriceLast,
    PriceChange,
    History,
    Fundamental,
}

impl fmt::Display for Expr {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.fmt_with_precedence(formatter, 0)
    }
}

impl Expr {
    pub(super) fn rename_sheet(&mut self, old: &str, new: &str) -> bool {
        let sheet = match self {
            Self::Reference(reference) => &mut reference.sheet,
            Self::Range(range) => &mut range.sheet,
            Self::Unary { operand, .. } => return operand.rename_sheet(old, new),
            Self::Binary { left, right, .. } => {
                let left_changed = left.rename_sheet(old, new);
                return right.rename_sheet(old, new) || left_changed;
            }
            Self::Function { arguments, .. } => {
                let mut changed = false;
                for argument in arguments {
                    changed |= argument.rename_sheet(old, new);
                }
                return changed;
            }
            Self::Number(_) | Self::Text(_) => return false,
        };
        if sheet
            .as_deref()
            .is_some_and(|name| name.eq_ignore_ascii_case(old))
        {
            *sheet = Some(new.to_owned());
            true
        } else {
            false
        }
    }

    fn fmt_with_precedence(
        &self,
        formatter: &mut fmt::Formatter<'_>,
        parent_precedence: u8,
    ) -> fmt::Result {
        match self {
            Self::Number(number) => write!(formatter, "{number}"),
            Self::Text(text) => write!(formatter, "\"{}\"", text.replace('\"', "\"\"")),
            Self::Reference(reference) => write!(formatter, "{reference}"),
            Self::Range(range) => write!(formatter, "{range}"),
            Self::Unary { operator, operand } => {
                let precedence = 4;
                let parenthesize = precedence < parent_precedence;
                if parenthesize {
                    write!(formatter, "(")?;
                }
                write!(
                    formatter,
                    "{}",
                    match operator {
                        UnaryOperator::Plus => "+",
                        UnaryOperator::Minus => "-",
                    }
                )?;
                operand.fmt_with_precedence(formatter, precedence)?;
                if parenthesize {
                    write!(formatter, ")")?;
                }
                Ok(())
            }
            Self::Binary {
                left,
                operator,
                right,
            } => {
                let precedence = operator.precedence();
                let parenthesize = precedence < parent_precedence;
                if parenthesize {
                    write!(formatter, "(")?;
                }
                left.fmt_with_precedence(formatter, precedence)?;
                write!(formatter, " {} ", operator.symbol())?;
                right.fmt_with_precedence(formatter, precedence + 1)?;
                if parenthesize {
                    write!(formatter, ")")?;
                }
                Ok(())
            }
            Self::Function {
                function,
                arguments,
            } => {
                write!(formatter, "{}(", function.name())?;
                for (index, argument) in arguments.iter().enumerate() {
                    if index > 0 {
                        write!(formatter, ", ")?;
                    }
                    argument.fmt_with_precedence(formatter, 0)?;
                }
                write!(formatter, ")")
            }
        }
    }
}

impl BinaryOperator {
    const fn precedence(self) -> u8 {
        match self {
            Self::Equal
            | Self::NotEqual
            | Self::LessThan
            | Self::LessThanOrEqual
            | Self::GreaterThan
            | Self::GreaterThanOrEqual => 1,
            Self::Add | Self::Subtract => 2,
            Self::Multiply | Self::Divide => 3,
        }
    }

    const fn symbol(self) -> &'static str {
        match self {
            Self::Add => "+",
            Self::Subtract => "-",
            Self::Multiply => "*",
            Self::Divide => "/",
            Self::Equal => "=",
            Self::NotEqual => "<>",
            Self::LessThan => "<",
            Self::LessThanOrEqual => "<=",
            Self::GreaterThan => ">",
            Self::GreaterThanOrEqual => ">=",
        }
    }
}

impl AggregateFunction {
    const fn name(self) -> &'static str {
        match self {
            Self::Sum => "SUM",
            Self::Average => "AVERAGE",
            Self::Minimum => "MIN",
            Self::Maximum => "MAX",
            Self::Count => "COUNT",
            Self::CountA => "COUNTA",
            Self::If => "IF",
            Self::And => "AND",
            Self::Or => "OR",
            Self::Not => "NOT",
            Self::Concat => "CONCAT",
            Self::Length => "LEN",
            Self::Lower => "LOWER",
            Self::Upper => "UPPER",
            Self::Trim => "TRIM",
            Self::Left => "LEFT",
            Self::Right => "RIGHT",
            Self::Absolute => "ABS",
            Self::Round => "ROUND",
            Self::Power => "POWER",
            Self::SquareRoot => "SQRT",
            Self::IfError => "IFERROR",
            Self::Date => "DATE",
            Self::Year => "YEAR",
            Self::Month => "MONTH",
            Self::Day => "DAY",
            Self::XLookup => "XLOOKUP",
            Self::PriceLast => "PX_LAST",
            Self::PriceChange => "PX_CHANGE",
            Self::History => "HISTORY",
            Self::Fundamental => "FUNDAMENTAL",
        }
    }
}

fn write_sheet_name(formatter: &mut fmt::Formatter<'_>, sheet: &str) -> fmt::Result {
    if sheet
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic())
        && sheet
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        write!(formatter, "{sheet}")
    } else {
        write!(formatter, "'{}'", sheet.replace('\'', "''"))
    }
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

/// Rewrites relative A1 references as if a formula were copied by the given
/// offset. Absolute row and column markers are retained independently.
pub fn translate_formula(
    input: &str,
    column_delta: i16,
    row_delta: i32,
) -> Result<String, FormulaError> {
    let expression = parse_formula(input)?;
    let translated =
        translate_expression(&expression, column_delta, row_delta).map_err(|error| {
            FormulaError {
                position: 0,
                message: error.to_string(),
            }
        })?;
    Ok(format!("={translated}"))
}

fn translate_expression(
    expression: &Expr,
    column_delta: i16,
    row_delta: i32,
) -> Result<Expr, AddressError> {
    Ok(match expression {
        Expr::Number(number) => Expr::Number(*number),
        Expr::Text(text) => Expr::Text(text.clone()),
        Expr::Reference(reference) => {
            Expr::Reference(reference.translated(column_delta, row_delta)?)
        }
        Expr::Range(range) => Expr::Range(range.translated(column_delta, row_delta)?),
        Expr::Unary { operator, operand } => Expr::Unary {
            operator: *operator,
            operand: Box::new(translate_expression(operand, column_delta, row_delta)?),
        },
        Expr::Binary {
            left,
            operator,
            right,
        } => Expr::Binary {
            left: Box::new(translate_expression(left, column_delta, row_delta)?),
            operator: *operator,
            right: Box::new(translate_expression(right, column_delta, row_delta)?),
        },
        Expr::Function {
            function,
            arguments,
        } => Expr::Function {
            function: *function,
            arguments: arguments
                .iter()
                .map(|argument| translate_expression(argument, column_delta, row_delta))
                .collect::<Result<_, _>>()?,
        },
    })
}

struct Parser<'a> {
    source: &'a str,
    position: usize,
}

impl<'a> Parser<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            position: 0,
        }
    }

    fn parse_expression(&mut self) -> Result<Expr, FormulaError> {
        let mut expression = self.parse_additive()?;
        loop {
            self.skip_whitespace();
            let operator = if self.consume("<=") {
                Some(BinaryOperator::LessThanOrEqual)
            } else if self.consume(">=") {
                Some(BinaryOperator::GreaterThanOrEqual)
            } else if self.consume("<>") {
                Some(BinaryOperator::NotEqual)
            } else if self.consume("=") {
                Some(BinaryOperator::Equal)
            } else if self.consume("<") {
                Some(BinaryOperator::LessThan)
            } else if self.consume(">") {
                Some(BinaryOperator::GreaterThan)
            } else {
                None
            };
            let Some(operator) = operator else { break };
            let right = self.parse_additive()?;
            expression = Expr::Binary {
                left: Box::new(expression),
                operator,
                right: Box::new(right),
            };
        }
        Ok(expression)
    }

    fn parse_additive(&mut self) -> Result<Expr, FormulaError> {
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
            expression = Expr::Binary {
                left: Box::new(expression),
                operator,
                right: Box::new(right),
            };
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
            expression = Expr::Binary {
                left: Box::new(expression),
                operator,
                right: Box::new(right),
            };
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
            return Ok(Expr::Unary {
                operator,
                operand: Box::new(self.parse_unary()?),
            });
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
            Some('"') => self.parse_text(),
            Some(character) if character.is_ascii_digit() || character == '.' => {
                self.parse_number()
            }
            Some('\'') => self.parse_quoted_sheet_reference(),
            Some(character) if character.is_ascii_alphabetic() || character == '$' => {
                self.parse_name_or_reference()
            }
            Some(_) => Err(self
                .error("expected a number, cell reference, function, or parenthesized expression")),
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

    fn parse_text(&mut self) -> Result<Expr, FormulaError> {
        self.advance();
        let mut value = String::new();
        loop {
            match self.peek() {
                Some('"') => {
                    self.advance();
                    if self.peek() == Some('"') {
                        self.advance();
                        value.push('"');
                    } else {
                        return Ok(Expr::Text(value));
                    }
                }
                Some(character) => {
                    self.advance();
                    value.push(character);
                }
                None => return Err(self.error("unterminated text literal")),
            }
        }
    }

    fn parse_name_or_reference(&mut self) -> Result<Expr, FormulaError> {
        let start = self.position;
        if self.peek() == Some('$') {
            self.advance();
        }
        while self
            .peek()
            .is_some_and(|character| character.is_ascii_alphanumeric() || character == '_')
        {
            self.advance();
        }
        let name_end = self.position;
        self.skip_whitespace();
        if self.peek() == Some('!') {
            let sheet = self.source[start..name_end].to_owned();
            self.advance();
            return self.parse_reference(Some(sheet));
        }
        if self.peek() == Some('(') {
            let name = self.source[start..name_end].to_owned();
            return self.parse_function(&name);
        }

        self.position = start;
        self.parse_reference(None)
    }

    fn parse_quoted_sheet_reference(&mut self) -> Result<Expr, FormulaError> {
        self.advance();
        let mut sheet = String::new();
        loop {
            match self.peek() {
                Some('\'') => {
                    self.advance();
                    if self.peek() == Some('\'') {
                        self.advance();
                        sheet.push('\'');
                    } else {
                        break;
                    }
                }
                Some(character) => {
                    self.advance();
                    sheet.push(character);
                }
                None => return Err(self.error("unterminated sheet name")),
            }
        }
        self.expect('!')?;
        self.parse_reference(Some(sheet))
    }

    fn parse_reference(&mut self, sheet: Option<String>) -> Result<Expr, FormulaError> {
        let start = self.position;
        if self.peek() == Some('$') {
            self.advance();
        }
        while self
            .peek()
            .is_some_and(|character| character.is_ascii_alphabetic())
        {
            self.advance();
        }
        if self.peek() == Some('$') {
            self.advance();
        }
        while self
            .peek()
            .is_some_and(|character| character.is_ascii_digit())
        {
            self.advance();
        }
        let address = self.source[start..self.position]
            .parse::<CellReference>()
            .map_err(|_| self.error("invalid cell reference"))?;
        self.skip_whitespace();
        if self.peek() != Some(':') {
            return Ok(Expr::Reference(FormulaReference::new(sheet, address)));
        }
        self.advance();
        self.skip_whitespace();
        let range_start = self.position;
        if self.peek() == Some('$') {
            self.advance();
        }
        while self
            .peek()
            .is_some_and(|character| character.is_ascii_alphabetic())
        {
            self.advance();
        }
        if self.peek() == Some('$') {
            self.advance();
        }
        while self
            .peek()
            .is_some_and(|character| character.is_ascii_digit())
        {
            self.advance();
        }
        let end = self.source[range_start..self.position]
            .parse::<CellReference>()
            .map_err(|_| self.error("invalid range end"))?;
        Ok(Expr::Range(FormulaRange::new(sheet, address, end)))
    }

    fn parse_function(&mut self, name: &str) -> Result<Expr, FormulaError> {
        let function = match name.trim_start_matches('$').to_ascii_uppercase().as_str() {
            "SUM" => AggregateFunction::Sum,
            "AVG" | "AVERAGE" => AggregateFunction::Average,
            "MIN" => AggregateFunction::Minimum,
            "MAX" => AggregateFunction::Maximum,
            "COUNT" => AggregateFunction::Count,
            "COUNTA" => AggregateFunction::CountA,
            "IF" => AggregateFunction::If,
            "AND" => AggregateFunction::And,
            "OR" => AggregateFunction::Or,
            "NOT" => AggregateFunction::Not,
            "CONCAT" | "CONCATENATE" => AggregateFunction::Concat,
            "LEN" => AggregateFunction::Length,
            "LOWER" => AggregateFunction::Lower,
            "UPPER" => AggregateFunction::Upper,
            "TRIM" => AggregateFunction::Trim,
            "LEFT" => AggregateFunction::Left,
            "RIGHT" => AggregateFunction::Right,
            "ABS" => AggregateFunction::Absolute,
            "ROUND" => AggregateFunction::Round,
            "POWER" | "POW" => AggregateFunction::Power,
            "SQRT" => AggregateFunction::SquareRoot,
            "IFERROR" => AggregateFunction::IfError,
            "DATE" => AggregateFunction::Date,
            "YEAR" => AggregateFunction::Year,
            "MONTH" => AggregateFunction::Month,
            "DAY" => AggregateFunction::Day,
            "XLOOKUP" => AggregateFunction::XLookup,
            "PX_LAST" => AggregateFunction::PriceLast,
            "PX_CHANGE" => AggregateFunction::PriceChange,
            "HISTORY" => AggregateFunction::History,
            "FUNDAMENTAL" => AggregateFunction::Fundamental,
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
        Ok(Expr::Function {
            function,
            arguments,
        })
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

    fn consume(&mut self, expected: &str) -> bool {
        if self.source[self.position..].starts_with(expected) {
            self.position += expected.len();
            true
        } else {
            false
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
        FormulaError {
            position: self.position,
            message: message.to_owned(),
        }
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
        assert!(matches!(
            expression,
            Expr::Binary {
                operator: BinaryOperator::Add,
                ..
            }
        ));
        let Expr::Binary { right, .. } = expression else {
            unreachable!()
        };
        assert!(matches!(
            *right,
            Expr::Binary {
                operator: BinaryOperator::Multiply,
                ..
            }
        ));
    }

    #[test]
    fn parser_accepts_ranges_and_case_insensitive_functions() {
        let expression = parse_formula("average(a1:B3, 10)").unwrap();
        let Expr::Function {
            function,
            arguments,
        } = expression
        else {
            panic!("expected function")
        };
        assert_eq!(function, AggregateFunction::Average);
        assert!(matches!(arguments[0], Expr::Range(_)));
        assert_eq!(arguments[1], Expr::Number(10.0));
    }

    #[test]
    fn parser_rejects_unknown_functions_and_trailing_input() {
        assert!(parse_formula("NOPE(A1)").is_err());
        assert!(parse_formula("1 + 2 hello").is_err());
    }

    #[test]
    fn parser_accepts_comparisons_text_and_richer_functions() {
        let expression =
            parse_formula("=IF(A1 >= 10, CONCAT(\"large \"\"position\"\"\", A1), \"small\")")
                .unwrap();
        let Expr::Function {
            function,
            arguments,
        } = expression
        else {
            panic!("expected function")
        };
        assert_eq!(function, AggregateFunction::If);
        assert!(matches!(
            arguments[0],
            Expr::Binary {
                operator: BinaryOperator::GreaterThanOrEqual,
                ..
            }
        ));
        assert!(matches!(
            arguments[1],
            Expr::Function {
                function: AggregateFunction::Concat,
                ..
            }
        ));
    }

    #[test]
    fn parser_and_translation_support_financial_functions() {
        let price = parse_formula("=PX_LAST(A1)").unwrap();
        assert!(matches!(
            price,
            Expr::Function {
                function: AggregateFunction::PriceLast,
                ..
            }
        ));
        let change = parse_formula("=PX_CHANGE(\"SPY US Equity\", \"1D\")").unwrap();
        assert!(matches!(
            change,
            Expr::Function {
                function: AggregateFunction::PriceChange,
                ..
            }
        ));
        assert_eq!(
            translate_formula("=PX_LAST(A1)", 1, 2).unwrap(),
            "=PX_LAST(B3)"
        );

        let history =
            parse_formula("=HISTORY(A1, \"PX_LAST\", \"2026-01-01\", \"2026-08-26\")").unwrap();
        assert!(matches!(
            history,
            Expr::Function {
                function: AggregateFunction::History,
                ..
            }
        ));
        let fundamental = parse_formula("=FUNDAMENTAL(A1, \"REVENUE\", \"FY2025\")").unwrap();
        assert!(matches!(
            fundamental,
            Expr::Function {
                function: AggregateFunction::Fundamental,
                ..
            }
        ));
    }

    #[test]
    fn parser_accepts_extended_pure_functions() {
        for (formula, expected) in [
            ("=LOWER(\"IBM\")", AggregateFunction::Lower),
            ("=UPPER(\"ibm\")", AggregateFunction::Upper),
            ("=TRIM(\"  IBM  \")", AggregateFunction::Trim),
            ("=LEFT(\"IBM\", 2)", AggregateFunction::Left),
            ("=RIGHT(\"IBM\", 2)", AggregateFunction::Right),
            ("=POWER(2, 8)", AggregateFunction::Power),
            ("=SQRT(81)", AggregateFunction::SquareRoot),
            ("=IFERROR(1 / 0, 0)", AggregateFunction::IfError),
            ("=DATE(2026, 8, 27)", AggregateFunction::Date),
            ("=YEAR(\"2026-08-27\")", AggregateFunction::Year),
            ("=MONTH(\"2026-08-27\")", AggregateFunction::Month),
            ("=DAY(\"2026-08-27\")", AggregateFunction::Day),
        ] {
            assert!(matches!(
                parse_formula(formula).unwrap(),
                Expr::Function { function, .. } if function == expected
            ));
        }
    }

    #[test]
    fn parser_reports_unterminated_text() {
        assert!(parse_formula("=\"unterminated").is_err());
    }

    #[test]
    fn parser_accepts_qualified_and_quoted_sheet_references() {
        let expression = parse_formula("=Inputs!$B2 + 'Base Case'!C$4").unwrap();
        let Expr::Binary { left, right, .. } = expression else {
            panic!("expected binary")
        };
        let Expr::Reference(left) = *left else {
            panic!("expected reference")
        };
        let Expr::Reference(right) = *right else {
            panic!("expected reference")
        };
        assert_eq!(left.sheet(), Some("Inputs"));
        assert_eq!(left.cell().to_string(), "$B2");
        assert_eq!(right.sheet(), Some("Base Case"));
        assert_eq!(right.cell().to_string(), "C$4");
    }

    #[test]
    fn translation_respects_mixed_absolute_axes_and_sheet_names() {
        let translated =
            translate_formula("=A1 + $A1 + A$1 + $A$1 + 'Base Case'!B2", 2, 3).unwrap();
        assert_eq!(translated, "=C4 + $A4 + C$1 + $A$1 + 'Base Case'!D5");
    }
}
