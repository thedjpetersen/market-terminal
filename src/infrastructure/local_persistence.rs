use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
};

use serde::{de::DeserializeOwned, Deserialize, Serialize};

use super::alert_state::{decode_alert_rules, encode_alert_rules};
use crate::features::alerts::{AlertRulesState, AlertStateError, AlertStateStore};
use crate::features::persistence::{
    DocumentId, FeatureDocument, FeatureDocumentRepository, FeatureKey, PersistenceError,
    SessionState, SessionStateRepository, MAX_DOCUMENT_BYTES,
};
use crate::features::portfolio::{PortfolioError, PortfolioImportStateStore};
use crate::features::spreadsheet::{
    SpreadsheetFileError, SpreadsheetWorkbookStore, StoredWorkbook,
};

const SESSION_SCHEMA: &str = "market-terminal.session";
const DOCUMENT_SCHEMA: &str = "market-terminal.feature-document";
const SESSION_VERSION: u64 = 2;
const DOCUMENT_VERSION: u64 = 1;
const MAX_ENVELOPE_BYTES: usize = MAX_DOCUMENT_BYTES + 16_384;
const ATOMIC_CREATE_ATTEMPTS: u64 = 32;

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Local, crash-safe persistence for one terminal profile.
///
/// Writes are flushed and atomically renamed within the destination directory.
/// The previous valid generation is retained as `.bak` and used only when the
/// current generation is absent or corrupt. A mutex serializes operations made
/// through one repository instance.
pub struct LocalPersistence {
    root: PathBuf,
    operation: Mutex<()>,
}

impl LocalPersistence {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            operation: Mutex::new(()),
        }
    }

    fn session_path(&self) -> PathBuf {
        self.root.join("session.json")
    }

    fn document_path(&self, feature: &FeatureKey, id: &DocumentId) -> PathBuf {
        self.root
            .join("documents")
            .join(feature.as_str())
            .join(format!("{}.json", id.as_str()))
    }

    fn document_directory(&self, feature: &FeatureKey) -> PathBuf {
        self.root.join("documents").join(feature.as_str())
    }

    fn load_session_at(path: &Path) -> Result<Option<SessionState>, PersistenceError> {
        let Some(raw) = read_bounded(path)? else {
            return Ok(None);
        };
        let envelope: RawEnvelope = decode_json(&raw)?;
        if envelope.schema != SESSION_SCHEMA {
            return Err(PersistenceError::Corrupt(format!(
                "expected schema {SESSION_SCHEMA}, found {}",
                envelope.schema
            )));
        }

        let state = match envelope.version {
            1 => migrate_session_v1(envelope.payload)?,
            SESSION_VERSION => serde_json::from_value(envelope.payload)
                .map_err(|error| PersistenceError::Corrupt(error.to_string()))?,
            version => {
                return Err(PersistenceError::UnsupportedVersion {
                    schema: SESSION_SCHEMA.to_owned(),
                    version,
                });
            }
        };
        state.validate()?;
        Ok(Some(state))
    }

    fn load_document_at(path: &Path) -> Result<Option<FeatureDocument>, PersistenceError> {
        let Some(raw) = read_bounded(path)? else {
            return Ok(None);
        };
        let envelope: RawEnvelope = decode_json(&raw)?;
        if envelope.schema != DOCUMENT_SCHEMA {
            return Err(PersistenceError::Corrupt(format!(
                "expected schema {DOCUMENT_SCHEMA}, found {}",
                envelope.schema
            )));
        }
        if envelope.version != DOCUMENT_VERSION {
            return Err(PersistenceError::UnsupportedVersion {
                schema: DOCUMENT_SCHEMA.to_owned(),
                version: envelope.version,
            });
        }
        let document: FeatureDocument = serde_json::from_value(envelope.payload)
            .map_err(|error| PersistenceError::Corrupt(error.to_string()))?;
        document.validate()?;
        Ok(Some(document))
    }
}

impl SpreadsheetWorkbookStore for LocalPersistence {
    fn load_workbook(&self, id: &str) -> Result<Option<StoredWorkbook>, SpreadsheetFileError> {
        let feature = spreadsheet_feature_key()?;
        let id = spreadsheet_document_id(id)?;
        FeatureDocumentRepository::load(self, &feature, &id)
            .map_err(spreadsheet_persistence_error)
            .map(|document| {
                document.map(|document| StoredWorkbook {
                    id: document.id().as_str().to_owned(),
                    revision: document.revision(),
                    payload: document.payload().clone(),
                })
            })
    }

    fn save_workbook(&self, workbook: &StoredWorkbook) -> Result<(), SpreadsheetFileError> {
        let document = FeatureDocument::new(
            spreadsheet_feature_key()?,
            spreadsheet_document_id(&workbook.id)?,
            workbook.revision,
            workbook.payload.clone(),
        )
        .map_err(|error| SpreadsheetFileError::InvalidLocation(error.to_string()))?;
        FeatureDocumentRepository::save(self, &document).map_err(spreadsheet_persistence_error)
    }

    fn list_workbooks(&self) -> Result<Vec<String>, SpreadsheetFileError> {
        FeatureDocumentRepository::list(self, &spreadsheet_feature_key()?)
            .map_err(spreadsheet_persistence_error)
            .map(|ids| ids.into_iter().map(|id| id.as_str().to_owned()).collect())
    }

