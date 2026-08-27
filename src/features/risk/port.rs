use std::fmt;

use super::RiskSnapshot;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RiskError {
    Unavailable(String),
    InvalidInput(String),
}

impl fmt::Display for RiskError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(message) => write!(formatter, "risk unavailable: {message}"),
            Self::InvalidInput(message) => write!(formatter, "risk input invalid: {message}"),
        }
    }
}

impl std::error::Error for RiskError {}

/// Versioned risk read boundary owned by Risk. Implementations may translate a
/// Portfolio snapshot, but Risk never imports or reaches into Portfolio state.
pub trait RiskQuery: Send + Sync {
    fn load_risk(&self) -> Result<RiskSnapshot, RiskError>;
}
