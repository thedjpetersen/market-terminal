use std::{collections::BTreeSet, fmt};

use serde::{Deserialize, Serialize};

use super::{
    LaunchpadState, LaunchpadTarget, LaunchpadTile, LaunchpadValidationError, MAX_LAUNCHPAD_TILES,
};

const PORTABLE_SCHEMA: &str = "market-terminal.launchpad";
const PORTABLE_VERSION: u16 = 1;
pub const MAX_LAUNCHPAD_DOCUMENT_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchpadImportMode {
    Merge,
    Replace,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchpadImportReport {
    pub imported: usize,
    pub skipped_duplicates: usize,
    pub replaced: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchpadDocumentError {
    TooLarge,
    InvalidJson(String),
    UnsupportedSchema,
    UnsupportedVersion(u16),
    DuplicateTile,
    Validation(LaunchpadValidationError),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct LaunchpadDocument {
    schema: String,
    version: u16,
    tiles: Vec<PortableTile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
struct PortableTile {
    label: String,
    target: LaunchpadTarget,
}

impl LaunchpadState {
    pub fn export_document(&self) -> Result<String, LaunchpadDocumentError> {
        self.validate()
            .map_err(LaunchpadDocumentError::Validation)?;
        let document = LaunchpadDocument {
            schema: PORTABLE_SCHEMA.to_owned(),
            version: PORTABLE_VERSION,
            tiles: self
                .tiles
                .iter()
                .map(|tile| PortableTile {
                    label: tile.label.clone(),
                    target: tile.target.clone(),
                })
                .collect(),
        };
        let json = serde_json::to_string_pretty(&document)
            .map_err(|error| LaunchpadDocumentError::InvalidJson(error.to_string()))?;
        if json.len() > MAX_LAUNCHPAD_DOCUMENT_BYTES {
            return Err(LaunchpadDocumentError::TooLarge);
        }
        Ok(json)
    }

    pub fn import_document(
        &mut self,
        json: &str,
        mode: LaunchpadImportMode,
    ) -> Result<LaunchpadImportReport, LaunchpadDocumentError> {
        if json.len() > MAX_LAUNCHPAD_DOCUMENT_BYTES {
            return Err(LaunchpadDocumentError::TooLarge);
        }
        let document: LaunchpadDocument = serde_json::from_str(json)
            .map_err(|error| LaunchpadDocumentError::InvalidJson(error.to_string()))?;
        if document.schema != PORTABLE_SCHEMA {
            return Err(LaunchpadDocumentError::UnsupportedSchema);
        }
        if document.version != PORTABLE_VERSION {
            return Err(LaunchpadDocumentError::UnsupportedVersion(document.version));
        }
        if document.tiles.len() > MAX_LAUNCHPAD_TILES {
            return Err(LaunchpadDocumentError::Validation(
                LaunchpadValidationError::TooManyTiles,
            ));
        }
        let mut identities = BTreeSet::new();
        for (index, tile) in document.tiles.iter().enumerate() {
            LaunchpadTile::new_target(index as u64 + 1, &tile.label, tile.target.clone())
                .map_err(LaunchpadDocumentError::Validation)?;
            if !identities.insert(tile.clone()) {
                return Err(LaunchpadDocumentError::DuplicateTile);
            }
        }

        let mut candidate = self.clone();
        let mut imported = 0;
        let mut skipped_duplicates = 0;
        match mode {
            LaunchpadImportMode::Merge => {
                let additions = document
                    .tiles
                    .into_iter()
                    .filter(|incoming| {
                        let duplicate = candidate.tiles.iter().any(|existing| {
                            existing.label == incoming.label && existing.target == incoming.target
                        });
                        skipped_duplicates += usize::from(duplicate);
                        !duplicate
                    })
                    .collect::<Vec<_>>();
                if candidate.tiles.len() + additions.len() > MAX_LAUNCHPAD_TILES {
                    return Err(LaunchpadDocumentError::Validation(
                        LaunchpadValidationError::TooManyTiles,
                    ));
                }
                for tile in additions {
                    let id = candidate.next_id;
                    candidate.next_id = candidate.next_id.saturating_add(1).max(id + 1);
                    candidate.tiles.push(
                        LaunchpadTile::new_target(id, tile.label, tile.target)
                            .map_err(LaunchpadDocumentError::Validation)?,
                    );
                    imported += 1;
                }
            }
            LaunchpadImportMode::Replace => {
                let mut next_id = candidate.next_id;
                let tiles = document
                    .tiles
                    .into_iter()
                    .map(|incoming| {
                        let id = candidate
                            .tiles
                            .iter()
                            .find(|existing| {
                                existing.label == incoming.label
                                    && existing.target == incoming.target
                            })
                            .map_or_else(
                                || {
                                    let id = next_id;
                                    next_id = next_id.saturating_add(1).max(id + 1);
                                    id
                                },
                                |existing| existing.id,
                            );
                        LaunchpadTile::new_target(id, incoming.label, incoming.target)
                            .map_err(LaunchpadDocumentError::Validation)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                imported = tiles.len();
                candidate.tiles = tiles;
                candidate.next_id = next_id.max(
                    candidate
                        .tiles
                        .iter()
                        .map(|tile| tile.id)
                        .max()
                        .unwrap_or(0)
                        .saturating_add(1),
                );
            }
        }

        if imported > 0 || mode == LaunchpadImportMode::Replace {
            candidate.revision = candidate.revision.saturating_add(1);
        }
        candidate
            .validate()
            .map_err(LaunchpadDocumentError::Validation)?;
        *self = candidate;
        Ok(LaunchpadImportReport {
            imported,
            skipped_duplicates,
            replaced: mode == LaunchpadImportMode::Replace,
        })
    }
}

impl fmt::Display for LaunchpadDocumentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge => formatter.write_str("launchpad document exceeds 64 KiB"),
            Self::InvalidJson(error) => write!(formatter, "launchpad document is invalid: {error}"),
            Self::UnsupportedSchema => {
                formatter.write_str("launchpad document schema is unsupported")
            }
            Self::UnsupportedVersion(version) => {
                write!(
                    formatter,
                    "launchpad document version {version} is unsupported"
                )
            }
            Self::DuplicateTile => {
                formatter.write_str("launchpad document contains duplicate tiles")
            }
            Self::Validation(error) => {
                write!(formatter, "launchpad document failed validation: {error}")
            }
        }
    }
}

impl std::error::Error for LaunchpadDocumentError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_document_round_trips_typed_targets_without_local_identity() {
        let source = LaunchpadState::seeded();
        let json = source.export_document().unwrap();
        assert!(!json.contains("next_id"));
        assert!(!json.contains("revision"));

        let mut restored = LaunchpadState {
            tiles: Vec::new(),
            next_id: 40,
            ..LaunchpadState::seeded()
        };
        let report = restored
            .import_document(&json, LaunchpadImportMode::Replace)
            .unwrap();
        assert_eq!(report.imported, source.tiles.len());
        assert!(report.replaced);
        assert_eq!(
            restored
                .tiles
                .iter()
                .map(|tile| (&tile.label, &tile.target))
                .collect::<Vec<_>>(),
            source
                .tiles
                .iter()
                .map(|tile| (&tile.label, &tile.target))
                .collect::<Vec<_>>()
        );
        assert!(restored.tiles.iter().all(|tile| tile.id >= 40));
    }

    #[test]
    fn merge_is_atomic_bounded_and_idempotent() {
        let mut state = LaunchpadState::seeded();
        let before = state.clone();
        let json = state.export_document().unwrap();
        let report = state
            .import_document(&json, LaunchpadImportMode::Merge)
            .unwrap();
        assert_eq!(report.imported, 0);
        assert_eq!(report.skipped_duplicates, before.tiles.len());
        assert_eq!(state, before);

        while state.tiles.len() < MAX_LAUNCHPAD_TILES {
            let index = state.tiles.len();
            state
                .add(format!("Extra {index}"), format!("HOME {index}"))
                .unwrap();
        }
        let incoming = r#"{
          "schema": "market-terminal.launchpad",
          "version": 1,
          "tiles": [
            {"label":"One More","target":{"kind":"command","command":"HOME FINAL"}}
          ]
        }"#;
        let snapshot = state.clone();
        assert!(matches!(
            state.import_document(incoming, LaunchpadImportMode::Merge),
            Err(LaunchpadDocumentError::Validation(
                LaunchpadValidationError::TooManyTiles
            ))
        ));
        assert_eq!(state, snapshot);
    }
}