    fn delete_workbook(&self, id: &str) -> Result<bool, SpreadsheetFileError> {
        FeatureDocumentRepository::delete(
            self,
            &spreadsheet_feature_key()?,
            &spreadsheet_document_id(id)?,
        )
        .map_err(spreadsheet_persistence_error)
    }
}

impl PortfolioImportStateStore for LocalPersistence {
    fn load_import_path(&self) -> Result<Option<PathBuf>, PortfolioError> {
        let feature = portfolio_feature_key()?;
        let id = portfolio_import_document_id()?;
        let document = FeatureDocumentRepository::load(self, &feature, &id)
            .map_err(portfolio_persistence_error)?;
        document
            .map(|document| {
                let path = document
                    .payload()
                    .get("path")
                    .and_then(serde_json::Value::as_str)
                    .filter(|path| !path.trim().is_empty() && path.len() <= 4_096)
                    .ok_or_else(|| {
                        PortfolioError::InvalidCsv(
                            "PERSISTED PORTFOLIO IMPORT PATH IS INVALID".to_owned(),
                        )
                    })?;
                Ok(PathBuf::from(path))
            })
            .transpose()
    }

    fn save_import_path(&self, path: &Path) -> Result<(), PortfolioError> {
        let path = path
            .to_str()
            .filter(|path| path.len() <= 4_096)
            .ok_or_else(|| {
                PortfolioError::Unsupported(
                    "PORTFOLIO IMPORT PATH MUST BE UTF-8 AND AT MOST 4096 BYTES".to_owned(),
                )
            })?;
        let document = FeatureDocument::new(
            portfolio_feature_key()?,
            portfolio_import_document_id()?,
            1,
            serde_json::json!({"path": path}),
        )
        .map_err(|error| PortfolioError::Io(error.to_string()))?;
        FeatureDocumentRepository::save(self, &document).map_err(portfolio_persistence_error)
    }

    fn load_activity_import_path(&self) -> Result<Option<PathBuf>, PortfolioError> {
        let feature = portfolio_feature_key()?;
        let id = portfolio_activity_import_document_id()?;
        let document = FeatureDocumentRepository::load(self, &feature, &id)
            .map_err(portfolio_persistence_error)?;
        document
            .map(|document| {
                let path = document
                    .payload()
                    .get("path")
                    .and_then(serde_json::Value::as_str)
                    .filter(|path| !path.trim().is_empty() && path.len() <= 4_096)
                    .ok_or_else(|| {
                        PortfolioError::InvalidCsv(
                            "PERSISTED PORTFOLIO ACTIVITY IMPORT PATH IS INVALID".to_owned(),
                        )
                    })?;
                Ok(PathBuf::from(path))
            })
            .transpose()
    }

    fn save_activity_import_path(&self, path: &Path) -> Result<(), PortfolioError> {
        let path = path
            .to_str()
            .filter(|path| path.len() <= 4_096)
            .ok_or_else(|| {
                PortfolioError::Unsupported(
                    "PORTFOLIO ACTIVITY IMPORT PATH MUST BE UTF-8 AND AT MOST 4096 BYTES"
                        .to_owned(),
                )
            })?;
        let document = FeatureDocument::new(
            portfolio_feature_key()?,
            portfolio_activity_import_document_id()?,
            1,
            serde_json::json!({"path": path}),
        )
        .map_err(|error| PortfolioError::Io(error.to_string()))?;
        FeatureDocumentRepository::save(self, &document).map_err(portfolio_persistence_error)
    }

    fn load_performance_import_path(&self) -> Result<Option<PathBuf>, PortfolioError> {
        let feature = portfolio_feature_key()?;
        let id = portfolio_performance_import_document_id()?;
        let document = FeatureDocumentRepository::load(self, &feature, &id)
            .map_err(portfolio_persistence_error)?;
        document
            .map(|document| {
                let path = document
                    .payload()
                    .get("path")
                    .and_then(serde_json::Value::as_str)
                    .filter(|path| !path.trim().is_empty() && path.len() <= 4_096)
                    .ok_or_else(|| {
                        PortfolioError::InvalidCsv(
                            "PERSISTED PORTFOLIO PERFORMANCE IMPORT PATH IS INVALID".to_owned(),
                        )
                    })?;
                Ok(PathBuf::from(path))
            })
            .transpose()
    }

    fn save_performance_import_path(&self, path: &Path) -> Result<(), PortfolioError> {
        let path = path
            .to_str()
            .filter(|path| path.len() <= 4_096)
            .ok_or_else(|| {
                PortfolioError::Unsupported(
                    "PORTFOLIO PERFORMANCE IMPORT PATH MUST BE UTF-8 AND AT MOST 4096 BYTES"
                        .to_owned(),
                )
            })?;
        let document = FeatureDocument::new(
            portfolio_feature_key()?,
            portfolio_performance_import_document_id()?,
            1,
            serde_json::json!({"path": path}),
        )
        .map_err(|error| PortfolioError::Io(error.to_string()))?;
        FeatureDocumentRepository::save(self, &document).map_err(portfolio_persistence_error)
    }

