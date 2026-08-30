//! Private, hashed, read-only credential catalog adapter.
//!
//! Bearer secrets are never persisted by this crate. The catalog stores only
//! SHA-256 digests and resolves them to the host-neutral authentication contract.

use std::{collections::BTreeSet, fmt, fs, io, path::Path};

use market_terminal_application::{
    ArtifactCapabilitySet, CapabilitySet, ExecutionBudget, ExecutionContext, PrincipalId, TenantId,
};
use market_terminal_auth::{
    CredentialId, CredentialResolveFailure, CredentialResolver, ResolvedCredential,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};

pub const CREDENTIAL_CATALOG_SCHEMA_VERSION: u16 = 1;
pub const MAX_CREDENTIAL_CATALOG_BYTES: usize = 1024 * 1024;
pub const MAX_CREDENTIALS: usize = 256;

#[derive(Debug)]
pub enum CredentialStoreConfigError {
    Unavailable(io::Error),
    CatalogIsSymlink,
    CatalogIsNotFile,
    InsecureCatalogPermissions,
    CatalogTooLarge,
    InvalidCatalog,
}

impl fmt::Display for CredentialStoreConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(error) => {
                write!(formatter, "credential catalog is unavailable: {error}")
            }
            Self::CatalogIsSymlink => {
                formatter.write_str("credential catalog must not be a symbolic link")
            }
            Self::CatalogIsNotFile => {
                formatter.write_str("credential catalog must be a regular file")
            }
            Self::InsecureCatalogPermissions => formatter.write_str(
                "credential catalog must not grant group or other permissions (expected mode 0600)",
            ),
            Self::CatalogTooLarge => formatter.write_str("credential catalog exceeds 1 MiB"),
            Self::InvalidCatalog => formatter.write_str("credential catalog is invalid"),
        }
    }
}

impl std::error::Error for CredentialStoreConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unavailable(error) => Some(error),
            _ => None,
        }
    }
}

#[derive(Clone)]
struct CredentialEntry {
    token_digest: [u8; 32],
    active: bool,
    not_before_epoch_seconds: Option<u64>,
    expires_at_epoch_seconds: Option<u64>,
    resolved: ResolvedCredential,
}

/// Immutable startup snapshot of a private credential catalog.
///
/// Replacing the catalog on disk does not mutate an active process. Operators
/// restart or roll the host to apply issuance, revocation, or policy changes.
#[derive(Clone)]
pub struct LocalCredentialResolver {
    entries: Vec<CredentialEntry>,
}

impl LocalCredentialResolver {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, CredentialStoreConfigError> {
        let path = path.as_ref();
        let metadata =
            fs::symlink_metadata(path).map_err(CredentialStoreConfigError::Unavailable)?;
        if metadata.file_type().is_symlink() {
            return Err(CredentialStoreConfigError::CatalogIsSymlink);
        }
        if !metadata.is_file() {
            return Err(CredentialStoreConfigError::CatalogIsNotFile);
        }
        ensure_private_catalog(&metadata)?;
        if metadata.len() > MAX_CREDENTIAL_CATALOG_BYTES as u64 {
            return Err(CredentialStoreConfigError::CatalogTooLarge);
        }
        let encoded = fs::read(path).map_err(CredentialStoreConfigError::Unavailable)?;
        if encoded.len() > MAX_CREDENTIAL_CATALOG_BYTES {
            return Err(CredentialStoreConfigError::CatalogTooLarge);
        }
        let catalog: CatalogDocument = serde_json::from_slice(&encoded)
            .map_err(|_| CredentialStoreConfigError::InvalidCatalog)?;
        Self::from_document(catalog)
    }

