mod domain;
mod port;
mod workspace;

pub use domain::{
    run_backtest, BacktestArtifact, BacktestBar, BacktestConfig, BacktestDecision, BacktestError,
    BacktestTrade, TradeSide, DEFAULT_INITIAL_CASH_MICROS,
};
pub use port::{
    BacktestHistoryError, BacktestHistoryQuery, BacktestHistoryRequest, BacktestHistorySnapshot,
};
pub use workspace::BacktestWorkspace;

use crate::app::WorkspaceId;

pub const ID: WorkspaceId = WorkspaceId::new("backtesting");