    fn load_tax_lot_import_path(&self) -> Result<Option<PathBuf>, PortfolioError> {
        let feature = portfolio_feature_key()?;
        let id = portfolio_tax_lot_import_document_id()?;
        let document = FeatureDocumentRepository::load(self, &feature, &id)
            .map_err(portfolio_persistence_error)?;
        document
            .map(|document| {
                let path = document
                    .payload()
                    .get("path")
                    .and_then(serde_json::Value::as_str)
                    .filter(|path| !path.trim().is_empty() && path.len() <= 4_096)
                    .ok_or_else(|| {
                        PortfolioError::InvalidCsv(
                            "PERSISTED PORTFOLIO TAX-LOT IMPORT PATH IS INVALID".to_owned(),
                        )
                    })?;
                Ok(PathBuf::from(path))
            })
            .transpose()
    }

    fn save_tax_lot_import_path(&self, path: &Path) -> Result<(), PortfolioError> {
        let path = path
            .to_str()
            .filter(|path| path.len() <= 4_096)
            .ok_or_else(|| {
                PortfolioError::Unsupported(
                    "PORTFOLIO TAX-LOT IMPORT PATH MUST BE UTF-8 AND AT MOST 4096 BYTES".to_owned(),
                )
            })?;
        let document = FeatureDocument::new(
            portfolio_feature_key()?,
            portfolio_tax_lot_import_document_id()?,
            1,
            serde_json::json!({"path": path}),
        )
        .map_err(|error| PortfolioError::Io(error.to_string()))?;
        FeatureDocumentRepository::save(self, &document).map_err(portfolio_persistence_error)
    }

    fn load_realized_gain_import_path(&self) -> Result<Option<PathBuf>, PortfolioError> {
        let feature = portfolio_feature_key()?;
        let id = portfolio_realized_gain_import_document_id()?;
        let document = FeatureDocumentRepository::load(self, &feature, &id)
            .map_err(portfolio_persistence_error)?;
        document
            .map(|document| {
                let path = document
                    .payload()
                    .get("path")
                    .and_then(serde_json::Value::as_str)
                    .filter(|path| !path.trim().is_empty() && path.len() <= 4_096)
                    .ok_or_else(|| {
                        PortfolioError::InvalidCsv(
                            "PERSISTED PORTFOLIO CLOSED-LOT IMPORT PATH IS INVALID".to_owned(),
                        )
                    })?;
                Ok(PathBuf::from(path))
            })
            .transpose()
    }

    fn save_realized_gain_import_path(&self, path: &Path) -> Result<(), PortfolioError> {
        let path = path
            .to_str()
            .filter(|path| path.len() <= 4_096)
            .ok_or_else(|| {
                PortfolioError::Unsupported(
                    "PORTFOLIO CLOSED-LOT IMPORT PATH MUST BE UTF-8 AND AT MOST 4096 BYTES"
                        .to_owned(),
                )
            })?;
        let document = FeatureDocument::new(
            portfolio_feature_key()?,
            portfolio_realized_gain_import_document_id()?,
            1,
            serde_json::json!({"path": path}),
        )
        .map_err(|error| PortfolioError::Io(error.to_string()))?;
        FeatureDocumentRepository::save(self, &document).map_err(portfolio_persistence_error)
    }

    fn load_trade_import_path(&self) -> Result<Option<PathBuf>, PortfolioError> {
        let feature = portfolio_feature_key()?;
        let id = portfolio_trade_import_document_id()?;
        let document = FeatureDocumentRepository::load(self, &feature, &id)
            .map_err(portfolio_persistence_error)?;
        document
            .map(|document| {
                let path = document
                    .payload()
                    .get("path")
                    .and_then(serde_json::Value::as_str)
                    .filter(|path| !path.trim().is_empty() && path.len() <= 4_096)
                    .ok_or_else(|| {
                        PortfolioError::InvalidCsv(
                            "PERSISTED PORTFOLIO TRADE IMPORT PATH IS INVALID".to_owned(),
                        )
                    })?;
                Ok(PathBuf::from(path))
            })
            .transpose()
    }

    fn save_trade_import_path(&self, path: &Path) -> Result<(), PortfolioError> {
        let path = path
            .to_str()
            .filter(|path| path.len() <= 4_096)
            .ok_or_else(|| {
                PortfolioError::Unsupported(
                    "PORTFOLIO TRADE IMPORT PATH MUST BE UTF-8 AND AT MOST 4096 BYTES".to_owned(),
                )
            })?;
        let document = FeatureDocument::new(
            portfolio_feature_key()?,
            portfolio_trade_import_document_id()?,
            1,
            serde_json::json!({"path": path}),
        )
        .map_err(|error| PortfolioError::Io(error.to_string()))?;
        FeatureDocumentRepository::save(self, &document).map_err(portfolio_persistence_error)
    }

