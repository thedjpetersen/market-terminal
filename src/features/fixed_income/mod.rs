mod domain;
mod workspace;

pub use domain::{
    analyze_bond, solve_yield_bps, BondAnalytics, BondCashFlow, BondModelError, BondModelInput,
    CouponFrequency, YieldScenario, MODEL_VERSION,
};
pub use workspace::FixedIncomeWorkspace;

use crate::app::WorkspaceId;

pub const ID: WorkspaceId = WorkspaceId::new("fixed_income");
