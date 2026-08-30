mod domain;
mod port;
mod workspace;

pub use domain::{LiveMarketRow, LiveMarketsSnapshot, MarketIndex, MarketsSnapshot};
pub use port::MarketsQuery;
pub use workspace::MarketsWorkspace;

use crate::app::WorkspaceId;

pub const ID: WorkspaceId = WorkspaceId::new("markets");