    fn load_contribution_import_path(&self) -> Result<Option<PathBuf>, PortfolioError> {
        let feature = portfolio_feature_key()?;
        let id = portfolio_contribution_import_document_id()?;
        let document = FeatureDocumentRepository::load(self, &feature, &id)
            .map_err(portfolio_persistence_error)?;
        document
            .map(|document| {
                let path = document
                    .payload()
                    .get("path")
                    .and_then(serde_json::Value::as_str)
                    .filter(|path| !path.trim().is_empty() && path.len() <= 4_096)
                    .ok_or_else(|| {
                        PortfolioError::InvalidCsv(
                            "PERSISTED PORTFOLIO CONTRIBUTION IMPORT PATH IS INVALID".to_owned(),
                        )
                    })?;
                Ok(PathBuf::from(path))
            })
            .transpose()
    }

    fn save_contribution_import_path(&self, path: &Path) -> Result<(), PortfolioError> {
        let path = path
            .to_str()
            .filter(|path| path.len() <= 4_096)
            .ok_or_else(|| {
                PortfolioError::Unsupported(
                    "PORTFOLIO CONTRIBUTION IMPORT PATH MUST BE UTF-8 AND AT MOST 4096 BYTES"
                        .to_owned(),
                )
            })?;
        let document = FeatureDocument::new(
            portfolio_feature_key()?,
            portfolio_contribution_import_document_id()?,
            1,
            serde_json::json!({"path": path}),
        )
        .map_err(|error| PortfolioError::Io(error.to_string()))?;
        FeatureDocumentRepository::save(self, &document).map_err(portfolio_persistence_error)
    }

    fn load_attribution_import_path(&self) -> Result<Option<PathBuf>, PortfolioError> {
        let feature = portfolio_feature_key()?;
        let id = portfolio_attribution_import_document_id()?;
        let document = FeatureDocumentRepository::load(self, &feature, &id)
            .map_err(portfolio_persistence_error)?;
        document
            .map(|document| {
                let path = document
                    .payload()
                    .get("path")
                    .and_then(serde_json::Value::as_str)
                    .filter(|path| !path.trim().is_empty() && path.len() <= 4_096)
                    .ok_or_else(|| {
                        PortfolioError::InvalidCsv(
                            "PERSISTED PORTFOLIO ATTRIBUTION IMPORT PATH IS INVALID".to_owned(),
                        )
                    })?;
                Ok(PathBuf::from(path))
            })
            .transpose()
    }

    fn save_attribution_import_path(&self, path: &Path) -> Result<(), PortfolioError> {
        let path = path
            .to_str()
            .filter(|path| path.len() <= 4_096)
            .ok_or_else(|| {
                PortfolioError::Unsupported(
                    "PORTFOLIO ATTRIBUTION IMPORT PATH MUST BE UTF-8 AND AT MOST 4096 BYTES"
                        .to_owned(),
                )
            })?;
        let document = FeatureDocument::new(
            portfolio_feature_key()?,
            portfolio_attribution_import_document_id()?,
            1,
            serde_json::json!({"path": path}),
        )
        .map_err(|error| PortfolioError::Io(error.to_string()))?;
        FeatureDocumentRepository::save(self, &document).map_err(portfolio_persistence_error)
    }
}

impl AlertStateStore for LocalPersistence {
    fn load_alert_rules(&self) -> Result<Option<AlertRulesState>, AlertStateError> {
        let feature = alerts_feature_key()?;
        let id = alerts_rules_document_id()?;
        FeatureDocumentRepository::load(self, &feature, &id)
            .map_err(alerts_persistence_error)?
            .map(|document| decode_alert_rules(document.revision(), document.payload()))
            .transpose()
    }

    fn save_alert_rules(&self, state: &AlertRulesState) -> Result<(), AlertStateError> {
        let document = FeatureDocument::new(
            alerts_feature_key()?,
            alerts_rules_document_id()?,
            state.revision,
            encode_alert_rules(state)?,
        )
        .map_err(|error| AlertStateError::Corrupt(error.to_string()))?;
        FeatureDocumentRepository::save(self, &document).map_err(alerts_persistence_error)
    }
}

fn spreadsheet_feature_key() -> Result<FeatureKey, SpreadsheetFileError> {
    FeatureKey::new("spreadsheet")
        .map_err(|error| SpreadsheetFileError::InvalidLocation(error.to_string()))
}

fn spreadsheet_document_id(id: &str) -> Result<DocumentId, SpreadsheetFileError> {
    DocumentId::new(id.trim())
        .map_err(|error| SpreadsheetFileError::InvalidLocation(error.to_string()))
}

fn spreadsheet_persistence_error(error: PersistenceError) -> SpreadsheetFileError {
    SpreadsheetFileError::Io(error.to_string())
}

fn portfolio_feature_key() -> Result<FeatureKey, PortfolioError> {
    FeatureKey::new("portfolio").map_err(|error| PortfolioError::Io(error.to_string()))
}

fn portfolio_import_document_id() -> Result<DocumentId, PortfolioError> {
    DocumentId::new("active_import").map_err(|error| PortfolioError::Io(error.to_string()))
}

fn portfolio_activity_import_document_id() -> Result<DocumentId, PortfolioError> {
    DocumentId::new("active_activity_import").map_err(|error| PortfolioError::Io(error.to_string()))
}

fn portfolio_performance_import_document_id() -> Result<DocumentId, PortfolioError> {
    DocumentId::new("active_performance_import")
        .map_err(|error| PortfolioError::Io(error.to_string()))
}

