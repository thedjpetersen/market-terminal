mod domain;
mod port;
mod workspace;

pub use domain::{
    Estimate, Filing, FinancialPeriod, OwnerPosition, PeerComparison, ResearchView,
    SecurityIdentity, SecurityPage, SecurityResearch, SecuritySnapshot,
};
pub use port::{SecurityError, SecurityQuery};
pub use workspace::SecurityWorkspace;

use crate::app::WorkspaceId;

pub const ID: WorkspaceId = WorkspaceId::new("security");
