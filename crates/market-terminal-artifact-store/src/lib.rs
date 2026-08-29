//! Read-only local implementation of the application-owned research artifact
//! query port.
//!
//! The adapter deliberately owns filesystem concerns. The application and
//! engine crates remain host-neutral, while HTTP and future worker hosts can
//! compose this implementation at their outermost boundary.

use std::{
    fmt, fs, io,
    path::{Path, PathBuf},
};

use market_terminal_application::{
    ArtifactQueryFailure, ResearchArtifactDocument, ResearchArtifactPage, ResearchArtifactQuery,
    TenantArtifactKey, TenantArtifactListKey, ARTIFACT_QUERY_SCHEMA_VERSION,
    MAX_ARTIFACT_DOCUMENT_BYTES,
};

pub const LOCAL_ARTIFACT_STORE_SCHEMA_VERSION: u16 = 1;
pub const MAX_ARTIFACTS_PER_TENANT: usize = 4_096;

const TENANT_PREFIX: &str = "tenant-";
const ARTIFACT_PREFIX: &str = "artifact-";
const ARTIFACT_SUFFIX: &str = ".json";

#[derive(Debug)]
pub enum LocalArtifactStoreConfigError {
    Unavailable(io::Error),
    RootIsSymlink,
    RootIsNotDirectory,
    InsecureRootPermissions,
}

impl fmt::Display for LocalArtifactStoreConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(error) => write!(formatter, "artifact root is unavailable: {error}"),
            Self::RootIsSymlink => formatter.write_str("artifact root must not be a symbolic link"),
            Self::RootIsNotDirectory => formatter.write_str("artifact root must be a directory"),
            Self::InsecureRootPermissions => formatter.write_str(
                "artifact root must not grant group or other permissions (expected mode 0700)",
            ),
        }
    }
}

impl std::error::Error for LocalArtifactStoreConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unavailable(error) => Some(error),
            Self::RootIsSymlink | Self::RootIsNotDirectory | Self::InsecureRootPermissions => None,
        }
    }
}

/// Filesystem-backed, read-only artifact query adapter.
///
/// Tenant and artifact identities are hex-encoded before becoming path
/// components. The adapter never interpolates raw client or tenant text into a
/// filesystem path and rejects symlinked tenant directories or documents.
#[derive(Debug, Clone)]
pub struct LocalArtifactQuery {
    canonical_root: PathBuf,
}

impl LocalArtifactQuery {
    pub fn open(root: impl AsRef<Path>) -> Result<Self, LocalArtifactStoreConfigError> {
        let root = root.as_ref();
        let metadata =
            fs::symlink_metadata(root).map_err(LocalArtifactStoreConfigError::Unavailable)?;
        if metadata.file_type().is_symlink() {
            return Err(LocalArtifactStoreConfigError::RootIsSymlink);
        }
        if !metadata.is_dir() {
            return Err(LocalArtifactStoreConfigError::RootIsNotDirectory);
        }
        ensure_private_root(&metadata)?;
        let canonical_root =
            fs::canonicalize(root).map_err(LocalArtifactStoreConfigError::Unavailable)?;
        Ok(Self { canonical_root })
    }

    pub fn root(&self) -> &Path {
        &self.canonical_root
    }

    fn tenant_directory(&self, tenant: &str) -> PathBuf {
        self.canonical_root
            .join(format!("{TENANT_PREFIX}{}", hex_encode(tenant.as_bytes())))
    }

    fn artifact_path(&self, tenant: &str, artifact_id: &str) -> PathBuf {
        self.tenant_directory(tenant).join(format!(
            "{ARTIFACT_PREFIX}{}{ARTIFACT_SUFFIX}",
            hex_encode(artifact_id.as_bytes())
        ))
    }

