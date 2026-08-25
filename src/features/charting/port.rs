use std::fmt;

use super::{ChartInstrument, ChartPeriod, HistorySeries};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryRequest {
    pub instrument: ChartInstrument,
    pub period: ChartPeriod,
}

impl HistoryRequest {
    pub fn new(instrument: ChartInstrument, period: ChartPeriod) -> Self {
        Self { instrument, period }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryError {
    Unavailable(String),
    PermissionDenied(String),
}

impl fmt::Display for HistoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(message) => write!(formatter, "history unavailable: {message}"),
            Self::PermissionDenied(message) => write!(formatter, "history permission denied: {message}"),
        }
    }
}

impl std::error::Error for HistoryError {}

/// Feature-owned outbound port for chart-ready historical prices.
pub trait ChartHistoryQuery: Send + Sync {
    fn load_history(&self, request: &HistoryRequest) -> Result<HistorySeries, HistoryError>;
}
