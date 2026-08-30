use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

pub const MAX_LAUNCHPAD_TILES: usize = 24;
pub const MAX_TILE_LABEL_BYTES: usize = 48;
pub const MAX_TILE_COMMAND_BYTES: usize = 512;
pub const MAX_TARGET_ID_BYTES: usize = 128;
pub const LAUNCHPAD_SCHEMA_VERSION: u16 = 2;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LaunchpadTarget {
    Command {
        command: String,
    },
    Instrument {
        canonical_id: String,
        symbol: String,
        workspace: String,
    },
    Screen {
        screen_id: String,
        command: String,
    },
    Portfolio {
        portfolio_id: String,
        view: Option<String>,
    },
    Sheet {
        workbook_id: String,
    },
    Layout {
        saved_view: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LaunchpadTile {
    pub id: u64,
    pub label: String,
    pub target: LaunchpadTarget,
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
    InvalidTarget,
}

impl LaunchpadTile {
    pub fn new_command(
        id: u64,
        label: impl Into<String>,
        command: impl Into<String>,
    ) -> Result<Self, LaunchpadValidationError> {
        Self::new_target(
            id,
            label,
            LaunchpadTarget::Command {
                command: command.into(),
            },
        )
    }

    pub fn new_target(
        id: u64,
        label: impl Into<String>,
        target: LaunchpadTarget,
    ) -> Result<Self, LaunchpadValidationError> {
        let tile = Self {
            id,
            label: label.into(),
            target,
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
        self.target.validate()?;
        Ok(())
    }

    pub fn command(&self) -> String {
        self.target.command()
    }
}

impl LaunchpadTarget {
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Command { .. } => "COMMAND",
            Self::Instrument { .. } => "INSTRUMENT",
            Self::Screen { .. } => "SCREEN",
            Self::Portfolio { .. } => "PORTFOLIO",
            Self::Sheet { .. } => "SHEET",
            Self::Layout { .. } => "LAYOUT",
        }
    }

    pub fn command(&self) -> String {
        match self {
            Self::Command { command } | Self::Screen { command, .. } => command.clone(),
            Self::Instrument {
                symbol, workspace, ..
            } => format!("{workspace} {symbol}"),
            Self::Portfolio { view, .. } => view
                .as_deref()
                .map_or_else(|| "PORT".to_owned(), |view| format!("PORT {view}")),
            Self::Sheet { workbook_id } => {
                format!("SHEET LOAD {}", quoted_argument(workbook_id))
            }
            Self::Layout { saved_view } => {
                format!("VIEW RESTORE {}", quoted_argument(saved_view))
            }
        }
    }

    pub fn validate(&self) -> Result<(), LaunchpadValidationError> {
        match self {
            Self::Command { command } => validate_command(command),
            Self::Instrument {
                canonical_id,
                symbol,
                workspace,
            } => {
                validate_id(canonical_id)?;
                validate_id(symbol)?;
                validate_token(workspace)
            }
            Self::Screen { screen_id, command } => {
                validate_id(screen_id)?;
                validate_command(command)
            }
            Self::Portfolio { portfolio_id, view } => {
                validate_id(portfolio_id)?;
                if let Some(view) = view {
                    validate_token(view)?;
                }
                Ok(())
            }
            Self::Sheet { workbook_id } => validate_id(workbook_id),
            Self::Layout { saved_view } => validate_id(saved_view),
        }
    }
}

impl LaunchpadState {
    pub fn seeded() -> Self {
        let seeds = vec![
            (
                "Mission Control",
                LaunchpadTarget::Command {
                    command: "HOME".to_owned(),
                },
            ),
            (
                "Trading Desk",
                LaunchpadTarget::Command {
                    command: "DESK".to_owned(),
                },
            ),
            (
                "Markets",
                LaunchpadTarget::Command {
                    command: "MARKETS".to_owned(),
                },
            ),
            (
                "Portfolio",
                LaunchpadTarget::Portfolio {
                    portfolio_id: "default".to_owned(),
                    view: None,
                },
            ),
            (
                "Risk",
                LaunchpadTarget::Command {
                    command: "RISK".to_owned(),
                },
            ),
            (
                "News",
                LaunchpadTarget::Command {
                    command: "NEWS".to_owned(),
                },
            ),
            (
                "Spreadsheet",
                LaunchpadTarget::Sheet {
                    workbook_id: "default".to_owned(),
                },
            ),
            (
                "Find Security",
                LaunchpadTarget::Screen {
                    screen_id: "find-us".to_owned(),
                    command: "FIND US".to_owned(),
                },
            ),
        ];
        let tiles = seeds
            .into_iter()
            .enumerate()
            .map(|(index, (label, target))| {
                LaunchpadTile::new_target(index as u64 + 1, label, target)
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
        self.add_target(
            label,
            LaunchpadTarget::Command {
                command: command.into(),
            },
        )
    }

    pub fn add_target(
        &mut self,
        label: impl Into<String>,
        target: LaunchpadTarget,
    ) -> Result<u64, LaunchpadValidationError> {
        if self.tiles.len() >= MAX_LAUNCHPAD_TILES {
            return Err(LaunchpadValidationError::TooManyTiles);
        }
        let id = self.next_id;
        let tile = LaunchpadTile::new_target(id, label, target)?;
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

fn validate_command(command: &str) -> Result<(), LaunchpadValidationError> {
    if valid_text(command, MAX_TILE_COMMAND_BYTES) {
        Ok(())
    } else {
        Err(LaunchpadValidationError::InvalidCommand)
    }
}

fn validate_id(value: &str) -> Result<(), LaunchpadValidationError> {
    if valid_text(value, MAX_TARGET_ID_BYTES) {
        Ok(())
    } else {
        Err(LaunchpadValidationError::InvalidTarget)
    }
}

fn validate_token(value: &str) -> Result<(), LaunchpadValidationError> {
    if !value.is_empty()
        && value.len() <= MAX_TARGET_ID_BYTES
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
    {
        Ok(())
    } else {
        Err(LaunchpadValidationError::InvalidTarget)
    }
}

fn quoted_argument(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
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
            Self::InvalidTarget => "launchpad target identity is empty, unsafe, or too long",
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

    #[test]
    fn typed_targets_validate_identity_and_generate_exact_commands() {
        let targets = [
            LaunchpadTarget::Instrument {
                canonical_id: "equity:us:AAPL".to_owned(),
                symbol: "AAPL US".to_owned(),
                workspace: "SEC".to_owned(),
            },
            LaunchpadTarget::Screen {
                screen_id: "us-movers".to_owned(),
                command: "FIND US".to_owned(),
            },
            LaunchpadTarget::Portfolio {
                portfolio_id: "default".to_owned(),
                view: Some("LOTS".to_owned()),
            },
            LaunchpadTarget::Sheet {
                workbook_id: "valuation model".to_owned(),
            },
            LaunchpadTarget::Layout {
                saved_view: "Morning Research".to_owned(),
            },
        ];
        assert_eq!(
            targets
                .iter()
                .map(LaunchpadTarget::command)
                .collect::<Vec<_>>(),
            vec![
                "SEC AAPL US",
                "FIND US",
                "PORT LOTS",
                "SHEET LOAD \"valuation model\"",
                "VIEW RESTORE \"Morning Research\"",
            ]
        );
        assert!(targets.iter().all(|target| target.validate().is_ok()));
        assert!(matches!(
            LaunchpadTarget::Instrument {
                canonical_id: "equity:us:AAPL".to_owned(),
                symbol: "AAPL US".to_owned(),
                workspace: "SEC NOW".to_owned(),
            }
            .validate(),
            Err(LaunchpadValidationError::InvalidTarget)
        ));
    }
}
