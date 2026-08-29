mod domain;
mod port;
mod portable;
mod workspace;

pub use domain::{
    LaunchpadState, LaunchpadTarget, LaunchpadTile, LaunchpadValidationError,
    LAUNCHPAD_SCHEMA_VERSION, MAX_LAUNCHPAD_TILES, MAX_TARGET_ID_BYTES, MAX_TILE_COMMAND_BYTES,
    MAX_TILE_LABEL_BYTES,
};
pub use port::{LaunchpadFileError, LaunchpadFileStore, LaunchpadStateError, LaunchpadStateStore};
pub use portable::{
    LaunchpadDocumentError, LaunchpadImportMode, LaunchpadImportReport,
    MAX_LAUNCHPAD_DOCUMENT_BYTES,
};
pub use workspace::LaunchpadWorkspace;

use crate::app::WorkspaceId;

pub const ID: WorkspaceId = WorkspaceId::new("launchpad");
