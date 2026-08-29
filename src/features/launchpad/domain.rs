use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

pub const MAX_LAUNCHPAD_TILES: usize = 24;
pub const MAX_TILE_LABEL_BYTES: usize = 48;
pub const MAX_TILE_COMMAND_BYTES: usize = 512;
pub const LAUNCHPAD_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LaunchpadTile {
    pub id: u64,
    pub label: String,
    pub command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LaunchpadState {
    pub schema_version: u16,
    pub revision: u64,
    pub next_id: u64,
    pub tiles: Vec<LaunchpadTile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchpadValidationError {
    TooManyTiles,
    DuplicateTileId,
    InvalidNextId,
    UnsupportedSchemaVersion,
    InvalidLabel,
    InvalidCommand,
}

impl LaunchpadTile {
    pub fn new(
        id: u64,
        label: impl Into<String>,
        command: impl Into<String>,
    ) -> Result<Self, LaunchpadValidationError> {
        let tile = Self {
            id,
            label: label.into(),
            command: command.into(),
        };
        tile.validate()?;
        Ok(tile)
    }

    pub fn validate(&self) -> Result<(), LaunchpadValidationError> {
        if self.id == 0 {
            return Err(LaunchpadValidationError::DuplicateTileId);
        }
        if !valid_text(&self.label, MAX_TILE_LABEL_BYTES) {
            return Err(LaunchpadValidationError::InvalidLabel);
        }
        if !valid_text(&self.command, MAX_TILE_COMMAND_BYTES) {
            return Err(LaunchpadValidationError::InvalidCommand);
        }
        Ok(())
    }
}

impl LaunchpadState {
    pub fn seeded() -> Self {
        let seeds = [
            ("Mission Control", "HOME"),
            ("Trading Desk", "DESK"),
            ("Markets", "MARKETS"),
            ("Portfolio", "PORT"),
            ("Risk", "RISK"),
            ("News", "NEWS"),
            ("Spreadsheet", "SHEET"),
            ("Find Security", "FIND US"),
        ];
        let tiles = seeds
            .into_iter()
            .enumerate()
            .map(|(index, (label, command))| {
                LaunchpadTile::new(index as u64 + 1, label, command)
                    .expect("built-in launch tile must be valid")
            })
            .collect();
        Self {
            schema_version: LAUNCHPAD_SCHEMA_VERSION,
            revision: 0,
            next_id: 9,
            tiles,
        }
    }

    pub fn validate(&self) -> Result<(), LaunchpadValidationError> {
        if self.schema_version != LAUNCHPAD_SCHEMA_VERSION {
            return Err(LaunchpadValidationError::UnsupportedSchemaVersion);
        }
        if self.tiles.len() > MAX_LAUNCHPAD_TILES {
            return Err(LaunchpadValidationError::TooManyTiles);
        }
        let mut ids = BTreeSet::new();
        for tile in &self.tiles {
            tile.validate()?;
            if !ids.insert(tile.id) {
                return Err(LaunchpadValidationError::DuplicateTileId);
            }
        }
        let maximum = self.tiles.iter().map(|tile| tile.id).max().unwrap_or(0);
        if self.next_id == 0 || self.next_id <= maximum {
            return Err(LaunchpadValidationError::InvalidNextId);
        }
        Ok(())
    }

    pub fn add(
        &mut self,
        label: impl Into<String>,
        command: impl Into<String>,
    ) -> Result<u64, LaunchpadValidationError> {
        if self.tiles.len() >= MAX_LAUNCHPAD_TILES {
            return Err(LaunchpadValidationError::TooManyTiles);
        }
        let id = self.next_id;
        let tile = LaunchpadTile::new(id, label, command)?;
        self.tiles.push(tile);
        self.next_id = self.next_id.saturating_add(1).max(id + 1);
        self.bump_revision();
        Ok(id)
    }

    pub fn rename(
        &mut self,
        index: usize,
        label: impl Into<String>,
    ) -> Result<(), LaunchpadValidationError> {
        let label = label.into();
        if !valid_text(&label, MAX_TILE_LABEL_BYTES) {
            return Err(LaunchpadValidationError::InvalidLabel);
        }
        let tile = self
            .tiles
            .get_mut(index)
            .ok_or(LaunchpadValidationError::InvalidLabel)?;
        tile.label = label;
        self.bump_revision();
        Ok(())
    }

    pub fn remove(&mut self, index: usize) -> bool {
        if index >= self.tiles.len() {
            return false;
        }
        self.tiles.remove(index);
        self.bump_revision();
        true
    }

    pub fn move_tile(&mut self, from: usize, to: usize) -> bool {
        if from >= self.tiles.len() || to >= self.tiles.len() || from == to {
            return false;
        }
        let tile = self.tiles.remove(from);
        self.tiles.insert(to, tile);
        self.bump_revision();
        true
    }

    fn bump_revision(&mut self) {
        self.revision = self.revision.saturating_add(1);
    }
}

fn valid_text(value: &str, maximum: usize) -> bool {
    let value = value.trim();
    !value.is_empty()
        && value.len() <= maximum
        && !value
            .chars()
            .any(|character| matches!(character, '\r' | '\n' | '\0'))
}

impl std::fmt::Display for LaunchpadValidationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::TooManyTiles => "launchpad tile count exceeds its limit",
            Self::DuplicateTileId => "launchpad tile IDs must be unique and nonzero",
            Self::InvalidNextId => "launchpad next tile ID is invalid",
            Self::UnsupportedSchemaVersion => "launchpad schema version is unsupported",
            Self::InvalidLabel => "launchpad label is empty, unsafe, or too long",
            Self::InvalidCommand => "launchpad command is empty, unsafe, or too long",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for LaunchpadValidationError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeded_state_is_bounded_and_every_tile_has_a_stable_identity() {
        let state = LaunchpadState::seeded();
        state.validate().unwrap();
        assert_eq!(state.tiles.len(), 8);
        assert_eq!(state.schema_version, LAUNCHPAD_SCHEMA_VERSION);
        assert_eq!(state.next_id, 9);
    }

    #[test]
    fn edits_increment_revision_and_preserve_identity() {
        let mut state = LaunchpadState::seeded();
        let first_id = state.tiles[0].id;
        let id = state.add("Apple", "SEC AAPL US").unwrap();
        state.rename(0, "Home").unwrap();
        assert!(state.move_tile(0, 2));
        assert!(state.remove(1));
        assert_eq!(state.revision, 4);
        assert_eq!(id, 9);
        assert!(state.tiles.iter().any(|tile| tile.id == first_id));
        state.validate().unwrap();
    }
}