    fn verified_tenant_directory(
        &self,
        tenant: &str,
    ) -> Result<Option<PathBuf>, ArtifactQueryFailure> {
        let directory = self.tenant_directory(tenant);
        let metadata = match fs::symlink_metadata(&directory) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(ArtifactQueryFailure::Unavailable),
        };
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(corrupt("tenant entry is not a regular directory"));
        }
        let canonical =
            fs::canonicalize(&directory).map_err(|_| ArtifactQueryFailure::Unavailable)?;
        if canonical.parent() != Some(self.canonical_root.as_path()) {
            return Err(corrupt("tenant directory escapes the configured root"));
        }
        Ok(Some(canonical))
    }

    fn read_document(
        &self,
        tenant: &str,
        path: &Path,
    ) -> Result<ResearchArtifactDocument, ArtifactQueryFailure> {
        let metadata = fs::symlink_metadata(path).map_err(map_read_error)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(corrupt("artifact entry is not a regular file"));
        }
        if metadata.len() > MAX_ARTIFACT_DOCUMENT_BYTES as u64 {
            return Err(corrupt("artifact document exceeds the size limit"));
        }
        let canonical = fs::canonicalize(path).map_err(|_| ArtifactQueryFailure::Unavailable)?;
        let tenant_directory = self.tenant_directory(tenant);
        if canonical.parent() != Some(tenant_directory.as_path()) {
            return Err(corrupt("artifact document escapes its tenant directory"));
        }
        let encoded = fs::read(&canonical).map_err(map_read_error)?;
        if encoded.len() > MAX_ARTIFACT_DOCUMENT_BYTES {
            return Err(corrupt("artifact document exceeds the size limit"));
        }
        let document: ResearchArtifactDocument = serde_json::from_slice(&encoded)
            .map_err(|_| corrupt("artifact document is not valid schema JSON"))?;
        if document.summary.tenant_id.as_str() != tenant {
            return Err(corrupt(
                "artifact ownership does not match its tenant directory",
            ));
        }
        let expected = self.artifact_path(tenant, &document.summary.artifact_id);
        if expected != path {
            return Err(corrupt(
                "artifact identity does not match its canonical filename",
            ));
        }
        Ok(document)
    }
}

impl ResearchArtifactQuery for LocalArtifactQuery {
    fn list(
        &self,
        key: &TenantArtifactListKey,
    ) -> Result<ResearchArtifactPage, ArtifactQueryFailure> {
        let tenant = key.tenant_id().as_str();
        let Some(directory) = self.verified_tenant_directory(tenant)? else {
            return Ok(empty_page());
        };
        let entries = fs::read_dir(directory).map_err(|_| ArtifactQueryFailure::Unavailable)?;
        let mut documents = Vec::new();
        let mut artifact_files = 0usize;
        for entry in entries {
            let path = entry.map_err(|_| ArtifactQueryFailure::Unavailable)?.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            artifact_files = artifact_files.saturating_add(1);
            if artifact_files > MAX_ARTIFACTS_PER_TENANT {
                return Err(corrupt(
                    "tenant artifact catalog exceeds its bounded capacity",
                ));
            }
            let document = self.read_document(tenant, &path)?;
            if key.kind().is_none_or(|kind| document.summary.kind == kind)
                && key
                    .cursor()
                    .is_none_or(|cursor| document.summary.artifact_id.as_str() > cursor)
            {
                documents.push(document);
            }
        }
        documents.sort_unstable_by(|left, right| {
            left.summary.artifact_id.cmp(&right.summary.artifact_id)
        });
        let has_more = documents.len() > key.limit();
        documents.truncate(key.limit());
        let items = documents
            .into_iter()
            .map(|document| document.summary)
            .collect::<Vec<_>>();
        let next_cursor = has_more
            .then(|| items.last().map(|item| item.artifact_id.clone()))
            .flatten();
        Ok(ResearchArtifactPage {
            schema_version: ARTIFACT_QUERY_SCHEMA_VERSION,
            items,
            next_cursor,
        })
    }

    fn get(
        &self,
        key: &TenantArtifactKey,
    ) -> Result<Option<ResearchArtifactDocument>, ArtifactQueryFailure> {
        let tenant = key.tenant_id().as_str();
        if self.verified_tenant_directory(tenant)?.is_none() {
            return Ok(None);
        }
        let path = self.artifact_path(tenant, key.artifact_id());
        match fs::symlink_metadata(&path) {
            Ok(_) => self.read_document(tenant, &path).map(Some),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err(ArtifactQueryFailure::Unavailable),
        }
    }
}

fn empty_page() -> ResearchArtifactPage {
    ResearchArtifactPage {
        schema_version: ARTIFACT_QUERY_SCHEMA_VERSION,
        items: Vec::new(),
        next_cursor: None,
    }
}

fn corrupt(message: impl Into<String>) -> ArtifactQueryFailure {
    ArtifactQueryFailure::Corrupt(message.into())
}

