mod controls;
mod domain;
mod mission;
mod port;
mod workspace;

pub use domain::{
    LiveOverviewSnapshot, OverviewEvent, OverviewHeadline, OverviewHealthState, OverviewHolding,
    OverviewMarketPulse, OverviewPriority, OverviewSavedWork, OverviewSnapshot,
    OverviewSourceHealth,
};
pub use port::OverviewQuery;
pub use workspace::OverviewWorkspace;

use crate::app::WorkspaceId;

pub const ID: WorkspaceId = WorkspaceId::new("overview");
