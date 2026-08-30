mod domain;
mod historical;
mod port;
mod workspace;

pub use domain::{
    calculate_risk, RiskCalculationError, RiskCurrencyInput, RiskCurrencySummary, RiskInput,
    RiskPositionExposure, RiskPositionInput, RiskSnapshot, SCENARIO_SHOCK_BPS,
};
pub use historical::{
    calculate_historical_risk, HistoricalRiskError, HistoricalRiskInput, HistoricalRiskPointInput,
    HistoricalRiskSeriesInput, HistoricalRiskSnapshot, HistoricalRiskSummary,
};
pub use port::{RiskError, RiskQuery};
pub use workspace::RiskWorkspace;

use crate::app::WorkspaceId;

pub const ID: WorkspaceId = WorkspaceId::new("risk");
