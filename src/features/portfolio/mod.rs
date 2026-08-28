mod domain;
mod port;
mod workspace;

pub use domain::{
    format_money, ExecutionPrice, PortfolioAccountId, PortfolioActivityCurrencyTotal,
    PortfolioActivityEntry, PortfolioActivityKind, PortfolioActivityLedger, PortfolioClosedLot,
    PortfolioCurrencyTotal, PortfolioPerformanceSeries, PortfolioPerformanceSnapshot,
    PortfolioRealizedGainCurrencyTotal, PortfolioRealizedGainSnapshot, PortfolioSnapshot,
    PortfolioTaxLot, PortfolioTaxLotCurrencyTotal, PortfolioTaxLotSnapshot,
    PortfolioTradeCurrencyTotal, PortfolioTradeExecution, PortfolioTradeLedger,
    PortfolioValuationPoint, Position, PositionQuantity, TaxLotHoldingPeriod, TradeSide,
};
pub use port::{PortfolioError, PortfolioImportStateStore, PortfolioRepository};
pub use workspace::PortfolioWorkspace;

use crate::app::WorkspaceId;

pub const ID: WorkspaceId = WorkspaceId::new("portfolio");
