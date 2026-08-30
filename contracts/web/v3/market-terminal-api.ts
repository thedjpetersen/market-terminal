/**
 * Market Terminal web contract: API v3 / application v2 / engine v1.
 *
 * Generated fixtures in this directory are the executable wire examples.
 * Rust CI verifies that every compiler-visible operation and result has an
 * exact replay and that every discriminator below remains present.
 *
 * Integer fields are JSON numbers. Consumers must reject values outside
 * Number.isSafeInteger instead of rounding analytical evidence silently.
 */

export type JsonInteger = number;

export interface HealthResponse {
  readonly status: "ok";
  readonly api_schema_version: 3;
  readonly application_schema_version: 2;
  readonly engine_schema_version: 1;
}

export type EngineOperationName =
  | "run_backtest"
  | "compare_backtests"
  | "price_option"
  | "analyze_bond";

export type ResearchArtifactOperation = "read_research_artifacts";

export interface CapabilityResponse {
  readonly api_schema_version: 3;
  readonly application_schema_version: 2;
  readonly engine_schema_version: 1;
  readonly tenant_id: string;
  readonly principal_id: string;
  readonly operations: readonly EngineOperationName[];
  readonly artifact_operations: readonly ResearchArtifactOperation[];
  readonly max_body_bytes: JsonInteger;
  readonly max_backtest_bars: JsonInteger;
  readonly max_comparison_points: JsonInteger;
  readonly requests_per_minute: JsonInteger;
  readonly burst_requests: JsonInteger;
  readonly engine_deadline_millis: JsonInteger;
  readonly artifact_deadline_millis: JsonInteger;
  readonly max_engine_in_flight: JsonInteger;
  readonly max_artifact_in_flight: JsonInteger;
}

export interface ProblemResponse {
  readonly code:
    | "unauthorized"
    | "authentication_unavailable"
    | "admission_unavailable"
    | "rate_limit_exceeded"
    | "concurrency_limit_exceeded"
    | "request_deadline_exceeded"
    | "execution_unavailable"
    | "invalid_json"
    | "unsupported_media_type"
    | "payload_too_large"
    | "capability_denied"
    | "workload_budget_exceeded"
    | "invalid_artifact_request"
    | "artifact_not_found"
    | "not_found"
    | "artifact_contract_violation"
    | "artifact_service_unavailable";
  readonly message: string;
}

export interface BacktestBar {
  readonly timestamp: JsonInteger;
  readonly open_micros: JsonInteger;
  readonly high_micros: JsonInteger;
  readonly low_micros: JsonInteger;
  readonly close_micros: JsonInteger;
  readonly volume: JsonInteger;
}

export interface BacktestConfig {
  readonly instrument_id: string;
  readonly symbol: string;
  readonly fast_window: JsonInteger;
  readonly slow_window: JsonInteger;
  readonly execution_cost_bps: JsonInteger;
  readonly commission_micros: JsonInteger;
  readonly initial_cash_micros: JsonInteger;
}

export interface BacktestRunInput {
  readonly config: BacktestConfig;
  readonly bars: readonly BacktestBar[];
  readonly source: string;
  readonly quality: string;
  readonly input_version: string;
}

export type TradeSide = "Buy" | "Sell";

export interface BacktestTrade {
  readonly side: TradeSide;
  readonly signal_timestamp: JsonInteger;
  readonly execution_timestamp: JsonInteger;
  readonly quantity: JsonInteger;
  readonly reference_price_micros: JsonInteger;
  readonly execution_price_micros: JsonInteger;
  readonly commission_micros: JsonInteger;
}

export interface BacktestDecision {
  readonly observed_at: JsonInteger;
  readonly executes_at: JsonInteger;
  readonly target_long: boolean;
}

export interface EquityPoint {
  readonly timestamp: JsonInteger;
  readonly equity_micros: JsonInteger;
}

