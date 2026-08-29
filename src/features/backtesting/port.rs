use std::fmt;

use super::{BacktestArtifact, BacktestBar};

pub const MAX_SAVED_BACKTEST_ARTIFACTS: usize = 64;
pub const MAX_BACKTEST_EXPORT_BYTES: usize = 8 * 1_048_576;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BacktestArtifactSummary {
    pub run_digest: String,
    pub artifact_digest: String,
    pub symbol: String,
    pub strategy: String,
    pub input_version: String,
    pub total_return_bps: i32,
    pub first_timestamp: i64,
    pub last_timestamp: i64,
}

impl From<&BacktestArtifact> for BacktestArtifactSummary {
    fn from(artifact: &BacktestArtifact) -> Self {
        Self {
            run_digest: artifact.run_digest.clone(),
            artifact_digest: artifact.artifact_digest.clone(),
            symbol: artifact.symbol.clone(),
            strategy: artifact.strategy.clone(),
            input_version: artifact.input_version.clone(),
            total_return_bps: artifact.total_return_bps,
            first_timestamp: artifact.first_timestamp,
            last_timestamp: artifact.last_timestamp,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BacktestArtifactError {
    Io(String),
    Corrupt(String),
    Unsupported(String),
    Capacity,
    NotFound(String),
    ImmutableConflict(String),
    InvalidLocation(String),
    AlreadyExists(String),
    TooLarge,
}

impl fmt::Display for BacktestArtifactError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(message) => write!(formatter, "artifact I/O failed: {message}"),
            Self::Corrupt(message) => write!(formatter, "artifact is corrupt: {message}"),
            Self::Unsupported(message) => write!(formatter, "artifact is unsupported: {message}"),
            Self::Capacity => write!(
                formatter,
                "artifact catalog reached its {MAX_SAVED_BACKTEST_ARTIFACTS}-run limit"
            ),
            Self::NotFound(id) => write!(formatter, "artifact {id} was not found"),
            Self::ImmutableConflict(id) => {
                write!(
                    formatter,
                    "immutable artifact {id} conflicts with stored content"
                )
            }
            Self::InvalidLocation(message) => {
                write!(formatter, "invalid export location: {message}")
            }
            Self::AlreadyExists(path) => write!(formatter, "export already exists: {path}"),
            Self::TooLarge => write!(formatter, "artifact exceeds the export size limit"),
        }
    }
}

impl std::error::Error for BacktestArtifactError {}

/// Immutable durable run boundary. Saving the same run is idempotent; a
/// different payload under an existing run digest must fail closed.
pub trait BacktestArtifactStore: Send + Sync {
    fn save_artifact(&self, artifact: &BacktestArtifact) -> Result<bool, BacktestArtifactError>;
    fn load_artifact(&self, run_digest: &str) -> Result<BacktestArtifact, BacktestArtifactError>;
    fn list_artifacts(&self) -> Result<Vec<BacktestArtifactSummary>, BacktestArtifactError>;
    fn delete_artifact(&self, run_digest: &str) -> Result<bool, BacktestArtifactError>;
}

/// Explicit portable JSON export boundary. Files are private and never
/// overwritten unless the command uses the opt-in overwrite form.
pub trait BacktestArtifactFileStore: Send + Sync {
    fn write_artifact(
        &self,
        location: &str,
        document: &str,
        overwrite: bool,
    ) -> Result<(), BacktestArtifactError>;
}