fn portfolio_tax_lot_import_document_id() -> Result<DocumentId, PortfolioError> {
    DocumentId::new("active_tax_lot_import").map_err(|error| PortfolioError::Io(error.to_string()))
}

fn portfolio_realized_gain_import_document_id() -> Result<DocumentId, PortfolioError> {
    DocumentId::new("active_realized_gain_import")
        .map_err(|error| PortfolioError::Io(error.to_string()))
}

fn portfolio_trade_import_document_id() -> Result<DocumentId, PortfolioError> {
    DocumentId::new("active_trade_import").map_err(|error| PortfolioError::Io(error.to_string()))
}

fn portfolio_contribution_import_document_id() -> Result<DocumentId, PortfolioError> {
    DocumentId::new("active_contribution_import")
        .map_err(|error| PortfolioError::Io(error.to_string()))
}

fn portfolio_attribution_import_document_id() -> Result<DocumentId, PortfolioError> {
    DocumentId::new("active_attribution_import")
        .map_err(|error| PortfolioError::Io(error.to_string()))
}

fn portfolio_persistence_error(error: PersistenceError) -> PortfolioError {
    PortfolioError::Io(error.to_string())
}

fn alerts_feature_key() -> Result<FeatureKey, AlertStateError> {
    FeatureKey::new("alerts").map_err(|error| AlertStateError::Corrupt(error.to_string()))
}

fn alerts_rules_document_id() -> Result<DocumentId, AlertStateError> {
    DocumentId::new("rule_register").map_err(|error| AlertStateError::Corrupt(error.to_string()))
}

fn alerts_persistence_error(error: PersistenceError) -> AlertStateError {
    match error {
        PersistenceError::Corrupt(message) => AlertStateError::Corrupt(message),
        PersistenceError::UnsupportedVersion { schema, version } => {
            AlertStateError::Unsupported(format!("{schema} version {version}"))
        }
        error => AlertStateError::Io(error.to_string()),
    }
}

impl SessionStateRepository for LocalPersistence {
    fn load(&self) -> Result<Option<SessionState>, PersistenceError> {
        let _guard = self
            .operation
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        load_with_fallback(&self.session_path(), Self::load_session_at)
    }

    fn save(&self, state: &SessionState) -> Result<(), PersistenceError> {
        state.validate()?;
        let _guard = self
            .operation
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let bytes = encode_envelope(SESSION_SCHEMA, SESSION_VERSION, state)?;
        write_with_backup(&self.session_path(), &bytes, Self::load_session_at)
    }
}

impl FeatureDocumentRepository for LocalPersistence {
    fn load(
        &self,
        feature: &FeatureKey,
        id: &DocumentId,
    ) -> Result<Option<FeatureDocument>, PersistenceError> {
        let _guard = self
            .operation
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        load_with_fallback(&self.document_path(feature, id), |path| {
            let document = Self::load_document_at(path)?;
            if document
                .as_ref()
                .is_some_and(|document| document.feature() != feature || document.id() != id)
            {
                return Err(PersistenceError::Corrupt(
                    "document identity does not match its storage key".to_owned(),
                ));
            }
            Ok(document)
        })
    }

    fn save(&self, document: &FeatureDocument) -> Result<(), PersistenceError> {
        document.validate()?;
        let _guard = self
            .operation
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let bytes = encode_envelope(DOCUMENT_SCHEMA, DOCUMENT_VERSION, document)?;
        write_with_backup(
            &self.document_path(document.feature(), document.id()),
            &bytes,
            Self::load_document_at,
        )
    }

    fn list(&self, feature: &FeatureKey) -> Result<Vec<DocumentId>, PersistenceError> {
        let _guard = self
            .operation
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let directory = self.document_directory(feature);
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };

        let mut ids = Vec::new();
        for entry in entries {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            let Some(stem) = name.strip_suffix(".json") else {
                continue;
            };
            if let Ok(id) = DocumentId::new(stem) {
                ids.push(id);
            }
        }
        ids.sort();
        ids.dedup();
        Ok(ids)
    }

    fn delete(&self, feature: &FeatureKey, id: &DocumentId) -> Result<bool, PersistenceError> {
        let _guard = self
            .operation
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let path = self.document_path(feature, id);
        let backup = backup_path(&path);
        let mut removed = remove_if_present(&path)?;
        removed |= remove_if_present(&backup)?;
        if removed {
            sync_directory(path.parent().expect("document path has a parent"))?;
        }
        Ok(removed)
    }
}

#[derive(Debug, Deserialize)]
struct RawEnvelope {
    schema: String,
    version: u64,
    payload: serde_json::Value,
}

#[derive(Serialize)]
struct Envelope<'a, T> {
    schema: &'a str,
    version: u64,
    payload: &'a T,
}

#[derive(Deserialize)]
struct SessionV1 {
    active_workspace: Option<String>,
    workspace_order: Vec<String>,
    recent_commands: Vec<String>,
}

fn migrate_session_v1(payload: serde_json::Value) -> Result<SessionState, PersistenceError> {
    let legacy: SessionV1 = serde_json::from_value(payload)
        .map_err(|error| PersistenceError::Corrupt(error.to_string()))?;
    SessionState::new(
        legacy.active_workspace,
        legacy.workspace_order,
        legacy.recent_commands,
        Default::default(),
    )
    .map_err(Into::into)
}

