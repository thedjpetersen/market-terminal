use std::collections::HashSet;

use serde::{Deserialize, Serialize};

const PRESET_DOCUMENT: &str = include_str!("../../assets/workspace-presets.json");
const MAX_PRESETS: usize = 16;
const MAX_WORKSPACES: usize = 64;
const MAX_TEXT_BYTES: usize = 512;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub(crate) struct WorkspacePresetCatalog {
    schema_version: u16,
    presets: Vec<WorkspacePreset>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub(crate) struct WorkspacePreset {
    pub id: String,
    pub version: u16,
    pub label: String,
    pub description: String,
    pub workspace_order: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkspacePresetPreview {
    pub id: String,
    pub version: u16,
    pub label: String,
    pub description: String,
    pub current_order: Vec<String>,
    pub proposed_order: Vec<String>,
    pub unavailable: Vec<String>,
    pub restoring_custom: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct WorkspaceReturnPoint {
    pub active_workspace: String,
    pub workspace_order: Vec<String>,
}

impl WorkspacePresetCatalog {
    pub fn built_in() -> Self {
        let catalog: Self = serde_json::from_str(PRESET_DOCUMENT)
            .expect("built-in workspace preset document must be valid JSON");
        catalog
            .validate()
            .expect("built-in workspace preset document must satisfy its contract");
        catalog
    }

    pub fn find(&self, id: &str) -> Option<&WorkspacePreset> {
        self.presets
            .iter()
            .find(|preset| preset.id.eq_ignore_ascii_case(id.trim()))
    }

    pub fn labels(&self) -> String {
        self.presets
            .iter()
            .map(|preset| preset.id.to_ascii_uppercase())
            .collect::<Vec<_>>()
            .join(" · ")
    }

    fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err("unsupported workspace preset schema".to_owned());
        }
        if self.presets.is_empty() || self.presets.len() > MAX_PRESETS {
            return Err("workspace preset count is outside its bounded contract".to_owned());
        }
        let mut preset_ids = HashSet::new();
        for preset in &self.presets {
            if preset.version == 0
                || preset.id.is_empty()
                || !preset
                    .id
                    .chars()
                    .all(|character| character.is_ascii_lowercase() || character == '_')
                || !preset_ids.insert(preset.id.as_str())
                || preset.label.is_empty()
                || preset.label.len() > MAX_TEXT_BYTES
                || preset.description.is_empty()
                || preset.description.len() > MAX_TEXT_BYTES
                || preset.workspace_order.is_empty()
                || preset.workspace_order.len() > MAX_WORKSPACES
            {
                return Err(format!("invalid workspace preset: {}", preset.id));
            }
            let mut workspaces = HashSet::new();
            if preset.workspace_order.iter().any(|workspace| {
                workspace.is_empty()
                    || workspace.len() > MAX_TEXT_BYTES
                    || !workspaces.insert(workspace.as_str())
            }) {
                return Err(format!("invalid workspace order: {}", preset.id));
            }
        }
        Ok(())
    }
}

impl WorkspaceReturnPoint {
    pub fn encode(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    pub fn decode(encoded: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(encoded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_catalog_has_versioned_role_seeds() {
        let catalog = WorkspacePresetCatalog::built_in();
        assert_eq!(catalog.presets.len(), 5);
        for id in ["trader", "quant", "pm", "risk", "ops"] {
            let preset = catalog.find(id).expect("role seed exists");
            assert_eq!(preset.version, 1);
            assert!(!preset.workspace_order.is_empty());
        }
    }

    #[test]
    fn return_point_round_trips_without_losing_order() {
        let expected = WorkspaceReturnPoint {
            active_workspace: "portfolio".to_owned(),
            workspace_order: vec!["portfolio".to_owned(), "overview".to_owned()],
        };
        let encoded = expected.encode().expect("encode");
        assert_eq!(WorkspaceReturnPoint::decode(&encoded).unwrap(), expected);
    }
}
