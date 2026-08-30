use std::{fmt, io};

use super::{DocumentId, FeatureDocument, FeatureKey, PersistenceValidationError, SessionState};

pub trait SessionStateRepository: Send + Sync {
    fn load(&self) -> Result<Option<SessionState>, PersistenceError>;
    fn save(&self, state: &SessionState) -> Result<(), PersistenceError>;
}

pub trait FeatureDocumentRepository: Send + Sync {
    fn load(
        &self,
        feature: &FeatureKey,
        id: &DocumentId,
    ) -> Result<Option<FeatureDocument>, PersistenceError>;
    fn save(&self, document: &FeatureDocument) -> Result<(), PersistenceError>;
    fn list(&self, feature: &FeatureKey) -> Result<Vec<DocumentId>, PersistenceError>;
    fn delete(&self, feature: &FeatureKey, id: &DocumentId) -> Result<bool, PersistenceError>;
}

#[derive(Debug)]
pub enum PersistenceError {
    Io(io::Error),
    Corrupt(String),
    UnsupportedVersion { schema: String, version: u64 },
    Validation(PersistenceValidationError),
    PayloadTooLarge,
}

impl fmt::Display for PersistenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "persistence I/O failed: {error}"),
            Self::Corrupt(message) => write!(formatter, "persisted state is corrupt: {message}"),
            Self::UnsupportedVersion { schema, version } => {
                write!(formatter, "unsupported {schema} schema version {version}")
            }
            Self::Validation(error) => error.fmt(formatter),
            Self::PayloadTooLarge => write!(formatter, "persisted payload exceeds its size limit"),
        }
    }
}

impl std::error::Error for PersistenceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Validation(error) => Some(error),
            Self::Corrupt(_) | Self::UnsupportedVersion { .. } | Self::PayloadTooLarge => None,
        }
    }
}

impl From<io::Error> for PersistenceError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<PersistenceValidationError> for PersistenceError {
    fn from(error: PersistenceValidationError) -> Self {
        Self::Validation(error)
    }
}
