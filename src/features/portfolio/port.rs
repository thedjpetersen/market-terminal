use std::{fmt, path::Path};

use super::PortfolioSnapshot;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortfolioError {
    Unsupported(String),
    Io(String),
    InvalidCsv(String),
}

impl fmt::Display for PortfolioError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(message) | Self::Io(message) | Self::InvalidCsv(message) => {
                formatter.write_str(message)
            }
        }
    }
}

impl std::error::Error for PortfolioError {}

pub trait PortfolioRepository: Send + Sync {
    fn load_portfolio(&self) -> PortfolioSnapshot;

    fn import_csv(&self, _path: &Path) -> Result<PortfolioSnapshot, PortfolioError> {
        Err(PortfolioError::Unsupported(
            "THIS PORTFOLIO PROVIDER DOES NOT SUPPORT CSV IMPORT".to_owned(),
        ))
    }

    fn reload(&self) -> Result<PortfolioSnapshot, PortfolioError> {
        Err(PortfolioError::Unsupported(
            "NO IMPORTED PORTFOLIO TO RELOAD".to_owned(),
        ))
    }
}