fn encode_envelope<T: Serialize>(
    schema: &str,
    version: u64,
    payload: &T,
) -> Result<Vec<u8>, PersistenceError> {
    let bytes = serde_json::to_vec_pretty(&Envelope {
        schema,
        version,
        payload,
    })
    .map_err(|error| PersistenceError::Corrupt(error.to_string()))?;
    if bytes.len() > MAX_ENVELOPE_BYTES {
        return Err(PersistenceError::PayloadTooLarge);
    }
    Ok(bytes)
}

fn decode_json<T: DeserializeOwned>(raw: &[u8]) -> Result<T, PersistenceError> {
    serde_json::from_slice(raw).map_err(|error| PersistenceError::Corrupt(error.to_string()))
}

fn read_bounded(path: &Path) -> Result<Option<Vec<u8>>, PersistenceError> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take((MAX_ENVELOPE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_ENVELOPE_BYTES {
        return Err(PersistenceError::PayloadTooLarge);
    }
    Ok(Some(bytes))
}

fn load_with_fallback<T>(
    path: &Path,
    read: impl Fn(&Path) -> Result<Option<T>, PersistenceError>,
) -> Result<Option<T>, PersistenceError> {
    match read(path) {
        Ok(Some(value)) => Ok(Some(value)),
        Ok(None) => read(&backup_path(path)),
        Err(
            primary @ (PersistenceError::Corrupt(_)
            | PersistenceError::Validation(_)
            | PersistenceError::PayloadTooLarge),
        ) => match read(&backup_path(path)) {
            Ok(Some(value)) => Ok(Some(value)),
            Ok(None) | Err(_) => Err(primary),
        },
        Err(error) => Err(error),
    }
}

fn write_with_backup<T>(
    path: &Path,
    bytes: &[u8],
    validate: impl Fn(&Path) -> Result<Option<T>, PersistenceError>,
) -> Result<(), PersistenceError> {
    let parent = path.parent().ok_or_else(|| {
        PersistenceError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path has no parent",
        ))
    })?;
    create_private_directory(parent)?;

    match validate(path) {
        Ok(Some(_)) => {
            if let Some(current) = read_bounded(path)? {
                atomic_replace(&backup_path(path), &current)?;
            }
        }
        Ok(None)
        | Err(PersistenceError::Corrupt(_))
        | Err(PersistenceError::Validation(_))
        | Err(PersistenceError::PayloadTooLarge) => {}
        Err(error) => return Err(error),
    }
    atomic_replace(path, bytes)
}

fn atomic_replace(path: &Path, bytes: &[u8]) -> Result<(), PersistenceError> {
    let parent = path
        .parent()
        .expect("validated persistence path has a parent");
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            PersistenceError::Io(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid file name",
            ))
        })?;

    for _ in 0..ATOMIC_CREATE_ATTEMPTS {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(
            ".{file_name}.{}.{}.tmp",
            std::process::id(),
            sequence
        ));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        configure_private_file(&mut options);
        let mut file = match options.open(&temporary) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        };

        let result = (|| -> Result<(), PersistenceError> {
            file.write_all(bytes)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temporary, path)?;
            sync_directory(parent)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        return result;
    }
    Err(PersistenceError::Io(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not reserve an atomic persistence file",
    )))
}

fn backup_path(path: &Path) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(".bak");
    PathBuf::from(value)
}

