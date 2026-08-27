mod domain;
mod port;
mod workspace;

pub use domain::{
    PortfolioAccountId, PortfolioCurrencyTotal, PortfolioSnapshot, Position, PositionQuantity,
    format_money,
};
pub use port::{PortfolioError, PortfolioImportStateStore, PortfolioRepository};
pub use workspace::PortfolioWorkspace;

use crate::app::WorkspaceId;

pub const ID: WorkspaceId = WorkspaceId::new("portfolio");
