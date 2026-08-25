use std::{collections::BTreeMap, fmt};

use serde::{Deserialize, Serialize};

pub const MAX_WORKSPACES: usize = 64;
pub const MAX_RECENT_COMMANDS: usize = 100;
pub const MAX_PREFERENCES: usize = 128;
pub const MAX_DOCUMENT_BYTES: usize = 1_048_576;

const MAX_WORKSPACE_ID_BYTES: usize = 64;
const MAX_COMMAND_BYTES: usize = 512;
const MAX_PREFERENCE_KEY_BYTES: usize = 64;
const MAX_PREFERENCE_VALUE_BYTES: usize = 1_024;
const MAX_IDENTIFIER_BYTES: usize = 64;

/// Durable shell state. The shell owns when this snapshot is captured; the
/// persistence feature owns validation and storage semantics.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionState {
    active_workspace: Option<String>,
    workspace_order: Vec<String>,
    recent_commands: Vec<String>,
    preferences: BTreeMap<String, String>,
}

impl SessionState {
    pub fn new(
        active_workspace: Option<String>,
        workspace_order: Vec<String>,
        recent_commands: Vec<String>,
        preferences: BTreeMap<String, String>,
    ) -> Result<Self, PersistenceValidationError> {
        let state = Self {
            active_workspace,
            workspace_order,
            recent_commands,
            preferences,
        };
        state.validate()?;
        Ok(state)
    }

    pub fn active_workspace(&self) -> Option<&str> {
        self.active_workspace.as_deref()
    }

    pub fn workspace_order(&self) -> &[String] {
        &self.workspace_order
    }

    pub fn recent_commands(&self) -> &[String] {
        &self.recent_commands
    }

    pub fn preferences(&self) -> &BTreeMap<String, String> {
        &self.preferences
    }

    pub fn validate(&self) -> Result<(), PersistenceValidationError> {
        if self.workspace_order.len() > MAX_WORKSPACES {
            return Err(PersistenceValidationError::TooManyWorkspaces);
        }
        if self.recent_commands.len() > MAX_RECENT_COMMANDS {
            return Err(PersistenceValidationError::TooManyRecentCommands);
        }
        if self.preferences.len() > MAX_PREFERENCES {
            return Err(PersistenceValidationError::TooManyPreferences);
        }

        if let Some(active) = &self.active_workspace {
            validate_workspace_id(active)?;
        }
        for workspace in &self.workspace_order {
            validate_workspace_id(workspace)?;
        }
        for command in &self.recent_commands {
            validate_nonempty_bounded(command, MAX_COMMAND_BYTES, "command")?;
        }
        for (key, value) in &self.preferences {
            validate_nonempty_bounded(key, MAX_PREFERENCE_KEY_BYTES, "preference key")?;
            if value.len() > MAX_PREFERENCE_VALUE_BYTES {
                return Err(PersistenceValidationError::FieldTooLong("preference value"));
            }
        }
        Ok(())
    }
}

fn validate_workspace_id(value: &str) -> Result<(), PersistenceValidationError> {
    validate_identifier(value, MAX_WORKSPACE_ID_BYTES, "workspace id")
}

/// A safe path component identifying a feature-owned document collection.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FeatureKey(String);

