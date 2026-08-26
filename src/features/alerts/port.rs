use std::fmt;

use super::{AlertSnapshot, InstrumentRef};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AlertsError {
    Unavailable(String),
    PermissionDenied(String),
}

impl fmt::Display for AlertsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(message) => {
                write!(formatter, "alert observations unavailable: {message}")
            }
            Self::PermissionDenied(message) => {
                write!(formatter, "alert observations permission denied: {message}")
            }
        }
    }
}

impl std::error::Error for AlertsError {}

/// Replay/read-model boundary owned by the Alerts bounded context.
///
/// Implementations may read market events or persisted alert state, but the
/// workspace only sees this context's deterministic snapshot vocabulary.
pub trait AlertsQuery: Send + Sync {
    fn load_snapshot(&self, instruments: &[InstrumentRef]) -> Result<AlertSnapshot, AlertsError>;
}