export interface BacktestArtifact {
  readonly schema_version: 1;
  readonly config: BacktestConfig;
  readonly strategy: string;
  readonly instrument_id: string;
  readonly symbol: string;
  readonly source: string;
  readonly quality: string;
  readonly input_version: string;
  readonly config_digest: string;
  readonly data_digest: string;
  readonly run_digest: string;
  readonly artifact_digest: string;
  readonly bars: JsonInteger;
  readonly first_timestamp: JsonInteger;
  readonly last_timestamp: JsonInteger;
  readonly initial_cash_micros: JsonInteger;
  readonly final_equity_micros: JsonInteger;
  readonly total_return_bps: JsonInteger;
  readonly max_drawdown_bps: JsonInteger;
  readonly turnover_bps: JsonInteger;
  readonly decisions: readonly BacktestDecision[];
  readonly trades: readonly BacktestTrade[];
  readonly equity: readonly EquityPoint[];
  readonly open_quantity: JsonInteger;
  readonly methodology: string;
  readonly disclosures: readonly string[];
}

export interface BacktestComparisonSide {
  readonly run_digest: string;
  readonly artifact_digest: string;
  readonly config_digest: string;
  readonly fast_window: JsonInteger;
  readonly slow_window: JsonInteger;
  readonly execution_cost_bps: JsonInteger;
  readonly commission_micros: JsonInteger;
  readonly final_equity_micros: JsonInteger;
  readonly total_return_bps: JsonInteger;
  readonly max_drawdown_bps: JsonInteger;
  readonly turnover_bps: JsonInteger;
  readonly trades: JsonInteger;
}

export interface BacktestComparison {
  readonly schema_version: 1;
  readonly instrument_id: string;
  readonly symbol: string;
  readonly source: string;
  readonly quality: string;
  readonly input_version: string;
  readonly data_digest: string;
  readonly bars: JsonInteger;
  readonly first_timestamp: JsonInteger;
  readonly last_timestamp: JsonInteger;
  readonly initial_cash_micros: JsonInteger;
  readonly baseline: BacktestComparisonSide;
  readonly candidate: BacktestComparisonSide;
  readonly changed_parameters: readonly string[];
  readonly final_equity_delta_micros: JsonInteger;
  readonly total_return_delta_bps: JsonInteger;
  readonly max_drawdown_delta_bps: JsonInteger;
  readonly turnover_delta_bps: JsonInteger;
  readonly trade_count_delta: JsonInteger;
  readonly comparison_digest: string;
  readonly methodology: string;
  readonly disclosure: string;
}

export type OptionRight = "call" | "put";

export interface OptionModelInput {
  readonly symbol: string;
  readonly right: OptionRight;
  readonly spot_micros: JsonInteger;
  readonly strike_micros: JsonInteger;
  readonly days_to_expiry: JsonInteger;
  readonly volatility_bps: JsonInteger;
  readonly risk_free_rate_bps: JsonInteger;
  readonly dividend_yield_bps: JsonInteger;
  readonly contract_multiplier: JsonInteger;
}

export interface OptionScenario {
  readonly spot_shock_bps: JsonInteger;
  readonly volatility_shift_bps: JsonInteger;
  readonly spot_micros: JsonInteger;
  readonly volatility_bps: JsonInteger;
  readonly price_micros: JsonInteger;
  readonly contract_value_micros: JsonInteger;
}

export interface OptionAnalytics {
  readonly input: OptionModelInput;
  readonly price_micros: JsonInteger;
  readonly intrinsic_micros: JsonInteger;
  readonly time_value_micros: JsonInteger;
  readonly delta_millionths: JsonInteger;
  readonly gamma_billionths: JsonInteger;
  readonly vega_micros_per_point: JsonInteger;
  readonly theta_micros_per_day: JsonInteger;
  readonly rho_micros_per_point: JsonInteger;
  readonly scenarios: readonly OptionScenario[];
  readonly model_version: string;
  readonly input_digest: string;
  readonly methodology: string;
  readonly disclosures: readonly [string, string, string, string];
}

export type CouponFrequency = "annual" | "semi_annual" | "quarterly";

