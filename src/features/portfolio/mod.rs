mod domain;
mod port;
mod workspace;

pub use domain::{PortfolioSnapshot, Position};
pub use port::PortfolioQuery;
pub use workspace::PortfolioWorkspace;

use crate::app::WorkspaceId;

pub const ID: WorkspaceId = WorkspaceId::new("portfolio");
