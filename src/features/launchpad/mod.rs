mod domain;
mod port;
mod workspace;

pub use domain::{
    LaunchpadState, LaunchpadTile, LaunchpadValidationError, LAUNCHPAD_SCHEMA_VERSION,
    MAX_LAUNCHPAD_TILES, MAX_TILE_COMMAND_BYTES, MAX_TILE_LABEL_BYTES,
};
pub use port::{LaunchpadStateError, LaunchpadStateStore};
pub use workspace::LaunchpadWorkspace;

use crate::app::WorkspaceId;

pub const ID: WorkspaceId = WorkspaceId::new("launchpad");
