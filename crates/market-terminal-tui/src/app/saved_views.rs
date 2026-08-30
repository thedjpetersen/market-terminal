use std::{collections::BTreeMap, fmt, sync::Arc};

use serde::{Deserialize, Serialize};

use crate::features::persistence::{
    DocumentId, FeatureDocument, FeatureDocumentRepository, FeatureKey, PersistenceError,
};

pub const SAVED_VIEW_SCHEMA_VERSION: u16 = 2;
const WORKSPACE_VIEW_SCHEMA_VERSION: u16 = 1;
const MAX_SAVED_VIEWS: usize = 32;
const MAX_LABEL_BYTES: usize = 64;
const MAX_WORKSPACE_ORDER: usize = 64;
const MAX_FIELDS: usize = 64;
const MAX_CHILDREN: usize = 16;
const MAX_DEPTH: usize = 4;
const MAX_TEXT_BYTES: usize = 512;
const MAX_LIST_ITEMS: usize = 32;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ViewValue {
    Text(String),
    Unsigned(u64),
    Boolean(bool),
    TextList(Vec<String>),
}

impl ViewValue {
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(value) => Some(value),
            _ => None,
        }
    }

    pub const fn as_unsigned(&self) -> Option<u64> {
        match self {
            Self::Unsigned(value) => Some(*value),
            _ => None,
        }
    }

    pub const fn as_boolean(&self) -> Option<bool> {
        match self {
            Self::Boolean(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_text_list(&self) -> Option<&[String]> {
        match self {
            Self::TextList(value) => Some(value),
            _ => None,
        }
    }
}

/// Shell-owned, provider-neutral workspace state. Feature workspaces own their
/// field schema; the shell only validates bounds and preserves unknown data.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceViewState {
    pub schema_version: u16,
    pub workspace: String,
    pub fields: BTreeMap<String, ViewValue>,
    pub children: Vec<WorkspaceViewState>,
}

impl WorkspaceViewState {
    pub fn new(workspace: impl Into<String>) -> Self {
        Self {
            schema_version: WORKSPACE_VIEW_SCHEMA_VERSION,
            workspace: workspace.into(),
            fields: BTreeMap::new(),
            children: Vec::new(),
        }
    }

    pub fn with_field(mut self, name: impl Into<String>, value: ViewValue) -> Self {
        self.fields.insert(name.into(), value);
        self
    }

    pub fn with_child(mut self, child: WorkspaceViewState) -> Self {
        self.children.push(child);
        self
    }

    pub fn validate(&self) -> Result<(), SavedViewError> {
        self.validate_at_depth(0)
    }

    fn validate_at_depth(&self, depth: usize) -> Result<(), SavedViewError> {
        if depth > MAX_DEPTH {
            return Err(SavedViewError::Invalid(
                "workspace state nesting is too deep",
            ));
        }
        if self.schema_version == 0 {
            return Err(SavedViewError::Invalid("workspace state schema is invalid"));
        }
        validate_identifier(&self.workspace, "workspace")?;
        if self.fields.len() > MAX_FIELDS {
            return Err(SavedViewError::Invalid(
                "workspace state has too many fields",
            ));
        }
        if self.children.len() > MAX_CHILDREN {
            return Err(SavedViewError::Invalid(
                "workspace state has too many children",
            ));
        }
        for (name, value) in &self.fields {
            validate_identifier(name, "workspace field")?;
            validate_value(value)?;
        }
        for child in &self.children {
            child.validate_at_depth(depth + 1)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ViewRestoreReport {
    pub restored_fields: usize,
    pub skipped_fields: usize,
    pub warnings: Vec<String>,
}

impl ViewRestoreReport {
    pub fn restored(count: usize) -> Self {
        Self {
            restored_fields: count,
            ..Self::default()
        }
    }

    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            warnings: vec![message.into()],
            ..Self::default()
        }
    }

    pub fn merge(&mut self, other: Self) {
        self.restored_fields += other.restored_fields;
        self.skipped_fields += other.skipped_fields;
        self.warnings.extend(other.warnings);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SavedView {
    pub id: u64,
    pub label: String,
    pub revision: u64,
    pub active_workspace: String,
    pub workspace_order: Vec<String>,
    pub workspace_state: WorkspaceViewState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SavedViewCatalog {
    pub schema_version: u16,
    pub revision: u64,
    pub next_id: u64,
    pub views: Vec<SavedView>,
}

impl Default for SavedViewCatalog {
    fn default() -> Self {
        Self {
            schema_version: SAVED_VIEW_SCHEMA_VERSION,
            revision: 0,
            next_id: 1,
            views: Vec::new(),
        }
    }
}

impl SavedViewCatalog {
    pub fn validate(&self) -> Result<(), SavedViewError> {
        if self.schema_version != SAVED_VIEW_SCHEMA_VERSION {
            return Err(SavedViewError::UnsupportedSchema(self.schema_version));
        }
        if self.views.len() > MAX_SAVED_VIEWS {
            return Err(SavedViewError::Invalid(
                "saved view count exceeds its limit",
            ));
        }
        let mut ids = std::collections::BTreeSet::new();
        for view in &self.views {
            if view.id == 0 || !ids.insert(view.id) {
                return Err(SavedViewError::Invalid(
                    "saved view IDs must be unique and nonzero",
                ));
            }
            validate_label(&view.label)?;
            validate_identifier(&view.active_workspace, "active workspace")?;
            if view.workspace_order.len() > MAX_WORKSPACE_ORDER {
                return Err(SavedViewError::Invalid("workspace order exceeds its limit"));
            }
            for workspace in &view.workspace_order {
                validate_identifier(workspace, "workspace order entry")?;
            }
            view.workspace_state.validate()?;
        }
        let maximum = self.views.iter().map(|view| view.id).max().unwrap_or(0);
        if self.next_id == 0 || self.next_id <= maximum {
            return Err(SavedViewError::Invalid("saved view next ID is invalid"));
        }
        Ok(())
    }

    pub fn save(
        &mut self,
        label: impl Into<String>,
        active_workspace: impl Into<String>,
        workspace_order: Vec<String>,
        workspace_state: WorkspaceViewState,
    ) -> Result<&SavedView, SavedViewError> {
        let label = label.into().trim().to_owned();
        validate_label(&label)?;
        workspace_state.validate()?;
        let active_workspace = active_workspace.into();
        validate_identifier(&active_workspace, "active workspace")?;
        if workspace_order.len() > MAX_WORKSPACE_ORDER {
            return Err(SavedViewError::Invalid("workspace order exceeds its limit"));
        }
        if let Some(index) = self
            .views
            .iter()
            .position(|view| view.label.eq_ignore_ascii_case(&label))
        {
            let view = &mut self.views[index];
            view.label = label;
            view.revision = view.revision.saturating_add(1);
            view.active_workspace = active_workspace;
            view.workspace_order = workspace_order;
            view.workspace_state = workspace_state;
            self.revision = self.revision.saturating_add(1);
            return Ok(&self.views[index]);
        }
        if self.views.len() == MAX_SAVED_VIEWS {
            return Err(SavedViewError::Invalid(
                "saved view count exceeds its limit",
            ));
        }
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1).max(id + 1);
        self.views.push(SavedView {
            id,
            label,
            revision: 1,
            active_workspace,
            workspace_order,
            workspace_state,
        });
        self.revision = self.revision.saturating_add(1);
        Ok(self.views.last().expect("saved view was appended"))
    }

    pub fn find(&self, reference: &str) -> Option<&SavedView> {
        let reference = reference.trim();
        reference
            .parse::<u64>()
            .ok()
            .and_then(|id| self.views.iter().find(|view| view.id == id))
            .or_else(|| {
                self.views
                    .iter()
                    .find(|view| view.label.eq_ignore_ascii_case(reference))
            })
    }

    pub fn delete(&mut self, reference: &str) -> Option<SavedView> {
        let id = self.find(reference)?.id;
        let index = self.views.iter().position(|view| view.id == id)?;
        let removed = self.views.remove(index);
        self.revision = self.revision.saturating_add(1);
        Some(removed)
    }

    fn from_payload(payload: serde_json::Value) -> Result<Self, SavedViewError> {
        let version = payload
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
            .ok_or(SavedViewError::Invalid("saved view schema is missing"))?;
        let catalog = match version {
            1 => migrate_v1(serde_json::from_value(payload).map_err(SavedViewError::decode)?),
            version if version == u64::from(SAVED_VIEW_SCHEMA_VERSION) => {
                serde_json::from_value(payload).map_err(SavedViewError::decode)?
            }
            version => {
                return Err(SavedViewError::UnsupportedSchema(
                    u16::try_from(version).unwrap_or(u16::MAX),
                ));
            }
        };
        catalog.validate()?;
        Ok(catalog)
    }
}

#[derive(Deserialize)]
struct SavedViewCatalogV1 {
    revision: u64,
    next_id: u64,
    views: Vec<SavedViewV1>,
}

#[derive(Deserialize)]
struct SavedViewV1 {
    id: u64,
    label: String,
    active_workspace: String,
    workspace_order: Vec<String>,
}

fn migrate_v1(previous: SavedViewCatalogV1) -> SavedViewCatalog {
    SavedViewCatalog {
        schema_version: SAVED_VIEW_SCHEMA_VERSION,
        revision: previous.revision,
        next_id: previous.next_id,
        views: previous
            .views
            .into_iter()
            .map(|view| SavedView {
                id: view.id,
                label: view.label,
                revision: 1,
                workspace_state: WorkspaceViewState::new(&view.active_workspace),
                active_workspace: view.active_workspace,
                workspace_order: view.workspace_order,
            })
            .collect(),
    }
}

pub struct SavedViewPersistence {
    repository: Arc<dyn FeatureDocumentRepository>,
}

impl SavedViewPersistence {
    pub fn new(repository: Arc<dyn FeatureDocumentRepository>) -> Self {
        Self { repository }
    }

    pub fn load(&self) -> Result<SavedViewCatalog, SavedViewError> {
        let feature = feature_key()?;
        let id = document_id()?;
        let Some(document) = self.repository.load(&feature, &id)? else {
            return Ok(SavedViewCatalog::default());
        };
        SavedViewCatalog::from_payload(document.payload().clone())
    }

    pub fn save(&self, catalog: &SavedViewCatalog) -> Result<(), SavedViewError> {
        catalog.validate()?;
        let document = FeatureDocument::new(
            feature_key()?,
            document_id()?,
            catalog.revision,
            serde_json::to_value(catalog).map_err(SavedViewError::encode)?,
        )?;
        self.repository.save(&document)?;
        Ok(())
    }
}

fn feature_key() -> Result<FeatureKey, SavedViewError> {
    Ok(FeatureKey::new("saved_views")?)
}

fn document_id() -> Result<DocumentId, SavedViewError> {
    Ok(DocumentId::new("catalog")?)
}

fn validate_identifier(value: &str, field: &'static str) -> Result<(), SavedViewError> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(SavedViewError::Invalid(field));
    }
    Ok(())
}

fn validate_label(value: &str) -> Result<(), SavedViewError> {
    if value.trim().is_empty()
        || value.len() > MAX_LABEL_BYTES
        || value
            .chars()
            .any(|character| matches!(character, '\r' | '\n' | '\0'))
    {
        return Err(SavedViewError::Invalid("saved view label is invalid"));
    }
    Ok(())
}

fn validate_value(value: &ViewValue) -> Result<(), SavedViewError> {
    match value {
        ViewValue::Text(value) if value.len() <= MAX_TEXT_BYTES => Ok(()),
        ViewValue::TextList(values)
            if values.len() <= MAX_LIST_ITEMS
                && values.iter().all(|value| value.len() <= MAX_TEXT_BYTES) =>
        {
            Ok(())
        }
        ViewValue::Unsigned(_) | ViewValue::Boolean(_) => Ok(()),
        ViewValue::Text(_) | ViewValue::TextList(_) => Err(SavedViewError::Invalid(
            "workspace field value exceeds its limit",
        )),
    }
}

#[derive(Debug)]
pub enum SavedViewError {
    Invalid(&'static str),
    UnsupportedSchema(u16),
    Persistence(PersistenceError),
    Json(String),
}

impl SavedViewError {
    fn decode(error: serde_json::Error) -> Self {
        Self::Json(format!("saved view document is corrupt: {error}"))
    }

    fn encode(error: serde_json::Error) -> Self {
        Self::Json(format!("saved view document could not be encoded: {error}"))
    }
}

impl fmt::Display for SavedViewError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(message) => formatter.write_str(message),
            Self::UnsupportedSchema(version) => {
                write!(formatter, "unsupported saved view schema version {version}")
            }
            Self::Persistence(error) => error.fmt(formatter),
            Self::Json(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for SavedViewError {}

impl From<PersistenceError> for SavedViewError {
    fn from(error: PersistenceError) -> Self {
        Self::Persistence(error)
    }
}

impl From<crate::features::persistence::PersistenceValidationError> for SavedViewError {
    fn from(error: crate::features::persistence::PersistenceValidationError) -> Self {
        Self::Persistence(PersistenceError::Validation(error))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_updates_case_insensitive_identity_without_losing_stable_id() {
        let mut catalog = SavedViewCatalog::default();
        let state = WorkspaceViewState::new("desk")
            .with_field("focused_pane", ViewValue::Text("charting".to_owned()));
        let id = catalog
            .save("Research", "desk", vec!["desk".to_owned()], state.clone())
            .unwrap()
            .id;
        catalog
            .save("research", "desk", vec!["desk".to_owned()], state)
            .unwrap();

        assert_eq!(catalog.views.len(), 1);
        assert_eq!(catalog.views[0].id, id);
        assert_eq!(catalog.views[0].revision, 2);
        assert_eq!(catalog.revision, 2);
        catalog.validate().unwrap();
    }

    #[test]
    fn v1_catalog_migrates_to_typed_workspace_state() {
        let payload = serde_json::json!({
            "schema_version": 1,
            "revision": 4,
            "next_id": 2,
            "views": [{
                "id": 1,
                "label": "Desk",
                "active_workspace": "desk",
                "workspace_order": ["desk", "charting"]
            }]
        });

        let catalog = SavedViewCatalog::from_payload(payload).unwrap();
        assert_eq!(catalog.schema_version, SAVED_VIEW_SCHEMA_VERSION);
        assert_eq!(catalog.views[0].workspace_state.workspace, "desk");
        assert!(catalog.views[0].workspace_state.fields.is_empty());
    }

    #[test]
    fn nested_workspace_state_is_bounded_and_typed() {
        let state = WorkspaceViewState::new("desk").with_child(
            WorkspaceViewState::new("charting")
                .with_field("period", ViewValue::Text("1Y".to_owned()))
                .with_field("cursor", ViewValue::Unsigned(8))
                .with_field("normalized", ViewValue::Boolean(true)),
        );
        state.validate().unwrap();

        let encoded = serde_json::to_value(&state).unwrap();
        let decoded: WorkspaceViewState = serde_json::from_value(encoded).unwrap();
        assert_eq!(decoded, state);
    }
}
