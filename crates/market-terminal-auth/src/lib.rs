//! Host-neutral authentication contract for HTTP, worker, MCP, and future web
//! hosts.
//!
//! This crate owns no transport, filesystem, environment, clock, hashing, or
//! persistence behavior. A host supplies an observed timestamp and a concrete
//! resolver returns the server-owned application context for one credential.

use std::fmt;

use market_terminal_application::ExecutionContext;

pub const MIN_BEARER_TOKEN_BYTES: usize = 32;
pub const MAX_BEARER_TOKEN_BYTES: usize = 1_024;
pub const MAX_CREDENTIAL_ID_BYTES: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CredentialId(String);

impl CredentialId {
    pub fn new(value: impl Into<String>) -> Result<Self, CredentialConfigError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > MAX_CREDENTIAL_ID_BYTES
            || !value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':')
            })
        {
            return Err(CredentialConfigError::InvalidCredentialId);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CredentialId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CredentialConfigError {
    InvalidCredentialId,
}

impl fmt::Display for CredentialConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCredentialId => {
                formatter.write_str("credential identity must be 1-64 safe ASCII characters")
            }
        }
    }
}

impl std::error::Error for CredentialConfigError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedCredential {
    credential_id: CredentialId,
    execution_context: ExecutionContext,
}

impl ResolvedCredential {
    pub const fn new(credential_id: CredentialId, execution_context: ExecutionContext) -> Self {
        Self {
            credential_id,
            execution_context,
        }
    }

    pub const fn credential_id(&self) -> &CredentialId {
        &self.credential_id
    }

    pub const fn execution_context(&self) -> &ExecutionContext {
        &self.execution_context
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialResolveFailure {
    Unavailable,
}

impl fmt::Display for CredentialResolveFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("credential resolver is unavailable")
    }
}

impl std::error::Error for CredentialResolveFailure {}

/// Resolves an opaque bearer secret into one server-owned actor context.
/// Unknown, revoked, not-yet-valid, and expired credentials all return `None`.
pub trait CredentialResolver: Send + Sync {
    fn resolve(
        &self,
        bearer_token: &str,
        observed_at_epoch_seconds: u64,
    ) -> Result<Option<ResolvedCredential>, CredentialResolveFailure>;
}

pub fn valid_bearer_token(token: &str) -> bool {
    (MIN_BEARER_TOKEN_BYTES..=MAX_BEARER_TOKEN_BYTES).contains(&token.len())
        && token.bytes().all(|value| (0x21..=0x7e).contains(&value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_identity_and_bearer_envelope_are_bounded() {
        assert!(CredentialId::new("tenant-a:browser-session-7").is_ok());
        assert!(CredentialId::new("").is_err());
        assert!(CredentialId::new("bad/id").is_err());
        assert!(valid_bearer_token(&"a".repeat(MIN_BEARER_TOKEN_BYTES)));
        assert!(!valid_bearer_token("short"));
        assert!(!valid_bearer_token(&format!("{} ", "a".repeat(32))));
    }
}
