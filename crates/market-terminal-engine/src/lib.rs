//! Host-neutral analytical engines for Market Terminal.
//!
//! This crate is intentionally independent of terminal rendering, network
//! clients, async runtimes, filesystems, and application-shell state. Native,
//! web, service, and WebAssembly hosts can all invoke the same versioned,
//! deterministic contracts.

pub mod api;
pub mod backtesting;
pub mod fixed_income;
pub mod options;

pub use api::{
    execute, BacktestComparisonRequest, BacktestRunRequest, EngineError, EngineErrorCode,
    EngineOperation, EngineOutcome, EngineRequest, EngineResponse, EngineResult,
    ENGINE_API_SCHEMA_VERSION,
};
