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

use crate::features::persistence::{
    DocumentId, FeatureDocument, FeatureDocumentRepository, FeatureKey, PersistenceError,
    SessionState, SessionStateRepository, MAX_DOCUMENT_BYTES,
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
        Self { root: root.into(), operation: Mutex::new(()) }
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

impl SessionStateRepository for LocalPersistence {
    fn load(&self) -> Result<Option<SessionState>, PersistenceError> {
        let _guard = self.operation.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        load_with_fallback(&self.session_path(), Self::load_session_at)
    }

    fn save(&self, state: &SessionState) -> Result<(), PersistenceError> {
        state.validate()?;
        let _guard = self.operation.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
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
        let _guard = self.operation.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
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
        let _guard = self.operation.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let bytes = encode_envelope(DOCUMENT_SCHEMA, DOCUMENT_VERSION, document)?;
        write_with_backup(
            &self.document_path(document.feature(), document.id()),
            &bytes,
            Self::load_document_at,
        )
    }

    fn list(&self, feature: &FeatureKey) -> Result<Vec<DocumentId>, PersistenceError> {
        let _guard = self.operation.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
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
        let _guard = self.operation.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
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
    let bytes = serde_json::to_vec_pretty(&Envelope { schema, version, payload })
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
            primary
            @ (PersistenceError::Corrupt(_)
            | PersistenceError::Validation(_)
            | PersistenceError::PayloadTooLarge),
        ) => {
            match read(&backup_path(path)) {
                Ok(Some(value)) => Ok(Some(value)),
                Ok(None) | Err(_) => Err(primary),
            }
        }
        Err(error) => Err(error),
    }
}

fn write_with_backup<T>(
    path: &Path,
    bytes: &[u8],
    validate: impl Fn(&Path) -> Result<Option<T>, PersistenceError>,
) -> Result<(), PersistenceError> {
    let parent = path.parent().ok_or_else(|| {
        PersistenceError::Io(io::Error::new(io::ErrorKind::InvalidInput, "path has no parent"))
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
    let parent = path.parent().expect("validated persistence path has a parent");
    let file_name = path.file_name().and_then(|name| name.to_str()).ok_or_else(|| {
        PersistenceError::Io(io::Error::new(io::ErrorKind::InvalidInput, "invalid file name"))
    })?;

    for _ in 0..ATOMIC_CREATE_ATTEMPTS {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(".{file_name}.{}.{}.tmp", std::process::id(), sequence));
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
    use std::{collections::BTreeMap, time::{SystemTime, UNIX_EPOCH}};

    use super::*;

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

        assert_eq!(SessionStateRepository::load(&repository).unwrap(), Some(expected));
        let names: Vec<_> = fs::read_dir(&directory.0)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert!(names.iter().all(|name| !name.to_string_lossy().ends_with(".tmp")));
    }

    #[test]
    fn corrupt_current_generation_recovers_previous_valid_generation() {
        let directory = TestDirectory::new("fallback");
        let repository = LocalPersistence::new(&directory.0);
        let previous = state("overview");
        SessionStateRepository::save(&repository, &previous).unwrap();
        SessionStateRepository::save(&repository, &state("news")).unwrap();
        fs::write(repository.session_path(), b"not json").unwrap();

        assert_eq!(SessionStateRepository::load(&repository).unwrap(), Some(previous));
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
