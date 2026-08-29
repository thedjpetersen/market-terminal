use std::fmt;

use super::{ScreenCatalogState, UniverseHistoryManifest, UniverseSnapshot};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScreeningError {
    UniverseNotFound(String),
    TemporarilyUnavailable(String),
    PermissionDenied(String),
    InvalidSnapshot(String),
}

impl fmt::Display for ScreeningError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UniverseNotFound(id) => write!(formatter, "screening universe not found: {id}"),
            Self::TemporarilyUnavailable(message) => {
                write!(formatter, "screening data unavailable: {message}")
            }
            Self::PermissionDenied(message) => {
                write!(formatter, "screening data denied: {message}")
            }
            Self::InvalidSnapshot(message) => {
                write!(formatter, "screening snapshot invalid: {message}")
            }
        }
    }
}

impl std::error::Error for ScreeningError {}

pub trait ScreeningUniverseQuery: Send + Sync {
    fn load_universe(&self, id: &str) -> Result<UniverseSnapshot, ScreeningError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScreenStateError {
    Io(String),
    Corrupt(String),
    Unsupported(String),
}

impl fmt::Display for ScreenStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(message) => write!(formatter, "screen catalog I/O failed: {message}"),
            Self::Corrupt(message) => write!(formatter, "screen catalog is corrupt: {message}"),
            Self::Unsupported(message) => {
                write!(formatter, "screen catalog is unsupported: {message}")
            }
        }
    }
}

impl std::error::Error for ScreenStateError {}

pub trait ScreenStateStore: Send + Sync {
    fn load_screens(&self) -> Result<Option<ScreenCatalogState>, ScreenStateError>;
    fn save_screens(&self, state: &ScreenCatalogState) -> Result<(), ScreenStateError>;
}

/// Durable immutable point-in-time inputs are independent of saved screen
/// definitions and saved workspace views. Implementations must publish a
/// snapshot before referencing it from the bounded manifest.
pub trait UniverseHistoryStore: Send + Sync {
    fn load_history(&self) -> Result<UniverseHistoryManifest, ScreenStateError>;
    fn load_snapshot(&self, version: u64) -> Result<UniverseSnapshot, ScreenStateError>;
    fn record_snapshot(
        &self,
        snapshot: &UniverseSnapshot,
    ) -> Result<UniverseHistoryManifest, ScreenStateError>;
}
