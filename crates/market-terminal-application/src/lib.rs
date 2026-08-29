//! Host-neutral application services around `market-terminal-engine`.
//!
//! This layer owns authenticated actor context, capability authorization, and
//! per-request workload budgets. HTTP, workers, MCP servers, and future hosts
//! call this service instead of invoking the deterministic engine directly.
//! It deliberately owns no transport, clock, filesystem, network, secret, or
//! provider behavior.

use std::fmt;

use serde::{Deserialize, Serialize};

pub use market_terminal_engine::{
    EngineErrorCode, EngineOperation, EngineOutcome, EngineRequest, EngineResponse,
    ENGINE_API_SCHEMA_VERSION,
};

pub const APPLICATION_SCHEMA_VERSION: u16 = 1;
pub const MAX_IDENTITY_BYTES: usize = 64;
pub const DEFAULT_MAX_BACKTEST_BARS: usize = 20_000;
pub const MAX_BACKTEST_BARS: usize = 20_000;
pub const DEFAULT_MAX_COMPARISON_POINTS: usize = 120_000;
pub const MAX_COMPARISON_POINTS: usize = 120_000;

macro_rules! identity_type {
    ($name:ident, $label:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, ApplicationConfigError> {
                let value = value.into();
                validate_identity(&value).map_err(|()| {
                    ApplicationConfigError::InvalidIdentity {
                        kind: $label,
                        value: value.clone(),
                    }
                })?;
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

identity_type!(TenantId, "tenant");
identity_type!(PrincipalId, "principal");

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineCapability {
    RunBacktest,
    CompareBacktests,
    PriceOption,
    AnalyzeBond,
}

impl EngineCapability {
    pub const fn name(self) -> &'static str {
        match self {
            Self::RunBacktest => "run_backtest",
            Self::CompareBacktests => "compare_backtests",
            Self::PriceOption => "price_option",
            Self::AnalyzeBond => "analyze_bond",
        }
    }

    pub const fn for_operation(operation: &EngineOperation) -> Self {
        match operation {
            EngineOperation::RunBacktest(_) => Self::RunBacktest,
            EngineOperation::CompareBacktests(_) => Self::CompareBacktests,
            EngineOperation::PriceOption(_) => Self::PriceOption,
            EngineOperation::AnalyzeBond(_) => Self::AnalyzeBond,
        }
    }

    fn parse(value: &str) -> Result<Self, ApplicationConfigError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "run_backtest" => Ok(Self::RunBacktest),
            "compare_backtests" => Ok(Self::CompareBacktests),
            "price_option" => Ok(Self::PriceOption),
            "analyze_bond" => Ok(Self::AnalyzeBond),
            _ => Err(ApplicationConfigError::UnknownCapability(
                value.trim().to_owned(),
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilitySet {
    run_backtest: bool,
    compare_backtests: bool,
    price_option: bool,
    analyze_bond: bool,
}

impl CapabilitySet {
    pub const fn all() -> Self {
        Self {
            run_backtest: true,
            compare_backtests: true,
            price_option: true,
            analyze_bond: true,
        }
    }

    pub const fn none() -> Self {
        Self {
            run_backtest: false,
            compare_backtests: false,
            price_option: false,
            analyze_bond: false,
        }
    }

    pub fn from_names<'a>(
        names: impl IntoIterator<Item = &'a str>,
    ) -> Result<Self, ApplicationConfigError> {
        let mut capabilities = Self::none();
        let mut count = 0_usize;
        for name in names {
            capabilities.insert(EngineCapability::parse(name)?);
            count += 1;
        }
        if count == 0 {
            return Err(ApplicationConfigError::EmptyCapabilitySet);
        }
        Ok(capabilities)
    }

    pub const fn allows(self, capability: EngineCapability) -> bool {
        match capability {
            EngineCapability::RunBacktest => self.run_backtest,
            EngineCapability::CompareBacktests => self.compare_backtests,
            EngineCapability::PriceOption => self.price_option,
            EngineCapability::AnalyzeBond => self.analyze_bond,
        }
    }

    pub fn allowed_names(self) -> Vec<&'static str> {
        [
            EngineCapability::RunBacktest,
            EngineCapability::CompareBacktests,
            EngineCapability::PriceOption,
            EngineCapability::AnalyzeBond,
        ]
        .into_iter()
        .filter(|capability| self.allows(*capability))
        .map(EngineCapability::name)
        .collect()
    }

    fn insert(&mut self, capability: EngineCapability) {
        match capability {
            EngineCapability::RunBacktest => self.run_backtest = true,
            EngineCapability::CompareBacktests => self.compare_backtests = true,
            EngineCapability::PriceOption => self.price_option = true,
            EngineCapability::AnalyzeBond => self.analyze_bond = true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionBudget {
    max_backtest_bars: usize,
    max_comparison_points: usize,
}

impl ExecutionBudget {
    pub fn new(
        max_backtest_bars: usize,
        max_comparison_points: usize,
    ) -> Result<Self, ApplicationConfigError> {
        if !(1..=MAX_BACKTEST_BARS).contains(&max_backtest_bars) {
            return Err(ApplicationConfigError::InvalidBacktestBudget(
                max_backtest_bars,
            ));
        }
        if !(1..=MAX_COMPARISON_POINTS).contains(&max_comparison_points) {
            return Err(ApplicationConfigError::InvalidComparisonBudget(
                max_comparison_points,
            ));
        }
        Ok(Self {
            max_backtest_bars,
            max_comparison_points,
        })
    }

    pub const fn max_backtest_bars(self) -> usize {
        self.max_backtest_bars
    }

    pub const fn max_comparison_points(self) -> usize {
        self.max_comparison_points
    }
}

impl Default for ExecutionBudget {
    fn default() -> Self {
        Self {
            max_backtest_bars: DEFAULT_MAX_BACKTEST_BARS,
            max_comparison_points: DEFAULT_MAX_COMPARISON_POINTS,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionContext {
    tenant_id: TenantId,
    principal_id: PrincipalId,
    capabilities: CapabilitySet,
    budget: ExecutionBudget,
}

impl ExecutionContext {
    pub const fn new(
        tenant_id: TenantId,
        principal_id: PrincipalId,
        capabilities: CapabilitySet,
        budget: ExecutionBudget,
    ) -> Self {
        Self {
            tenant_id,
            principal_id,
            capabilities,
            budget,
        }
    }

    pub const fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub const fn principal_id(&self) -> &PrincipalId {
        &self.principal_id
    }

    pub const fn capabilities(&self) -> CapabilitySet {
        self.capabilities
    }

    pub const fn budget(&self) -> ExecutionBudget {
        self.budget
    }

    pub const fn with_capabilities(mut self, capabilities: CapabilitySet) -> Self {
        self.capabilities = capabilities;
        self
    }

    pub const fn with_budget(mut self, budget: ExecutionBudget) -> Self {
        self.budget = budget;
        self
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct AnalyticalApplicationService;

impl AnalyticalApplicationService {
    pub fn execute(
        self,
        context: &ExecutionContext,
        request: EngineRequest,
    ) -> Result<EngineResponse, ApplicationError> {
        let capability = EngineCapability::for_operation(&request.operation);
        if !context.capabilities.allows(capability) {
            return Err(ApplicationError {
                code: ApplicationErrorCode::CapabilityDenied,
                message: format!(
                    "principal {} in tenant {} cannot execute {}",
                    context.principal_id,
                    context.tenant_id,
                    capability.name()
                ),
            });
        }
        enforce_budget(context.budget, &request.operation)?;
        Ok(market_terminal_engine::execute(request))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationErrorCode {
    CapabilityDenied,
    WorkloadBudgetExceeded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ApplicationError {
    pub code: ApplicationErrorCode,
    pub message: String,
}

impl fmt::Display for ApplicationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ApplicationError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplicationConfigError {
    InvalidIdentity { kind: &'static str, value: String },
    EmptyCapabilitySet,
    UnknownCapability(String),
    InvalidBacktestBudget(usize),
    InvalidComparisonBudget(usize),
}

impl fmt::Display for ApplicationConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidIdentity { kind, value } => write!(
                formatter,
                "invalid {kind} identity {value:?}; use 1-{MAX_IDENTITY_BYTES} ASCII identity characters"
            ),
            Self::EmptyCapabilitySet => write!(formatter, "at least one capability is required"),
            Self::UnknownCapability(value) => write!(formatter, "unknown capability {value}"),
            Self::InvalidBacktestBudget(value) => write!(
                formatter,
                "backtest bar budget {value} must be between 1 and {MAX_BACKTEST_BARS}"
            ),
            Self::InvalidComparisonBudget(value) => write!(
                formatter,
                "comparison point budget {value} must be between 1 and {MAX_COMPARISON_POINTS}"
            ),
        }
    }
}

impl std::error::Error for ApplicationConfigError {}

fn validate_identity(value: &str) -> Result<(), ()> {
    if value.is_empty()
        || value.len() > MAX_IDENTITY_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return Err(());
    }
    Ok(())
}

fn enforce_budget(
    budget: ExecutionBudget,
    operation: &EngineOperation,
) -> Result<(), ApplicationError> {
    let (actual, allowed, unit) = match operation {
        EngineOperation::RunBacktest(request) => {
            (request.bars.len(), budget.max_backtest_bars, "bars")
        }
        EngineOperation::CompareBacktests(request) => {
            let points = comparison_points(&request.baseline)
                .saturating_add(comparison_points(&request.candidate));
            (points, budget.max_comparison_points, "comparison points")
        }
        EngineOperation::PriceOption(_) | EngineOperation::AnalyzeBond(_) => return Ok(()),
    };
    if actual > allowed {
        return Err(ApplicationError {
            code: ApplicationErrorCode::WorkloadBudgetExceeded,
            message: format!("request requires {actual} {unit}; principal budget allows {allowed}"),
        });
    }
    Ok(())
}

fn comparison_points(artifact: &market_terminal_engine::backtesting::BacktestArtifact) -> usize {
    artifact
        .decisions
        .len()
        .saturating_add(artifact.trades.len())
        .saturating_add(artifact.equity.len())
}

#[cfg(test)]
mod tests {
    use market_terminal_engine::{
        api::BacktestRunRequest,
        backtesting::{BacktestBar, BacktestConfig},
        options::OptionModelInput,
    };

    use super::*;

    fn context(capabilities: CapabilitySet, budget: ExecutionBudget) -> ExecutionContext {
        ExecutionContext::new(
            TenantId::new("tenant-a").unwrap(),
            PrincipalId::new("researcher-1").unwrap(),
            capabilities,
            budget,
        )
    }

    fn option_request() -> EngineRequest {
        EngineRequest {
            schema_version: ENGINE_API_SCHEMA_VERSION,
            request_id: "application:option:1".to_owned(),
            operation: EngineOperation::PriceOption(OptionModelInput::default()),
        }
    }

    #[test]
    fn identities_and_capability_names_are_bounded_and_exact() {
        assert!(TenantId::new("").is_err());
        assert!(PrincipalId::new("with spaces").is_err());
        assert!(TenantId::new("a".repeat(MAX_IDENTITY_BYTES + 1)).is_err());
        assert!(CapabilitySet::from_names([]).is_err());
        assert!(CapabilitySet::from_names(["unknown"]).is_err());

        let capabilities = CapabilitySet::from_names(["price_option", "analyze_bond"]).unwrap();
        assert_eq!(
            capabilities.allowed_names(),
            vec!["price_option", "analyze_bond"]
        );
    }

    #[test]
    fn denied_capability_never_reaches_domain_execution() {
        let context = context(CapabilitySet::none(), ExecutionBudget::default());
        let mut request = option_request();
        if let EngineOperation::PriceOption(input) = &mut request.operation {
            input.spot_micros = 0;
        }
        let error = AnalyticalApplicationService
            .execute(&context, request)
            .unwrap_err();
        assert_eq!(error.code, ApplicationErrorCode::CapabilityDenied);
    }

    #[test]
    fn principal_budget_rejects_work_before_engine_execution() {
        let budget = ExecutionBudget::new(2, DEFAULT_MAX_COMPARISON_POINTS).unwrap();
        let context = context(CapabilitySet::all(), budget);
        let bars = (0..3)
            .map(|index| BacktestBar {
                timestamp: index,
                open_micros: 100_000_000,
                high_micros: 101_000_000,
                low_micros: 99_000_000,
                close_micros: 100_000_000,
                volume: 1_000,
            })
            .collect();
        let request = EngineRequest {
            schema_version: ENGINE_API_SCHEMA_VERSION,
            request_id: "application:backtest:budget".to_owned(),
            operation: EngineOperation::RunBacktest(BacktestRunRequest {
                config: BacktestConfig::moving_average_cross("us:xnas:aapl", "AAPL"),
                bars,
                source: "fixture".to_owned(),
                quality: "verified".to_owned(),
                input_version: "v1".to_owned(),
            }),
        };
        let error = AnalyticalApplicationService
            .execute(&context, request)
            .unwrap_err();
        assert_eq!(error.code, ApplicationErrorCode::WorkloadBudgetExceeded);
        assert!(error.message.contains("3 bars"));
    }

    #[test]
    fn authorized_execution_preserves_the_engine_contract() {
        let context = context(CapabilitySet::all(), ExecutionBudget::default());
        let response = AnalyticalApplicationService
            .execute(&context, option_request())
            .unwrap();
        assert_eq!(response.request_id, "application:option:1");
        assert!(matches!(response.outcome, EngineOutcome::Ok { .. }));
        assert_eq!(context.tenant_id().as_str(), "tenant-a");
        assert_eq!(context.principal_id().as_str(), "researcher-1");
    }

    #[test]
    fn budget_construction_is_fail_closed() {
        assert!(ExecutionBudget::new(0, 1).is_err());
        assert!(ExecutionBudget::new(MAX_BACKTEST_BARS + 1, 1).is_err());
        assert!(ExecutionBudget::new(1, 0).is_err());
        assert!(ExecutionBudget::new(1, MAX_COMPARISON_POINTS + 1).is_err());
    }
}
