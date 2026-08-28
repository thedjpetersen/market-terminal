mod domain;
mod port;
mod workspace;

pub use domain::{
    format_money, PortfolioAccountId, PortfolioActivityCurrencyTotal, PortfolioActivityEntry,
    PortfolioActivityKind, PortfolioActivityLedger, PortfolioCurrencyTotal,
    PortfolioPerformanceSeries, PortfolioPerformanceSnapshot, PortfolioSnapshot, PortfolioTaxLot,
    PortfolioTaxLotCurrencyTotal, PortfolioTaxLotSnapshot, PortfolioValuationPoint, Position,
    PositionQuantity, TaxLotHoldingPeriod,
};
pub use port::{PortfolioError, PortfolioImportStateStore, PortfolioRepository};
pub use workspace::PortfolioWorkspace;

use crate::app::WorkspaceId;

pub const ID: WorkspaceId = WorkspaceId::new("portfolio");
