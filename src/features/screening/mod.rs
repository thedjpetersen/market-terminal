mod domain;
mod port;
mod workspace;

pub use domain::{
    builtin_screen_definitions, evaluate_screen, universe_content_digest, ClauseEvidence,
    Comparison, ScreenCatalogState, ScreenClause, ScreenDefinition, ScreenEvaluation, ScreenField,
    ScreenResultRow, ScreenSortDirection, UniverseHistoryEntry, UniverseHistoryManifest,
    UniverseMember, UniverseSnapshot, MAX_SAVED_SCREENS, MAX_SCREEN_CLAUSES, MAX_SCREEN_RESULTS,
    MAX_UNIVERSE_HISTORY, MAX_UNIVERSE_MEMBERS,
};
pub use port::{
    ScreenStateError, ScreenStateStore, ScreeningError, ScreeningUniverseQuery,
    UniverseHistoryStore,
};
pub use workspace::ScreeningWorkspace;

use crate::app::WorkspaceId;

pub const ID: WorkspaceId = WorkspaceId::new("screening");
