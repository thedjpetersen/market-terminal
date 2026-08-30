use std::fmt;

use super::LaunchpadState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchpadStateError {
    Io(String),
    Corrupt(String),
    Unsupported(String),
}

impl fmt::Display for LaunchpadStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(message) => write!(formatter, "launchpad state I/O failed: {message}"),
            Self::Corrupt(message) => write!(formatter, "launchpad state is corrupt: {message}"),
            Self::Unsupported(message) => {
                write!(formatter, "launchpad state is unsupported: {message}")
            }
        }
    }
}

impl std::error::Error for LaunchpadStateError {}

pub trait LaunchpadStateStore: Send + Sync {
    fn load_launchpad(&self) -> Result<Option<LaunchpadState>, LaunchpadStateError>;
    fn save_launchpad(&self, state: &LaunchpadState) -> Result<(), LaunchpadStateError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchpadFileError {
    Io(String),
    InvalidLocation(String),
    AlreadyExists(String),
    TooLarge,
}

impl fmt::Display for LaunchpadFileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(message) => write!(formatter, "launchpad file I/O failed: {message}"),
            Self::InvalidLocation(message) => {
                write!(formatter, "invalid launchpad file: {message}")
            }
            Self::AlreadyExists(path) => write!(formatter, "launchpad file already exists: {path}"),
            Self::TooLarge => formatter.write_str("launchpad file exceeds 64 KiB"),
        }
    }
}

impl std::error::Error for LaunchpadFileError {}

pub trait LaunchpadFileStore: Send + Sync {
    fn read_document(&self, location: &str) -> Result<String, LaunchpadFileError>;
    fn write_document(
        &self,
        location: &str,
        document: &str,
        overwrite: bool,
    ) -> Result<(), LaunchpadFileError>;
}
