mod domain;
mod port;
mod workspace;

pub use domain::{
    builtin_screen_definitions, evaluate_screen, ClauseEvidence, Comparison, ScreenCatalogState,
    ScreenClause, ScreenDefinition, ScreenEvaluation, ScreenField, ScreenResultRow,
    ScreenSortDirection, UniverseMember, UniverseSnapshot, MAX_SAVED_SCREENS, MAX_SCREEN_CLAUSES,
    MAX_SCREEN_RESULTS, MAX_UNIVERSE_MEMBERS,
};
pub use port::{ScreenStateError, ScreenStateStore, ScreeningError, ScreeningUniverseQuery};
pub use workspace::ScreeningWorkspace;

use crate::app::WorkspaceId;

pub const ID: WorkspaceId = WorkspaceId::new("screening");