    fn from_document(catalog: CatalogDocument) -> Result<Self, CredentialStoreConfigError> {
        if catalog.schema_version != CREDENTIAL_CATALOG_SCHEMA_VERSION
            || catalog.credentials.is_empty()
            || catalog.credentials.len() > MAX_CREDENTIALS
        {
            return Err(CredentialStoreConfigError::InvalidCatalog);
        }
        let mut ids = BTreeSet::new();
        let mut digests = BTreeSet::new();
        let mut entries = Vec::with_capacity(catalog.credentials.len());
        for record in catalog.credentials {
            let credential_id = CredentialId::new(record.credential_id)
                .map_err(|_| CredentialStoreConfigError::InvalidCatalog)?;
            let token_digest = decode_sha256(&record.token_sha256)
                .ok_or(CredentialStoreConfigError::InvalidCatalog)?;
            if !ids.insert(credential_id.clone()) || !digests.insert(token_digest) {
                return Err(CredentialStoreConfigError::InvalidCatalog);
            }
            if record
                .not_before_epoch_seconds
                .zip(record.expires_at_epoch_seconds)
                .is_some_and(|(not_before, expires)| not_before >= expires)
            {
                return Err(CredentialStoreConfigError::InvalidCatalog);
            }
            let capabilities =
                CapabilitySet::from_names(record.operations.iter().map(String::as_str))
                    .map_err(|_| CredentialStoreConfigError::InvalidCatalog)?;
            let budget =
                ExecutionBudget::new(record.max_backtest_bars, record.max_comparison_points)
                    .map_err(|_| CredentialStoreConfigError::InvalidCatalog)?;
            let mut context = ExecutionContext::new(
                TenantId::new(record.tenant_id)
                    .map_err(|_| CredentialStoreConfigError::InvalidCatalog)?,
                PrincipalId::new(record.principal_id)
                    .map_err(|_| CredentialStoreConfigError::InvalidCatalog)?,
                capabilities,
                budget,
            );
            if record.artifact_read {
                context = context.with_artifact_capabilities(ArtifactCapabilitySet::read_only());
            }
            entries.push(CredentialEntry {
                token_digest,
                active: record.status == CredentialStatus::Active,
                not_before_epoch_seconds: record.not_before_epoch_seconds,
                expires_at_epoch_seconds: record.expires_at_epoch_seconds,
                resolved: ResolvedCredential::new(credential_id, context),
            });
        }
        Ok(Self { entries })
    }

    pub fn credential_count(&self) -> usize {
        self.entries.len()
    }
}

