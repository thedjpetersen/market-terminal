mod attribution;
mod contribution;
mod domain;
mod port;
mod workspace;

pub use attribution::{
    calculate_multi_period_attribution, PortfolioAttributionCurrencyTotal,
    PortfolioAttributionError, PortfolioAttributionInput, PortfolioAttributionRow,
    PortfolioAttributionSnapshot,
};
pub use contribution::{
    calculate_contribution, PortfolioContributionCurrencyTotal, PortfolioContributionError,
    PortfolioContributionInput, PortfolioContributionInputRow, PortfolioContributionRow,
    PortfolioContributionSnapshot,
};
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
