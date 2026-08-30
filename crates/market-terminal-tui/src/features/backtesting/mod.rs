mod domain;
mod port;
mod workspace;

pub use domain::{
    compare_backtests, run_backtest, BacktestArtifact, BacktestBar, BacktestComparison,
    BacktestComparisonSide, BacktestConfig, BacktestDecision, BacktestError, BacktestTrade,
    TradeSide, DEFAULT_INITIAL_CASH_MICROS,
};
pub use port::{
    BacktestArtifactError, BacktestArtifactFileStore, BacktestArtifactStore,
    BacktestArtifactSummary, BacktestHistoryError, BacktestHistoryQuery, BacktestHistoryRequest,
    BacktestHistorySnapshot, MAX_BACKTEST_EXPORT_BYTES, MAX_SAVED_BACKTEST_ARTIFACTS,
};
pub use workspace::BacktestWorkspace;

use crate::app::WorkspaceId;

pub const ID: WorkspaceId = WorkspaceId::new("backtesting");