impl CredentialResolver for LocalCredentialResolver {
    fn resolve(
        &self,
        bearer_token: &str,
        observed_at_epoch_seconds: u64,
    ) -> Result<Option<ResolvedCredential>, CredentialResolveFailure> {
        let candidate: [u8; 32] = Sha256::digest(bearer_token.as_bytes()).into();
        let mut matched = None;
        for entry in &self.entries {
            let equal = constant_time_digest_eq(&candidate, &entry.token_digest);
            if equal {
                matched = Some(entry);
            }
        }
        Ok(matched
            .filter(|entry| entry.active)
            .filter(|entry| {
                entry
                    .not_before_epoch_seconds
                    .is_none_or(|not_before| observed_at_epoch_seconds >= not_before)
            })
            .filter(|entry| {
                entry
                    .expires_at_epoch_seconds
                    .is_none_or(|expires| observed_at_epoch_seconds < expires)
            })
            .map(|entry| entry.resolved.clone()))
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogDocument {
    schema_version: u16,
    credentials: Vec<CredentialRecord>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CredentialRecord {
    credential_id: String,
    token_sha256: String,
    tenant_id: String,
    principal_id: String,
    status: CredentialStatus,
    not_before_epoch_seconds: Option<u64>,
    expires_at_epoch_seconds: Option<u64>,
    operations: Vec<String>,
    artifact_read: bool,
    max_backtest_bars: usize,
    max_comparison_points: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CredentialStatus {
    Active,
    Revoked,
}

fn decode_sha256(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return None;
    }
    let mut output = [0_u8; 32];
    for (index, pair) in value.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        output[index] = (hex_digit(pair[0])? << 4) | hex_digit(pair[1])?;
    }
    Some(output)
}

fn hex_digit(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}

fn constant_time_digest_eq(left: &[u8; 32], right: &[u8; 32]) -> bool {
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

#[cfg(unix)]
fn ensure_private_catalog(metadata: &fs::Metadata) -> Result<(), CredentialStoreConfigError> {
    use std::os::unix::fs::PermissionsExt;

    if metadata.permissions().mode() & 0o077 != 0 {
        Err(CredentialStoreConfigError::InsecureCatalogPermissions)
    } else {
        Ok(())
    }
}

#[cfg(not(unix))]
fn ensure_private_catalog(_: &fs::Metadata) -> Result<(), CredentialStoreConfigError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    use market_terminal_auth::CredentialResolver;
    use serde_json::{json, Value};

    use super::*;

    static TEMP_ID: AtomicU64 = AtomicU64::new(0);

    struct TestFile(PathBuf);

    impl TestFile {
        fn write(document: &Value) -> Self {
            let sequence = TEMP_ID.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "market-terminal-credentials-{}-{sequence}.json",
                std::process::id()
            ));
            fs::write(&path, serde_json::to_vec(document).unwrap()).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
            }
            Self(path)
        }
    }

    impl Drop for TestFile {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    fn sha256(value: &str) -> String {
        Sha256::digest(value.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    fn record(
        credential_id: &str,
        token: &str,
        tenant: &str,
        status: &str,
        not_before: Option<u64>,
        expires: Option<u64>,
    ) -> Value {
        json!({
            "credential_id": credential_id,
            "token_sha256": sha256(token),
            "tenant_id": tenant,
            "principal_id": format!("principal-{tenant}"),
            "status": status,
            "not_before_epoch_seconds": not_before,
            "expires_at_epoch_seconds": expires,
            "operations": ["price_option"],
            "artifact_read": tenant == "tenant-a",
            "max_backtest_bars": 100,
            "max_comparison_points": 500
        })
    }

    #[test]
    fn resolves_distinct_tenants_capabilities_and_budgets_from_hashes() {
        let token_a = "tenant-a-token-0123456789-ABCDEFGHIJ";
        let token_b = "tenant-b-token-0123456789-ABCDEFGHIJ";
        let file = TestFile::write(&json!({
            "schema_version": 1,
            "credentials": [
                record("credential-a", token_a, "tenant-a", "active", None, None),
                record("credential-b", token_b, "tenant-b", "active", Some(100), Some(200))
            ]
        }));
        let resolver = LocalCredentialResolver::open(&file.0).unwrap();
        assert_eq!(resolver.credential_count(), 2);

        let a = resolver.resolve(token_a, 150).unwrap().unwrap();
        assert_eq!(a.credential_id().as_str(), "credential-a");
        assert_eq!(a.execution_context().tenant_id().as_str(), "tenant-a");
        assert_eq!(
            a.execution_context().capabilities().allowed_names(),
            ["price_option"]
        );
        assert!(a.execution_context().artifact_capabilities().allows_read());
        assert_eq!(a.execution_context().budget().max_backtest_bars(), 100);

        let b = resolver.resolve(token_b, 150).unwrap().unwrap();
        assert_eq!(b.execution_context().tenant_id().as_str(), "tenant-b");
        assert!(!b.execution_context().artifact_capabilities().allows_read());
        assert!(resolver
            .resolve("unknown-token-0123456789-ABCDEFGHIJ", 150)
            .unwrap()
            .is_none());
    }

    #[test]
    fn revoked_not_yet_valid_and_expired_are_indistinguishable_misses() {
        let revoked = "revoked-token-0123456789-ABCDEFGHIJ";
        let future = "future-token-0123456789-ABCDEFGHIJK";
        let expired = "expired-token-0123456789-ABCDEFGHIJ";
        let file = TestFile::write(&json!({
            "schema_version": 1,
            "credentials": [
                record("revoked", revoked, "tenant-a", "revoked", None, None),
                record("future", future, "tenant-a", "active", Some(200), None),
                record("expired", expired, "tenant-a", "active", None, Some(100))
            ]
        }));
        let resolver = LocalCredentialResolver::open(&file.0).unwrap();
        for token in [
            revoked,
            future,
            expired,
            "unknown-token-0123456789-ABCDEFGHIJ",
        ] {
            assert!(resolver.resolve(token, 100).unwrap().is_none());
        }
    }

    #[test]
    fn duplicates_plaintext_and_invalid_windows_fail_configuration() {
        let token = "duplicate-token-0123456789-ABCDEFGHIJ";
        let mut first = record("first", token, "tenant-a", "active", None, None);
        first["plaintext_token"] = json!(token);
        let plaintext = TestFile::write(&json!({"schema_version": 1, "credentials": [first]}));
        assert!(matches!(
            LocalCredentialResolver::open(&plaintext.0),
            Err(CredentialStoreConfigError::InvalidCatalog)
        ));

        let duplicate = TestFile::write(&json!({
            "schema_version": 1,
            "credentials": [
                record("first", token, "tenant-a", "active", None, None),
                record("second", token, "tenant-b", "active", None, None)
            ]
        }));
        assert!(matches!(
            LocalCredentialResolver::open(&duplicate.0),
            Err(CredentialStoreConfigError::InvalidCatalog)
        ));

        let invalid_window = TestFile::write(&json!({
            "schema_version": 1,
            "credentials": [record("first", token, "tenant-a", "active", Some(200), Some(200))]
        }));
        assert!(matches!(
            LocalCredentialResolver::open(&invalid_window.0),
            Err(CredentialStoreConfigError::InvalidCatalog)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn shared_permissions_and_symlinked_catalogs_are_rejected() {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let token = "private-token-0123456789-ABCDEFGHIJK";
        let file = TestFile::write(&json!({
            "schema_version": 1,
            "credentials": [record("private", token, "tenant-a", "active", None, None)]
        }));
        fs::set_permissions(&file.0, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(matches!(
            LocalCredentialResolver::open(&file.0),
            Err(CredentialStoreConfigError::InsecureCatalogPermissions)
        ));
        fs::set_permissions(&file.0, fs::Permissions::from_mode(0o600)).unwrap();

        let link = file.0.with_extension("link");
        symlink(&file.0, &link).unwrap();
        assert!(matches!(
            LocalCredentialResolver::open(&link),
            Err(CredentialStoreConfigError::CatalogIsSymlink)
        ));
        fs::remove_file(link).unwrap();
    }
}