fn map_read_error(error: io::Error) -> ArtifactQueryFailure {
    if error.kind() == io::ErrorKind::NotFound {
        corrupt("artifact catalog changed during a read")
    } else {
        ArtifactQueryFailure::Unavailable
    }
}

fn hex_encode(input: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(input.len() * 2);
    for byte in input {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

#[cfg(unix)]
fn ensure_private_root(metadata: &fs::Metadata) -> Result<(), LocalArtifactStoreConfigError> {
    use std::os::unix::fs::PermissionsExt;

    if metadata.permissions().mode() & 0o077 != 0 {
        Err(LocalArtifactStoreConfigError::InsecureRootPermissions)
    } else {
        Ok(())
    }
}

#[cfg(not(unix))]
fn ensure_private_root(_: &fs::Metadata) -> Result<(), LocalArtifactStoreConfigError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

    use market_terminal_application::{
        ApplicationErrorCode, ArtifactCapabilitySet, ArtifactListRequest, CapabilitySet,
        ExecutionBudget, ExecutionContext, PrincipalId, ResearchArtifactApplicationService,
        ResearchArtifactDocument, ResearchArtifactKind, ResearchArtifactSummary, TenantId,
        ARTIFACT_QUERY_SCHEMA_VERSION, MAX_ARTIFACT_DOCUMENT_BYTES,
    };

    use super::*;

    static TEMP_ID: AtomicU64 = AtomicU64::new(0);

    struct TestRoot(PathBuf);

    impl TestRoot {
        fn new() -> Self {
            let sequence = TEMP_ID.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "market-terminal-artifacts-{}-{sequence}",
                std::process::id()
            ));
            fs::create_dir(&path).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
            }
            Self(path)
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn context(tenant: &str) -> ExecutionContext {
        ExecutionContext::new(
            TenantId::new(tenant).unwrap(),
            PrincipalId::new("researcher").unwrap(),
            CapabilitySet::all(),
            ExecutionBudget::default(),
        )
        .with_artifact_capabilities(ArtifactCapabilitySet::read_only())
    }

    fn document(tenant: &str, id: &str, kind: ResearchArtifactKind) -> ResearchArtifactDocument {
        ResearchArtifactDocument {
            summary: ResearchArtifactSummary {
                schema_version: ARTIFACT_QUERY_SCHEMA_VERSION,
                tenant_id: TenantId::new(tenant).unwrap(),
                artifact_id: id.to_owned(),
                kind,
                title: format!("Research {id}"),
                created_at_epoch_ms: 1_800_000_000_000,
                input_version: "fixture-v1".to_owned(),
                source: "verified-fixture".to_owned(),
                quality: "complete".to_owned(),
                content_digest: format!("sha256:{id}:12345678"),
            },
            content: serde_json::json!({"artifact_id": id}),
        }
    }

    fn write_document(root: &Path, document: &ResearchArtifactDocument) -> PathBuf {
        let query = LocalArtifactQuery {
            canonical_root: root.to_owned(),
        };
        let directory = query.tenant_directory(document.summary.tenant_id.as_str());
        fs::create_dir_all(&directory).unwrap();
        let path = query.artifact_path(
            document.summary.tenant_id.as_str(),
            &document.summary.artifact_id,
        );
        fs::write(&path, serde_json::to_vec(document).unwrap()).unwrap();
        path
    }

    #[test]
    fn tenant_owned_list_get_filter_and_cursor_are_deterministic() {
        let root = TestRoot::new();
        for id in ["run-c", "run-a", "run-b"] {
            write_document(
                &root.0,
                &document("tenant-a", id, ResearchArtifactKind::BacktestRun),
            );
        }
        write_document(
            &root.0,
            &document("tenant-a", "news-a", ResearchArtifactKind::NewsSnapshot),
        );
        write_document(
            &root.0,
            &document("tenant-b", "run-b", ResearchArtifactKind::BacktestRun),
        );
        write_document(
            &root.0,
            &document(
                "tenant-b",
                "only-tenant-b",
                ResearchArtifactKind::SecurityResearch,
            ),
        );

        let service = ResearchArtifactApplicationService::new(std::sync::Arc::new(
            LocalArtifactQuery::open(&root.0).unwrap(),
        ));
        let first = service
            .list(
                &context("tenant-a"),
                ArtifactListRequest {
                    kind: Some(ResearchArtifactKind::BacktestRun),
                    cursor: None,
                    limit: Some(2),
                },
            )
            .unwrap();
        assert_eq!(
            first
                .items
                .iter()
                .map(|item| item.artifact_id.as_str())
                .collect::<Vec<_>>(),
            ["run-a", "run-b"]
        );
        assert_eq!(first.next_cursor.as_deref(), Some("run-b"));

        let second = service
            .list(
                &context("tenant-a"),
                ArtifactListRequest {
                    kind: Some(ResearchArtifactKind::BacktestRun),
                    cursor: first.next_cursor,
                    limit: Some(2),
                },
            )
            .unwrap();
        assert_eq!(second.items[0].artifact_id, "run-c");
        assert_eq!(second.next_cursor, None);
        assert_eq!(
            service
                .get(&context("tenant-a"), "run-b")
                .unwrap()
                .unwrap()
                .summary
                .tenant_id
                .as_str(),
            "tenant-a"
        );
        assert!(service
            .get(&context("tenant-a"), "only-tenant-b")
            .unwrap()
            .is_none());
    }

    #[test]
    fn malformed_and_oversized_documents_fail_closed_without_contents() {
        let root = TestRoot::new();
        let malformed = document(
            "tenant-a",
            "malformed",
            ResearchArtifactKind::SecurityResearch,
        );
        let path = write_document(&root.0, &malformed);
        fs::write(&path, b"{not-json").unwrap();

        let service = ResearchArtifactApplicationService::new(std::sync::Arc::new(
            LocalArtifactQuery::open(&root.0).unwrap(),
        ));
        let error = service.get(&context("tenant-a"), "malformed").unwrap_err();
        assert_eq!(error.code, ApplicationErrorCode::ArtifactContractViolation);
        assert!(!error.message.contains("not-json"));

        fs::write(&path, vec![b'x'; MAX_ARTIFACT_DOCUMENT_BYTES + 1]).unwrap();
        let error = service.get(&context("tenant-a"), "malformed").unwrap_err();
        assert_eq!(error.code, ApplicationErrorCode::ArtifactContractViolation);
    }

    #[test]
    fn misnamed_and_cross_tenant_documents_fail_closed() {
        let root = TestRoot::new();
        let original = document(
            "tenant-a",
            "canonical-id",
            ResearchArtifactKind::SecurityResearch,
        );
        let original_path = write_document(&root.0, &original);
        let query = LocalArtifactQuery::open(&root.0).unwrap();
        let alias_path = query.artifact_path("tenant-a", "alias-id");
        fs::rename(original_path, &alias_path).unwrap();

        let service = ResearchArtifactApplicationService::new(std::sync::Arc::new(query.clone()));
        let error = service.get(&context("tenant-a"), "alias-id").unwrap_err();
        assert_eq!(error.code, ApplicationErrorCode::ArtifactContractViolation);

        let foreign = document(
            "tenant-b",
            "alias-id",
            ResearchArtifactKind::SecurityResearch,
        );
        fs::write(&alias_path, serde_json::to_vec(&foreign).unwrap()).unwrap();
        let error = service.get(&context("tenant-a"), "alias-id").unwrap_err();
        assert_eq!(error.code, ApplicationErrorCode::ArtifactContractViolation);
        assert!(!error.message.contains("tenant-b"));
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_and_shared_root_permissions_are_rejected() {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let root = TestRoot::new();
        fs::set_permissions(&root.0, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(matches!(
            LocalArtifactQuery::open(&root.0),
            Err(LocalArtifactStoreConfigError::InsecureRootPermissions)
        ));
        fs::set_permissions(&root.0, fs::Permissions::from_mode(0o700)).unwrap();

        let source = document("tenant-a", "source", ResearchArtifactKind::ScreenResult);
        let source_path = write_document(&root.0, &source);
        let query = LocalArtifactQuery::open(&root.0).unwrap();
        let linked_path = query.artifact_path("tenant-a", "linked");
        symlink(source_path, &linked_path).unwrap();
        let linked = document("tenant-a", "linked", ResearchArtifactKind::ScreenResult);
        assert_ne!(linked.summary.artifact_id, source.summary.artifact_id);

        let service = ResearchArtifactApplicationService::new(std::sync::Arc::new(query));
        let error = service.get(&context("tenant-a"), "linked").unwrap_err();
        assert_eq!(error.code, ApplicationErrorCode::ArtifactContractViolation);
    }
}
