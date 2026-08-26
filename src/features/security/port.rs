use std::fmt;

use super::SecurityPage;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecurityError {
    Unavailable(String),
    PermissionDenied(String),
}

impl fmt::Display for SecurityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(message) => write!(formatter, "security data unavailable: {message}"),
            Self::PermissionDenied(message) => {
                write!(formatter, "security data permission denied: {message}")
            }
        }
    }
}

impl std::error::Error for SecurityError {}

pub trait SecurityQuery: Send + Sync {
    fn load_security(&self, symbol: &str) -> Result<SecurityPage, SecurityError>;

    fn request_refresh(&self, _symbol: &str) {}
}
