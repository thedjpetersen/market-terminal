mod domain;
mod port;
mod workspace;

pub use domain::{
    Estimate, Filing, OwnerPosition, PeerComparison, ResearchView, SecurityIdentity,
    SecurityResearch, SecuritySnapshot,
};
pub use port::SecurityQuery;
pub use workspace::SecurityWorkspace;

use crate::app::WorkspaceId;

pub const ID: WorkspaceId = WorkspaceId::new("security");
