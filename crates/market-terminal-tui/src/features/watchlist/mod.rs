mod domain;
mod port;
mod workspace;

pub use domain::{
    MonitorColumn, SortDirection, SortField, SortSpec, WatchlistDefinition, WatchlistItem,
};
pub use port::WatchlistCatalog;
pub use workspace::WatchlistWorkspace;

use crate::app::WorkspaceId;

pub const ID: WorkspaceId = WorkspaceId::new("watchlist");
