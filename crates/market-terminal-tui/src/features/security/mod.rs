mod domain;
mod insider_chart;
mod port;
mod workspace;

pub use domain::{
    Estimate, Filing, FinancialPeriod, InsiderTransaction, OwnerPosition, PeerComparison,
    ResearchView, SecurityIdentity, SecurityPage, SecurityResearch, SecuritySnapshot,
};
pub use port::{SecurityDocumentOpenError, SecurityDocumentOpener, SecurityError, SecurityQuery};
pub use workspace::SecurityWorkspace;

use crate::app::WorkspaceId;

pub const ID: WorkspaceId = WorkspaceId::new("security");