impl FeatureKey {
    pub fn new(value: impl Into<String>) -> Result<Self, PersistenceValidationError> {
        let value = value.into();
        validate_identifier(&value, MAX_IDENTIFIER_BYTES, "feature key")?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A safe path component identifying one document within a feature.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DocumentId(String);

impl DocumentId {
    pub fn new(value: impl Into<String>) -> Result<Self, PersistenceValidationError> {
        let value = value.into();
        validate_identifier(&value, MAX_IDENTIFIER_BYTES, "document id")?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A feature document is deliberately opaque to the shell and adapter. Each
/// feature maps its domain model to a versioned JSON payload at its own edge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureDocument {
    feature: FeatureKey,
    id: DocumentId,
    revision: u64,
    payload: serde_json::Value,
}

impl FeatureDocument {
    pub fn new(
        feature: FeatureKey,
        id: DocumentId,
        revision: u64,
        payload: serde_json::Value,
    ) -> Result<Self, PersistenceValidationError> {
        let document = Self { feature, id, revision, payload };
        document.validate()?;
        Ok(document)
    }

    pub fn feature(&self) -> &FeatureKey {
        &self.feature
    }

    pub fn id(&self) -> &DocumentId {
        &self.id
    }

    pub const fn revision(&self) -> u64 {
        self.revision
    }

    pub const fn payload(&self) -> &serde_json::Value {
        &self.payload
    }

    pub fn validate(&self) -> Result<(), PersistenceValidationError> {
        validate_identifier(self.feature.as_str(), MAX_IDENTIFIER_BYTES, "feature key")?;
        validate_identifier(self.id.as_str(), MAX_IDENTIFIER_BYTES, "document id")?;
        let encoded = serde_json::to_vec(&self.payload)
            .map_err(|_| PersistenceValidationError::InvalidPayload)?;
        if encoded.len() > MAX_DOCUMENT_BYTES {
            return Err(PersistenceValidationError::DocumentTooLarge);
        }
        Ok(())
    }
}

fn validate_identifier(
    value: &str,
    maximum: usize,
    field: &'static str,
) -> Result<(), PersistenceValidationError> {
    validate_nonempty_bounded(value, maximum, field)?;
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(PersistenceValidationError::UnsafeIdentifier(field));
    }
    Ok(())
}

fn validate_nonempty_bounded(
    value: &str,
    maximum: usize,
    field: &'static str,
) -> Result<(), PersistenceValidationError> {
    if value.is_empty() {
        return Err(PersistenceValidationError::EmptyField(field));
    }
    if value.len() > maximum {
        return Err(PersistenceValidationError::FieldTooLong(field));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersistenceValidationError {
    EmptyField(&'static str),
    FieldTooLong(&'static str),
    UnsafeIdentifier(&'static str),
    TooManyWorkspaces,
    TooManyRecentCommands,
    TooManyPreferences,
    DocumentTooLarge,
    InvalidPayload,
}

impl fmt::Display for PersistenceValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyField(field) => write!(formatter, "{field} cannot be empty"),
            Self::FieldTooLong(field) => write!(formatter, "{field} exceeds its size limit"),
            Self::UnsafeIdentifier(field) => {
                write!(formatter, "{field} contains an unsafe path character")
            }
            Self::TooManyWorkspaces => write!(formatter, "workspace order exceeds its limit"),
            Self::TooManyRecentCommands => {
                write!(formatter, "recent command history exceeds its limit")
            }
            Self::TooManyPreferences => write!(formatter, "preference count exceeds its limit"),
            Self::DocumentTooLarge => write!(formatter, "feature document exceeds its size limit"),
            Self::InvalidPayload => write!(formatter, "feature document payload is not serializable"),
        }
    }
}

impl std::error::Error for PersistenceValidationError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_reject_traversal_and_separators() {
        for value in ["../secret", "a/b", r"a\b", "."] {
            assert!(FeatureKey::new(value).is_err());
            assert!(DocumentId::new(value).is_err());
        }
        assert_eq!(FeatureKey::new("spreadsheet_v2").unwrap().as_str(), "spreadsheet_v2");
    }

    #[test]
    fn session_state_enforces_history_bounds() {
        let commands = (0..=MAX_RECENT_COMMANDS).map(|index| format!("CMD {index}")).collect();
        assert_eq!(
            SessionState::new(None, Vec::new(), commands, BTreeMap::new()),
            Err(PersistenceValidationError::TooManyRecentCommands)
        );
    }

    #[test]
    fn documents_enforce_serialized_payload_size() {
        let payload = serde_json::Value::String("x".repeat(MAX_DOCUMENT_BYTES));
        let error = FeatureDocument::new(
            FeatureKey::new("spreadsheet").unwrap(),
            DocumentId::new("main").unwrap(),
            1,
            payload,
        )
        .unwrap_err();
        assert_eq!(error, PersistenceValidationError::DocumentTooLarge);
    }
}
