use std::{collections::BTreeSet, fmt};

use super::{AlertRule, AlertSnapshot, InstrumentRef, MAX_ALERT_RULES};

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AlertStateError {
    Io(String),
    Corrupt(String),
    Unsupported(String),
}

impl fmt::Display for AlertStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(message) => write!(formatter, "alert state I/O failed: {message}"),
            Self::Corrupt(message) => write!(formatter, "alert state is corrupt: {message}"),
            Self::Unsupported(message) => {
                write!(formatter, "alert state is unsupported: {message}")
            }
        }
    }
}

impl std::error::Error for AlertStateError {}

#[derive(Debug, Clone, PartialEq)]
pub struct AlertRulesState {
    pub revision: u64,
    pub rules: Vec<AlertRule>,
}

impl AlertRulesState {
    pub fn new(revision: u64, rules: Vec<AlertRule>) -> Result<Self, AlertStateError> {
        if rules.len() > MAX_ALERT_RULES {
            return Err(AlertStateError::Corrupt(format!(
                "rule count exceeds {MAX_ALERT_RULES}"
            )));
        }
        let mut ids = BTreeSet::new();
        if rules
            .iter()
            .any(|rule| !ids.insert(rule.id.as_str().to_owned()))
        {
            return Err(AlertStateError::Corrupt(
                "rule IDs must be unique".to_owned(),
            ));
        }
        Ok(Self { revision, rules })
    }
}

/// Replay/read-model boundary owned by the Alerts bounded context.
///
/// Implementations may read market events or persisted alert state, but the
/// workspace only sees this context's deterministic snapshot vocabulary.
pub trait AlertsQuery: Send + Sync {
    fn load_snapshot(&self, instruments: &[InstrumentRef]) -> Result<AlertSnapshot, AlertsError>;
}

/// Durable state boundary owned by Alerts. Implementations persist complete
/// rule runtime state so debounce, idempotency, and audit survive restarts.
pub trait AlertStateStore: Send + Sync {
    fn load_alert_rules(&self) -> Result<Option<AlertRulesState>, AlertStateError>;
    fn save_alert_rules(&self, state: &AlertRulesState) -> Result<(), AlertStateError>;
}
