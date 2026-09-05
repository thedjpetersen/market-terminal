use market_terminal_api::API_SCHEMA_VERSION;
use market_terminal_application::APPLICATION_SCHEMA_VERSION;
use market_terminal_engine::{
    backtesting::{run_backtest, BacktestBar, BacktestConfig},
    execute,
    fixed_income::BondModelInput,
    options::OptionModelInput,
    BacktestComparisonRequest, BacktestRunRequest, EngineOperation, EngineOutcome, EngineRequest,
    ENGINE_API_SCHEMA_VERSION,
};
use serde::Serialize;

pub const CONTRACT_SCHEMA_VERSION: u16 = 1;

#[derive(Serialize)]
pub struct ContractFixtureDocument {
    contract_schema_version: u16,
    api_schema_version: u16,
    application_schema_version: u16,
    engine_schema_version: u16,
    cases: Vec<ContractCase>,
}

#[derive(Serialize)]
struct ContractCase {
    operation: &'static str,
    result_type: &'static str,
    request: EngineRequest,
    response: market_terminal_engine::EngineResponse,
}

pub fn fixture_requests() -> Vec<EngineRequest> {
    let bars = fixture_bars();
    let baseline_config = fixture_config(2, 4);
    let candidate_config = fixture_config(3, 4);
    let baseline = run_backtest(
        &baseline_config,
        &bars,
        "contract-fixture".to_owned(),
        "deterministic".to_owned(),
        "fixture-v1".to_owned(),
    )
    .expect("baseline contract artifact");
    let candidate = run_backtest(
        &candidate_config,
        &bars,
        "contract-fixture".to_owned(),
        "deterministic".to_owned(),
        "fixture-v1".to_owned(),
    )
    .expect("candidate contract artifact");

    vec![
        request(
            "contract:run-backtest",
            EngineOperation::RunBacktest(BacktestRunRequest {
                config: baseline_config,
                bars,
                source: "contract-fixture".to_owned(),
                quality: "deterministic".to_owned(),
                input_version: "fixture-v1".to_owned(),
            }),
        ),
        request(
            "contract:compare-backtests",
            EngineOperation::CompareBacktests(BacktestComparisonRequest {
                baseline: Box::new(baseline),
                candidate: Box::new(candidate),
            }),
        ),
        request(
            "contract:price-option",
            EngineOperation::PriceOption(OptionModelInput::default()),
        ),
        request(
            "contract:analyze-bond",
            EngineOperation::AnalyzeBond(BondModelInput::default()),
        ),
        request(
            "contract:option-large-integer",
            EngineOperation::PriceOption(OptionModelInput {
                spot_micros: 1_000_000_000_001,
                strike_micros: 1_000_000,
                days_to_expiry: 0,
                contract_multiplier: 9_999,
                ..OptionModelInput::default()
            }),
        ),
    ]
}

pub fn render_contract_fixture() -> String {
    let cases = fixture_requests()
        .into_iter()
        .map(|request| {
            let operation = request.operation.name();
            let response = execute(request.clone());
            let result_type = match &response.outcome {
                EngineOutcome::Ok { result } => result.name(),
                EngineOutcome::Error { error } => {
                    panic!("valid contract request failed: {:?}", error.code)
                }
            };
            ContractCase {
                operation,
                result_type,
                request,
                response,
            }
        })
        .collect();
    let document = ContractFixtureDocument {
        contract_schema_version: CONTRACT_SCHEMA_VERSION,
        api_schema_version: API_SCHEMA_VERSION,
        application_schema_version: APPLICATION_SCHEMA_VERSION,
        engine_schema_version: ENGINE_API_SCHEMA_VERSION,
        cases,
    };
    let mut rendered = serde_json::to_string_pretty(&document).expect("serialize fixtures");
    rendered.push('\n');
    rendered
}

fn request(request_id: &str, operation: EngineOperation) -> EngineRequest {
    EngineRequest {
        schema_version: ENGINE_API_SCHEMA_VERSION,
        request_id: request_id.to_owned(),
        operation,
    }
}

fn fixture_config(fast_window: usize, slow_window: usize) -> BacktestConfig {
    BacktestConfig {
        instrument_id: "security:contract".to_owned(),
        symbol: "FIXTURE".to_owned(),
        fast_window,
        slow_window,
        execution_cost_bps: 3,
        commission_micros: 250_000,
        initial_cash_micros: 100_000_000_000,
    }
}

fn fixture_bars() -> Vec<BacktestBar> {
    [100, 101, 102, 99, 98, 103, 105, 104]
        .into_iter()
        .enumerate()
        .map(|(index, close)| BacktestBar {
            timestamp: 1_800_000_000 + index as i64 * 86_400,
            open_micros: (close - 1) * 1_000_000,
            high_micros: (close + 2) * 1_000_000,
            low_micros: (close - 2) * 1_000_000,
            close_micros: close * 1_000_000,
            volume: 1_000_000 + index as u64 * 10_000,
        })
        .collect()
}
