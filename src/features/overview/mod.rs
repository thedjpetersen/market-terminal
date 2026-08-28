mod controls;
mod domain;
mod port;
mod workspace;

pub use domain::{LiveOverviewSnapshot, OverviewHeadline, OverviewHolding, OverviewSnapshot};
pub use port::OverviewQuery;
pub use workspace::OverviewWorkspace;

use crate::app::WorkspaceId;

pub const ID: WorkspaceId = WorkspaceId::new("overview");
