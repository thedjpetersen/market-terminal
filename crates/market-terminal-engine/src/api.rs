use serde::{Deserialize, Serialize};

use crate::{
    backtesting::{
        compare_backtests, run_backtest, BacktestArtifact, BacktestBar, BacktestComparison,
        BacktestConfig, BacktestError,
    },
    fixed_income::{analyze_bond, BondAnalytics, BondModelError, BondModelInput},
    options::{price_option, OptionAnalytics, OptionModelError, OptionModelInput},
};

pub const ENGINE_API_SCHEMA_VERSION: u16 = 1;
const MAX_REQUEST_ID_BYTES: usize = 128;
const MAX_PROVENANCE_BYTES: usize = 1_024;

/// A transport-neutral command envelope suitable for HTTP, workers, native
/// hosts, WebAssembly, queues, and deterministic tests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineRequest {
    pub schema_version: u16,
    pub request_id: String,
    #[serde(flatten)]
    pub operation: EngineOperation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", content = "input", rename_all = "snake_case")]
pub enum EngineOperation {
    RunBacktest(BacktestRunRequest),
    CompareBacktests(BacktestComparisonRequest),
    PriceOption(OptionModelInput),
    AnalyzeBond(BondModelInput),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BacktestRunRequest {
    pub config: BacktestConfig,
    pub bars: Vec<BacktestBar>,
    pub source: String,
    pub quality: String,
    pub input_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BacktestComparisonRequest {
    pub baseline: Box<BacktestArtifact>,
    pub candidate: Box<BacktestArtifact>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EngineResponse {
    pub schema_version: u16,
    pub request_id: String,
    #[serde(flatten)]
    pub outcome: EngineOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum EngineOutcome {
    Ok {
        #[serde(flatten)]
        result: Box<EngineResult>,
    },
    Error {
        error: EngineError,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "result_type", content = "data", rename_all = "snake_case")]
pub enum EngineResult {
    Backtest(BacktestArtifact),
    BacktestComparison(BacktestComparison),
    OptionAnalytics(OptionAnalytics),
    BondAnalytics(BondAnalytics),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EngineError {
    pub code: EngineErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineErrorCode {
    UnsupportedSchema,
    InvalidRequestId,
    InvalidProvenance,
    BacktestRejected,
    ComparisonRejected,
    OptionModelRejected,
    BondModelRejected,
}

/// Execute one deterministic engine command without consulting a clock,
/// filesystem, network, environment variable, terminal, or global state.
///
/// Hosts must still impose a transport body limit before deserializing an
/// untrusted request. The engine then applies its domain-specific collection
/// and numeric bounds.
pub fn execute(request: EngineRequest) -> EngineResponse {
    let request_id = request.request_id;
    let outcome = validate_envelope(request.schema_version, &request_id)
        .and_then(|()| execute_operation(request.operation))
        .map_or_else(
            |error| EngineOutcome::Error { error },
            |result| EngineOutcome::Ok {
                result: Box::new(result),
            },
        );
    EngineResponse {
        schema_version: ENGINE_API_SCHEMA_VERSION,
        request_id,
        outcome,
    }
}

fn validate_envelope(schema_version: u16, request_id: &str) -> Result<(), EngineError> {
    if schema_version != ENGINE_API_SCHEMA_VERSION {
        return Err(EngineError {
            code: EngineErrorCode::UnsupportedSchema,
            message: format!(
                "unsupported engine API schema {schema_version}; expected {ENGINE_API_SCHEMA_VERSION}"
            ),
        });
    }
    if request_id.is_empty()
        || request_id.len() > MAX_REQUEST_ID_BYTES
        || !request_id
            .chars()
            .all(|value| value.is_ascii_alphanumeric() || matches!(value, '-' | '_' | '.' | ':'))
    {
        return Err(EngineError {
            code: EngineErrorCode::InvalidRequestId,
            message: "request_id must be 1-128 ASCII identity characters".to_owned(),
        });
    }
    Ok(())
}

fn execute_operation(operation: EngineOperation) -> Result<EngineResult, EngineError> {
    match operation {
        EngineOperation::RunBacktest(BacktestRunRequest {
            config,
            bars,
            source,
            quality,
            input_version,
        }) => {
            validate_provenance(&source, &quality, &input_version)?;
            run_backtest(&config, &bars, source, quality, input_version)
                .map(EngineResult::Backtest)
                .map_err(backtest_error)
        }
        EngineOperation::CompareBacktests(BacktestComparisonRequest {
            baseline,
            candidate,
        }) => compare_backtests(&baseline, &candidate)
            .map(EngineResult::BacktestComparison)
            .map_err(comparison_error),
        EngineOperation::PriceOption(input) => price_option(&input)
            .map(EngineResult::OptionAnalytics)
            .map_err(option_error),
        EngineOperation::AnalyzeBond(input) => analyze_bond(&input)
            .map(EngineResult::BondAnalytics)
            .map_err(bond_error),
    }
}

fn validate_provenance(source: &str, quality: &str, version: &str) -> Result<(), EngineError> {
    if [source, quality, version]
        .iter()
        .any(|value| value.trim().is_empty() || value.len() > MAX_PROVENANCE_BYTES)
    {
        return Err(EngineError {
            code: EngineErrorCode::InvalidProvenance,
            message: "source, quality, and input_version must be non-empty and at most 1024 bytes"
                .to_owned(),
        });
    }
    Ok(())
}

fn backtest_error(error: BacktestError) -> EngineError {
    EngineError {
        code: EngineErrorCode::BacktestRejected,
        message: error.to_string(),
    }
}

fn comparison_error(error: BacktestError) -> EngineError {
    EngineError {
        code: EngineErrorCode::ComparisonRejected,
        message: error.to_string(),
    }
}

fn option_error(error: OptionModelError) -> EngineError {
    EngineError {
        code: EngineErrorCode::OptionModelRejected,
        message: error.to_string(),
    }
}

fn bond_error(error: BondModelError) -> EngineError {
    EngineError {
        code: EngineErrorCode::BondModelRejected,
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{fixed_income::CouponFrequency, options::OptionRight};

    #[test]
    fn request_contract_round_trips_and_response_is_transport_ready() {
        let request = EngineRequest {
            schema_version: ENGINE_API_SCHEMA_VERSION,
            request_id: "web:option:42".to_owned(),
            operation: EngineOperation::PriceOption(OptionModelInput {
                symbol: "AAPL".to_owned(),
                right: OptionRight::Call,
                spot_micros: 190_000_000,
                strike_micros: 200_000_000,
                days_to_expiry: 30,
                volatility_bps: 2_500,
                risk_free_rate_bps: 500,
                dividend_yield_bps: 0,
                contract_multiplier: 100,
            }),
        };
        let encoded = serde_json::to_string(&request).expect("serialize request");
        let decoded = serde_json::from_str(&encoded).expect("deserialize request");
        assert_eq!(decoded, request);

        let response = execute(decoded);
        let encoded = serde_json::to_string(&response).expect("serialize response");
        assert!(encoded.contains("\"status\":\"ok\""));
        assert!(encoded.contains("\"result_type\":\"option_analytics\""));
        assert!(encoded.contains("BLACK-SCHOLES-EUROPEAN-V1"));
    }

    #[test]
    fn unsupported_version_and_invalid_identity_fail_as_typed_responses() {
        let response = execute(EngineRequest {
            schema_version: ENGINE_API_SCHEMA_VERSION + 1,
            request_id: "web:bond:1".to_owned(),
            operation: EngineOperation::AnalyzeBond(BondModelInput::default()),
        });
        assert!(matches!(
            response.outcome,
            EngineOutcome::Error {
                error: EngineError {
                    code: EngineErrorCode::UnsupportedSchema,
                    ..
                }
            }
        ));

        let response = execute(EngineRequest {
            schema_version: ENGINE_API_SCHEMA_VERSION,
            request_id: "not allowed/identity".to_owned(),
            operation: EngineOperation::PriceOption(OptionModelInput::default()),
        });
        assert!(matches!(
            response.outcome,
            EngineOutcome::Error {
                error: EngineError {
                    code: EngineErrorCode::InvalidRequestId,
                    ..
                }
            }
        ));
    }

    #[test]
    fn invalid_domain_input_is_not_partially_executed() {
        let response = execute(EngineRequest {
            schema_version: ENGINE_API_SCHEMA_VERSION,
            request_id: "web:bond:invalid".to_owned(),
            operation: EngineOperation::AnalyzeBond(BondModelInput {
                frequency: CouponFrequency::SemiAnnual,
                face_micros: 0,
                ..BondModelInput::default()
            }),
        });
        assert!(matches!(
            response.outcome,
            EngineOutcome::Error {
                error: EngineError {
                    code: EngineErrorCode::BondModelRejected,
                    ..
                }
            }
        ));
    }

    #[test]
    fn provenance_is_bounded_before_backtest_execution() {
        let response = execute(EngineRequest {
            schema_version: ENGINE_API_SCHEMA_VERSION,
            request_id: "web:backtest:1".to_owned(),
            operation: EngineOperation::RunBacktest(BacktestRunRequest {
                config: BacktestConfig::moving_average_cross("us:xnas:aapl", "AAPL"),
                bars: Vec::new(),
                source: " ".to_owned(),
                quality: "delayed".to_owned(),
                input_version: "v1".to_owned(),
            }),
        });
        assert!(matches!(
            response.outcome,
            EngineOutcome::Error {
                error: EngineError {
                    code: EngineErrorCode::InvalidProvenance,
                    ..
                }
            }
        ));
    }

    #[test]
    fn backtest_and_comparison_dispatch_use_the_same_typed_artifacts() {
        let bars = [10, 10, 11, 12, 13, 14]
            .into_iter()
            .enumerate()
            .map(|(index, close)| BacktestBar {
                timestamp: 1_700_000_000 + index as i64 * 86_400,
                open_micros: close * 1_000_000,
                high_micros: close * 1_000_000 + 100_000,
                low_micros: close * 1_000_000 - 100_000,
                close_micros: close * 1_000_000,
                volume: 1_000_000,
            })
            .collect::<Vec<_>>();
        let mut baseline_config = BacktestConfig::moving_average_cross("us:xnas:aapl", "AAPL");
        baseline_config.fast_window = 2;
        baseline_config.slow_window = 3;
        baseline_config.execution_cost_bps = 0;
        baseline_config.commission_micros = 0;
        let mut candidate_config = baseline_config.clone();
        candidate_config.execution_cost_bps = 25;

        let run = |request_id: &str, config| {
            execute(EngineRequest {
                schema_version: ENGINE_API_SCHEMA_VERSION,
                request_id: request_id.to_owned(),
                operation: EngineOperation::RunBacktest(BacktestRunRequest {
                    config,
                    bars: bars.clone(),
                    source: "fixture".to_owned(),
                    quality: "replay".to_owned(),
                    input_version: "v1".to_owned(),
                }),
            })
        };
        let artifact = |response: EngineResponse| match response.outcome {
            EngineOutcome::Ok { result } => match *result {
                EngineResult::Backtest(artifact) => artifact,
                other => panic!("unexpected engine result: {other:?}"),
            },
            EngineOutcome::Error { error } => panic!("unexpected engine error: {error:?}"),
        };
        let baseline = artifact(run("web:backtest:baseline", baseline_config));
        let candidate = artifact(run("web:backtest:candidate", candidate_config));

        let response = execute(EngineRequest {
            schema_version: ENGINE_API_SCHEMA_VERSION,
            request_id: "web:backtest:compare".to_owned(),
            operation: EngineOperation::CompareBacktests(BacktestComparisonRequest {
                baseline: Box::new(baseline),
                candidate: Box::new(candidate),
            }),
        });
        assert!(matches!(
            response.outcome,
            EngineOutcome::Ok { result }
                if matches!(*result, EngineResult::BacktestComparison(_))
        ));
    }
}
