use std::fmt;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Cell {
    raw: String,
}

impl Cell {
    pub fn new(raw: impl Into<String>) -> Self {
        Self { raw: raw.into() }
    }

    pub fn raw(&self) -> &str {
        &self.raw
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CellValue {
    Blank,
    Number(f64),
    Text(String),
    Error(CellError),
}

impl CellValue {
    pub fn as_number(&self) -> Option<f64> {
        match self {
            Self::Number(number) => Some(*number),
            _ => None,
        }
    }
}

impl fmt::Display for CellValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Blank => Ok(()),
            Self::Number(number) => write!(formatter, "{number}"),
            Self::Text(text) => formatter.write_str(text),
            Self::Error(error) => error.fmt(formatter),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CellError {
    Parse,
    DivisionByZero,
    InvalidValue,
    UnknownFunction,
    CircularReference,
    InvalidReference,
    NotAvailable,
}

impl fmt::Display for CellError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = match self {
            Self::Parse => "#PARSE!",
            Self::DivisionByZero => "#DIV/0!",
            Self::InvalidValue => "#VALUE!",
            Self::UnknownFunction => "#NAME?",
            Self::CircularReference => "#CYCLE!",
            Self::InvalidReference => "#REF!",
            Self::NotAvailable => "#N/A",
        };
        formatter.write_str(code)
    }
}