export interface BondModelInput {
  readonly instrument_id: string;
  readonly currency: string;
  readonly face_micros: JsonInteger;
  readonly coupon_bps: JsonInteger;
  readonly yield_bps: JsonInteger;
  readonly years_to_maturity: JsonInteger;
  readonly frequency: CouponFrequency;
  readonly accrued_period_bps: JsonInteger;
}

export interface BondCashFlow {
  readonly ordinal: JsonInteger;
  readonly time_years_millionths: JsonInteger;
  readonly coupon_micros: JsonInteger;
  readonly principal_micros: JsonInteger;
  readonly total_micros: JsonInteger;
  readonly present_value_micros: JsonInteger;
}

export interface YieldScenario {
  readonly shock_bps: JsonInteger;
  readonly yield_bps: JsonInteger;
  readonly clean_price_micros: JsonInteger;
  readonly dirty_price_micros: JsonInteger;
  readonly clean_change_micros: JsonInteger;
  readonly clean_change_bps: JsonInteger;
}

export interface BondAnalytics {
  readonly input: BondModelInput;
  readonly clean_price_micros: JsonInteger;
  readonly dirty_price_micros: JsonInteger;
  readonly accrued_interest_micros: JsonInteger;
  readonly coupon_payment_micros: JsonInteger;
  readonly current_yield_bps: JsonInteger;
  readonly macaulay_duration_years_millionths: JsonInteger;
  readonly modified_duration_years_millionths: JsonInteger;
  readonly convexity_years2_millionths: JsonInteger;
  readonly dv01_micros: JsonInteger;
  readonly cash_flows: readonly BondCashFlow[];
  readonly scenarios: readonly YieldScenario[];
  readonly model_version: string;
  readonly input_digest: string;
  readonly methodology: string;
  readonly disclosures: readonly [string, string, string, string, string];
}

type EngineEnvelope = {
  readonly schema_version: 1;
  readonly request_id: string;
};

export type EngineRequest = EngineEnvelope &
  (
    | { readonly operation: "run_backtest"; readonly input: BacktestRunInput }
    | {
        readonly operation: "compare_backtests";
        readonly input: {
          readonly baseline: BacktestArtifact;
          readonly candidate: BacktestArtifact;
        };
      }
    | { readonly operation: "price_option"; readonly input: OptionModelInput }
    | { readonly operation: "analyze_bond"; readonly input: BondModelInput }
  );

export type EngineResult =
  | { readonly result_type: "backtest"; readonly data: BacktestArtifact }
  | {
      readonly result_type: "backtest_comparison";
      readonly data: BacktestComparison;
    }
  | { readonly result_type: "option_analytics"; readonly data: OptionAnalytics }
  | { readonly result_type: "bond_analytics"; readonly data: BondAnalytics };

export type EngineErrorCode =
  | "unsupported_schema"
  | "invalid_request_id"
  | "invalid_provenance"
  | "backtest_rejected"
  | "comparison_rejected"
  | "option_model_rejected"
  | "bond_model_rejected";

export type EngineResponse = EngineEnvelope &
  (
    | ({ readonly status: "ok" } & EngineResult)
    | {
        readonly status: "error";
        readonly error: { readonly code: EngineErrorCode; readonly message: string };
      }
  );

export type ResearchArtifactKind =
  | "backtest_run"
  | "backtest_comparison"
  | "screen_result"
  | "news_snapshot"
  | "security_research";

export interface ResearchArtifactSummary {
  readonly schema_version: 1;
  readonly tenant_id: string;
  readonly artifact_id: string;
  readonly kind: ResearchArtifactKind;
  readonly title: string;
  readonly created_at_epoch_ms: JsonInteger;
  readonly input_version: string;
  readonly source: string;
  readonly quality: string;
  readonly content_digest: string;
}

export interface ResearchArtifactDocument extends ResearchArtifactSummary {
  readonly content: unknown;
}

export interface ResearchArtifactPage {
  readonly schema_version: 1;
  readonly items: readonly ResearchArtifactSummary[];
  readonly next_cursor: string | null;
}
