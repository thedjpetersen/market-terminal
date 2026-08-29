use std::fmt;

use super::BacktestBar;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BacktestHistoryRequest {
    pub instrument_id: String,
    pub symbol: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BacktestHistorySnapshot {
    pub instrument_id: String,
    pub symbol: String,
    pub bars: Vec<BacktestBar>,
    pub source: String,
    pub quality: String,
    pub input_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BacktestHistoryError {
    Unavailable(String),
    PermissionDenied(String),
    Invalid(String),
}

impl fmt::Display for BacktestHistoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(message) => {
                write!(formatter, "backtest history unavailable: {message}")
            }
            Self::PermissionDenied(message) => {
                write!(formatter, "backtest history permission denied: {message}")
            }
            Self::Invalid(message) => write!(formatter, "backtest history invalid: {message}"),
        }
    }
}

impl std::error::Error for BacktestHistoryError {}

/// Backtesting-owned point-in-time history boundary. Composition-root adapters
/// translate provider or chart history without exposing another feature's model.
pub trait BacktestHistoryQuery: Send + Sync {
    fn load_history(
        &self,
        request: &BacktestHistoryRequest,
    ) -> Result<BacktestHistorySnapshot, BacktestHistoryError>;
}
