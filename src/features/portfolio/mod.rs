mod domain;
mod port;
mod workspace;

pub use domain::{
    format_money, PortfolioAccountId, PortfolioActivityCurrencyTotal, PortfolioActivityEntry,
    PortfolioActivityKind, PortfolioActivityLedger, PortfolioCurrencyTotal, PortfolioSnapshot,
    Position, PositionQuantity,
};
pub use port::{PortfolioError, PortfolioImportStateStore, PortfolioRepository};
pub use workspace::PortfolioWorkspace;

use crate::app::WorkspaceId;

pub const ID: WorkspaceId = WorkspaceId::new("portfolio");