fn remove_if_present(path: &Path) -> Result<bool, PersistenceError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn create_private_directory(path: &Path) -> Result<(), PersistenceError> {
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

#[cfg(unix)]
fn configure_private_file(options: &mut OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt;
    options.mode(0o600);
}

#[cfg(not(unix))]
fn configure_private_file(_options: &mut OpenOptions) {}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), PersistenceError> {
    File::open(path)?.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> Result<(), PersistenceError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(test: &str) -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            Self(std::env::temp_dir().join(format!(
                "market-terminal-{test}-{}-{nonce}",
                std::process::id()
            )))
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn state(active: &str) -> SessionState {
        SessionState::new(
            Some(active.to_owned()),
            vec![active.to_owned(), "news".to_owned()],
            vec!["AAPL US EQUITY".to_owned()],
            BTreeMap::from([("density".to_owned(), "compact".to_owned())]),
        )
        .unwrap()
    }

    #[test]
    fn session_round_trip_is_durable_and_leaves_no_temporary_files() {
        let directory = TestDirectory::new("session");
        let repository = LocalPersistence::new(&directory.0);
        let expected = state("overview");

        SessionStateRepository::save(&repository, &expected).unwrap();

        assert_eq!(
            SessionStateRepository::load(&repository).unwrap(),
            Some(expected)
        );
        let names: Vec<_> = fs::read_dir(&directory.0)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert!(names
            .iter()
            .all(|name| !name.to_string_lossy().ends_with(".tmp")));
    }

    #[test]
    fn corrupt_current_generation_recovers_previous_valid_generation() {
        let directory = TestDirectory::new("fallback");
        let repository = LocalPersistence::new(&directory.0);
        let previous = state("overview");
        SessionStateRepository::save(&repository, &previous).unwrap();
        SessionStateRepository::save(&repository, &state("news")).unwrap();
        fs::write(repository.session_path(), b"not json").unwrap();

        assert_eq!(
            SessionStateRepository::load(&repository).unwrap(),
            Some(previous)
        );
    }

    #[test]
    fn migrates_version_one_session_envelopes() {
        let directory = TestDirectory::new("migration");
        let repository = LocalPersistence::new(&directory.0);
        fs::create_dir_all(&directory.0).unwrap();
        fs::write(
            repository.session_path(),
            br#"{
                "schema":"market-terminal.session",
                "version":1,
                "payload":{
                    "active_workspace":"overview",
                    "workspace_order":["overview","news"],
                    "recent_commands":["NEWS"]
                }
            }"#,
        )
        .unwrap();

        let migrated = SessionStateRepository::load(&repository).unwrap().unwrap();
        assert_eq!(migrated.active_workspace(), Some("overview"));
        assert!(migrated.preferences().is_empty());
    }

    #[test]
    fn feature_documents_round_trip_list_and_delete_by_safe_identity() {
        let directory = TestDirectory::new("documents");
        let repository = LocalPersistence::new(&directory.0);
        let feature = FeatureKey::new("spreadsheet").unwrap();
        let first_id = DocumentId::new("main").unwrap();
        let second_id = DocumentId::new("scenario_b").unwrap();
        let first = FeatureDocument::new(
            feature.clone(),
            first_id.clone(),
            7,
            serde_json::json!({"sheets":["Inputs", "Model"]}),
        )
        .unwrap();
        let second = FeatureDocument::new(
            feature.clone(),
            second_id.clone(),
            1,
            serde_json::json!({"sheets":[]}),
        )
        .unwrap();

        FeatureDocumentRepository::save(&repository, &second).unwrap();
        FeatureDocumentRepository::save(&repository, &first).unwrap();

        assert_eq!(
            FeatureDocumentRepository::list(&repository, &feature).unwrap(),
            vec![first_id.clone(), second_id]
        );
        assert_eq!(
            FeatureDocumentRepository::load(&repository, &feature, &first_id).unwrap(),
            Some(first)
        );
        assert!(FeatureDocumentRepository::delete(&repository, &feature, &first_id).unwrap());
        assert!(!FeatureDocumentRepository::delete(&repository, &feature, &first_id).unwrap());
    }

    #[test]
    fn portfolio_import_path_round_trips_through_private_feature_document() {
        let directory = TestDirectory::new("portfolio-import");
        let repository = LocalPersistence::new(&directory.0);
        let path = PathBuf::from("/Users/example/Documents/positions.csv");

        PortfolioImportStateStore::save_import_path(&repository, &path).unwrap();

        assert_eq!(
            PortfolioImportStateStore::load_import_path(&repository).unwrap(),
            Some(path)
        );
        let document = directory
            .0
            .join("documents")
            .join("portfolio")
            .join("active_import.json");
        assert!(document.is_file());
        #[cfg(unix)]
        assert_eq!(
            fs::metadata(document).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn portfolio_activity_import_path_is_private_and_separate_from_positions() {
        let directory = TestDirectory::new("portfolio-activity-import");
        let repository = LocalPersistence::new(&directory.0);
        let positions = PathBuf::from("/Users/example/Documents/positions.csv");
        let activity = PathBuf::from("/Users/example/Documents/activity.csv");

        PortfolioImportStateStore::save_import_path(&repository, &positions).unwrap();
        PortfolioImportStateStore::save_activity_import_path(&repository, &activity).unwrap();

        assert_eq!(
            PortfolioImportStateStore::load_import_path(&repository).unwrap(),
            Some(positions)
        );
        assert_eq!(
            PortfolioImportStateStore::load_activity_import_path(&repository).unwrap(),
            Some(activity)
        );
        let document = directory
            .0
            .join("documents")
            .join("portfolio")
            .join("active_activity_import.json");
        assert!(document.is_file());
        #[cfg(unix)]
        assert_eq!(
            fs::metadata(document).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn portfolio_performance_import_path_is_private_and_separate() {
        let directory = TestDirectory::new("portfolio-performance-import");
        let repository = LocalPersistence::new(&directory.0);
        let performance = PathBuf::from("/Users/example/Documents/performance.csv");

        PortfolioImportStateStore::save_performance_import_path(&repository, &performance).unwrap();

        assert_eq!(
            PortfolioImportStateStore::load_performance_import_path(&repository).unwrap(),
            Some(performance)
        );
        let document = directory
            .0
            .join("documents")
            .join("portfolio")
            .join("active_performance_import.json");
        assert!(document.is_file());
        #[cfg(unix)]
        assert_eq!(
            fs::metadata(document).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn portfolio_tax_lot_import_path_is_private_and_separate() {
        let directory = TestDirectory::new("portfolio-tax-lot-import");
        let repository = LocalPersistence::new(&directory.0);
        let lots = PathBuf::from("/Users/example/Documents/tax-lots.csv");

        PortfolioImportStateStore::save_tax_lot_import_path(&repository, &lots).unwrap();

        assert_eq!(
            PortfolioImportStateStore::load_tax_lot_import_path(&repository).unwrap(),
            Some(lots)
        );
        let document = directory
            .0
            .join("documents")
            .join("portfolio")
            .join("active_tax_lot_import.json");
        assert!(document.is_file());
        #[cfg(unix)]
        assert_eq!(
            fs::metadata(document).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn portfolio_realized_gain_import_path_is_private_and_separate() {
        let directory = TestDirectory::new("portfolio-realized-gain-import");
        let repository = LocalPersistence::new(&directory.0);
        let closed_lots = PathBuf::from("/Users/example/Documents/closed-lots.csv");

        PortfolioImportStateStore::save_realized_gain_import_path(&repository, &closed_lots)
            .unwrap();

        assert_eq!(
            PortfolioImportStateStore::load_realized_gain_import_path(&repository).unwrap(),
            Some(closed_lots)
        );
        let document = directory
            .0
            .join("documents")
            .join("portfolio")
            .join("active_realized_gain_import.json");
        assert!(document.is_file());
        #[cfg(unix)]
        assert_eq!(
            fs::metadata(document).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn portfolio_trade_import_path_is_private_and_separate() {
        let directory = TestDirectory::new("portfolio-trade-import");
        let repository = LocalPersistence::new(&directory.0);
        let trades = PathBuf::from("/Users/example/Documents/trades.csv");

        PortfolioImportStateStore::save_trade_import_path(&repository, &trades).unwrap();

        assert_eq!(
            PortfolioImportStateStore::load_trade_import_path(&repository).unwrap(),
            Some(trades)
        );
        let document = directory
            .0
            .join("documents")
            .join("portfolio")
            .join("active_trade_import.json");
        assert!(document.is_file());
        #[cfg(unix)]
        assert_eq!(
            fs::metadata(document).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn portfolio_contribution_import_path_is_private_and_separate() {
        let directory = TestDirectory::new("portfolio-contribution-import");
        let repository = LocalPersistence::new(&directory.0);
        let contribution = PathBuf::from("/Users/example/Documents/contribution.csv");

        PortfolioImportStateStore::save_contribution_import_path(&repository, &contribution)
            .unwrap();

        assert_eq!(
            PortfolioImportStateStore::load_contribution_import_path(&repository).unwrap(),
            Some(contribution)
        );
        let document = directory
            .0
            .join("documents")
            .join("portfolio")
            .join("active_contribution_import.json");
        assert!(document.is_file());
        #[cfg(unix)]
        assert_eq!(
            fs::metadata(document).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn portfolio_attribution_import_path_is_private_and_separate() {
        let directory = TestDirectory::new("portfolio-attribution-import");
        let repository = LocalPersistence::new(&directory.0);
        let attribution = PathBuf::from("/Users/example/Documents/attribution.csv");

        PortfolioImportStateStore::save_attribution_import_path(&repository, &attribution).unwrap();

        assert_eq!(
            PortfolioImportStateStore::load_attribution_import_path(&repository).unwrap(),
            Some(attribution)
        );
        let document = directory
            .0
            .join("documents")
            .join("portfolio")
            .join("active_attribution_import.json");
        assert!(document.is_file());
        #[cfg(unix)]
        assert_eq!(
            fs::metadata(document).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn alert_rule_runtime_state_round_trips_through_private_feature_document() {
        use crate::features::alerts::{
            AlertCondition, AlertEvaluation, AlertObservation, AlertRule, AlertRuleId,
            DebouncePolicy, InstrumentRef,
        };

        let directory = TestDirectory::new("alert-state");
        let repository = LocalPersistence::new(&directory.0);
        let mut rule = AlertRule::new(
            AlertRuleId::new("local:ibm:1"),
            InstrumentRef::new("us:listed:ibm", "IBM"),
            AlertCondition::price_above(100.0),
            DebouncePolicy::consecutive(1),
        );
        let observation = AlertObservation::new(
            "alpha-vantage:ibm:2026-08-27",
            "us:listed:ibm",
            250.0,
            1.0,
            "2026-08-27T20:00:00Z",
        );
        assert!(matches!(
            rule.evaluate(&observation),
            AlertEvaluation::Triggered(_)
        ));
        rule.acknowledge("2026-08-27T20:01:00Z");
        let state = AlertRulesState::new(3, vec![rule]).unwrap();

        AlertStateStore::save_alert_rules(&repository, &state).unwrap();
        let restored = AlertStateStore::load_alert_rules(&repository)
            .unwrap()
            .unwrap();

        assert_eq!(restored, state);
        assert_eq!(
            restored.rules[0].clone().evaluate(&observation),
            AlertEvaluation::Duplicate
        );
        let document = directory
            .0
            .join("documents")
            .join("alerts")
            .join("rule_register.json");
        assert!(document.is_file());
        #[cfg(unix)]
        assert_eq!(
            fs::metadata(document).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn unsupported_future_versions_are_not_silently_downgraded() {
        let directory = TestDirectory::new("future-version");
        let repository = LocalPersistence::new(&directory.0);
        fs::create_dir_all(&directory.0).unwrap();
        fs::write(
            repository.session_path(),
            br#"{"schema":"market-terminal.session","version":999,"payload":{}}"#,
        )
        .unwrap();

        assert!(matches!(
            SessionStateRepository::load(&repository),
            Err(PersistenceError::UnsupportedVersion { version: 999, .. })
        ));
    }
}
