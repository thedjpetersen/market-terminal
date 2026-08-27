mod domain;
mod port;
mod workspace;

pub use domain::{
    calculate_risk, RiskCalculationError, RiskCurrencyInput, RiskCurrencySummary, RiskInput,
    RiskPositionExposure, RiskPositionInput, RiskSnapshot, SCENARIO_SHOCK_BPS,
};
pub use port::{RiskError, RiskQuery};
pub use workspace::RiskWorkspace;

use crate::app::WorkspaceId;

pub const ID: WorkspaceId = WorkspaceId::new("risk");
