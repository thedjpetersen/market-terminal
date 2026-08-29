//! Authenticated HTTP transport for `market-terminal-engine`.
//!
//! This crate is a host adapter, not a second analytical implementation. It
//! owns HTTP extraction, authentication, authorization, size limits, status
//! mapping, and response headers; every successful operation is delegated to
//! the deterministic engine crate.

use std::{fmt, sync::Arc, time::Instant};

use axum::{
    extract::{rejection::JsonRejection, DefaultBodyLimit, Request, State},
    http::{header, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use market_terminal_engine::{
    execute, EngineErrorCode, EngineOperation, EngineOutcome, EngineRequest, EngineResponse,
    ENGINE_API_SCHEMA_VERSION,
};
use serde::Serialize;
use tracing::info;

pub const API_SCHEMA_VERSION: u16 = 1;
pub const DEFAULT_MAX_BODY_BYTES: usize = 4 * 1024 * 1024;
pub const MIN_MAX_BODY_BYTES: usize = 1_024;
pub const MAX_MAX_BODY_BYTES: usize = 8 * 1024 * 1024;
const MIN_TOKEN_BYTES: usize = 32;
const MAX_TOKEN_BYTES: usize = 1_024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationPolicy {
    run_backtest: bool,
    compare_backtests: bool,
    price_option: bool,
    analyze_bond: bool,
}

impl OperationPolicy {
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
    ) -> Result<Self, ApiConfigError> {
        let mut policy = Self::none();
        let mut count = 0_usize;
        for name in names {
            let name = name.trim().to_ascii_lowercase();
            match name.as_str() {
                "run_backtest" => policy.run_backtest = true,
                "compare_backtests" => policy.compare_backtests = true,
                "price_option" => policy.price_option = true,
                "analyze_bond" => policy.analyze_bond = true,
                _ => return Err(ApiConfigError::UnknownOperation(name)),
            }
            count += 1;
        }
        if count == 0 {
            return Err(ApiConfigError::EmptyOperationPolicy);
        }
        Ok(policy)
    }

    pub fn allowed_names(self) -> Vec<&'static str> {
        let mut names = Vec::with_capacity(4);
        if self.run_backtest {
            names.push("run_backtest");
        }
        if self.compare_backtests {
            names.push("compare_backtests");
        }
        if self.price_option {
            names.push("price_option");
        }
        if self.analyze_bond {
            names.push("analyze_bond");
        }
        names
    }

    fn allows(self, operation: &EngineOperation) -> bool {
        match operation {
            EngineOperation::RunBacktest(_) => self.run_backtest,
            EngineOperation::CompareBacktests(_) => self.compare_backtests,
            EngineOperation::PriceOption(_) => self.price_option,
            EngineOperation::AnalyzeBond(_) => self.analyze_bond,
        }
    }
}

#[derive(Clone)]
pub struct ApiConfig {
    bearer_token: Arc<str>,
    max_body_bytes: usize,
    operation_policy: OperationPolicy,
}

impl ApiConfig {
    pub fn new(bearer_token: impl Into<String>) -> Result<Self, ApiConfigError> {
        let bearer_token = bearer_token.into();
        validate_token(&bearer_token)?;
        Ok(Self {
            bearer_token: Arc::from(bearer_token),
            max_body_bytes: DEFAULT_MAX_BODY_BYTES,
            operation_policy: OperationPolicy::all(),
        })
    }

    pub fn with_max_body_bytes(mut self, max_body_bytes: usize) -> Result<Self, ApiConfigError> {
        if !(MIN_MAX_BODY_BYTES..=MAX_MAX_BODY_BYTES).contains(&max_body_bytes) {
            return Err(ApiConfigError::InvalidBodyLimit(max_body_bytes));
        }
        self.max_body_bytes = max_body_bytes;
        Ok(self)
    }

    pub fn with_operation_policy(mut self, operation_policy: OperationPolicy) -> Self {
        self.operation_policy = operation_policy;
        self
    }

    pub const fn max_body_bytes(&self) -> usize {
        self.max_body_bytes
    }

    pub const fn operation_policy(&self) -> OperationPolicy {
        self.operation_policy
    }
}

impl fmt::Debug for ApiConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ApiConfig")
            .field("bearer_token", &"[REDACTED]")
            .field("max_body_bytes", &self.max_body_bytes)
            .field("operation_policy", &self.operation_policy)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiConfigError {
    InvalidToken,
    InvalidBodyLimit(usize),
    EmptyOperationPolicy,
    UnknownOperation(String),
}

impl fmt::Display for ApiConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidToken => write!(
                formatter,
                "API token must be 32-1024 visible ASCII characters without whitespace"
            ),
            Self::InvalidBodyLimit(value) => write!(
                formatter,
                "API body limit {value} must be between {MIN_MAX_BODY_BYTES} and {MAX_MAX_BODY_BYTES} bytes"
            ),
            Self::EmptyOperationPolicy => {
                write!(formatter, "at least one engine operation must be allowed")
            }
            Self::UnknownOperation(value) => write!(formatter, "unknown engine operation {value}"),
        }
    }
}

impl std::error::Error for ApiConfigError {}

#[derive(Clone)]
struct ApiState {
    config: ApiConfig,
}

pub fn router(config: ApiConfig) -> Router {
    let state = ApiState { config };
    let protected = Router::new()
        .route("/v1/capabilities", get(capabilities))
        .route("/v1/engine", post(run_engine))
        .route_layer(middleware::from_fn_with_state(state.clone(), authorize))
        .layer(DefaultBodyLimit::max(state.config.max_body_bytes));

    Router::new()
        .route("/healthz", get(health))
        .merge(protected)
        .fallback(not_found)
        .with_state(state)
        .layer(middleware::from_fn(trace_request))
}

async fn health() -> Response {
    secure_response((
        StatusCode::OK,
        Json(HealthResponse {
            status: "ok",
            api_schema_version: API_SCHEMA_VERSION,
            engine_schema_version: ENGINE_API_SCHEMA_VERSION,
        }),
    ))
}

async fn capabilities(State(state): State<ApiState>) -> Response {
    secure_response((
        StatusCode::OK,
        Json(CapabilityResponse {
            api_schema_version: API_SCHEMA_VERSION,
            engine_schema_version: ENGINE_API_SCHEMA_VERSION,
            operations: state.config.operation_policy.allowed_names(),
            max_body_bytes: state.config.max_body_bytes,
        }),
    ))
}

async fn run_engine(
    State(state): State<ApiState>,
    payload: Result<Json<EngineRequest>, JsonRejection>,
) -> Response {
    let request = match payload {
        Ok(Json(request)) => request,
        Err(rejection) => return json_rejection(rejection),
    };
    if !state.config.operation_policy.allows(&request.operation) {
        return problem(
            StatusCode::FORBIDDEN,
            "operation_forbidden",
            "the authenticated deployment policy does not allow this operation",
        );
    }

    let response = execute(request);
    let status = engine_status(&response);
    let request_id = HeaderValue::from_str(&response.request_id).ok();
    let mut response = secure_response((status, Json(response)));
    if let Some(request_id) = request_id {
        response.headers_mut().insert("x-request-id", request_id);
    }
    response
}

async fn authorize(State(state): State<ApiState>, request: Request, next: Next) -> Response {
    let authorized = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|candidate| constant_time_eq(candidate, &state.config.bearer_token));
    if !authorized {
        let mut response = problem(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "a valid bearer token is required",
        );
        response.headers_mut().insert(
            header::WWW_AUTHENTICATE,
            HeaderValue::from_static("Bearer realm=\"market-terminal\""),
        );
        return response;
    }
    secure_response(next.run(request).await)
}

async fn trace_request(request: Request, next: Next) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let started = Instant::now();
    let response = next.run(request).await;
    let request_id = response
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("unavailable");
    info!(
        method = %method,
        path = %path,
        status = response.status().as_u16(),
        elapsed_micros = started.elapsed().as_micros() as u64,
        request_id,
        "market terminal API request"
    );
    response
}

async fn not_found() -> Response {
    problem(
        StatusCode::NOT_FOUND,
        "not_found",
        "the requested API route does not exist",
    )
}

fn engine_status(response: &EngineResponse) -> StatusCode {
    match &response.outcome {
        EngineOutcome::Ok { .. } => StatusCode::OK,
        EngineOutcome::Error { error } => match error.code {
            EngineErrorCode::UnsupportedSchema
            | EngineErrorCode::InvalidRequestId
            | EngineErrorCode::InvalidProvenance => StatusCode::BAD_REQUEST,
            EngineErrorCode::BacktestRejected
            | EngineErrorCode::ComparisonRejected
            | EngineErrorCode::OptionModelRejected
            | EngineErrorCode::BondModelRejected => StatusCode::UNPROCESSABLE_ENTITY,
        },
    }
}

fn json_rejection(rejection: JsonRejection) -> Response {
    let status = match rejection.status() {
        StatusCode::PAYLOAD_TOO_LARGE => StatusCode::PAYLOAD_TOO_LARGE,
        StatusCode::UNSUPPORTED_MEDIA_TYPE => StatusCode::UNSUPPORTED_MEDIA_TYPE,
        _ => StatusCode::BAD_REQUEST,
    };
    let (code, message) = match status {
        StatusCode::PAYLOAD_TOO_LARGE => (
            "payload_too_large",
            "request body exceeds the configured limit",
        ),
        StatusCode::UNSUPPORTED_MEDIA_TYPE => (
            "unsupported_media_type",
            "content-type must be application/json",
        ),
        _ => ("invalid_json", "request body is not a valid engine request"),
    };
    problem(status, code, message)
}

fn problem(status: StatusCode, code: &'static str, message: &'static str) -> Response {
    secure_response((status, Json(ProblemResponse { code, message })))
}

fn secure_response(response: impl IntoResponse) -> Response {
    let mut response = response.into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    response
        .headers_mut()
        .insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    response
}

fn validate_token(token: &str) -> Result<(), ApiConfigError> {
    if !(MIN_TOKEN_BYTES..=MAX_TOKEN_BYTES).contains(&token.len())
        || !token.bytes().all(|value| (0x21..=0x7e).contains(&value))
    {
        return Err(ApiConfigError::InvalidToken);
    }
    Ok(())
}

fn constant_time_eq(candidate: &str, expected: &str) -> bool {
    if candidate.len() != expected.len() {
        return false;
    }
    candidate
        .bytes()
        .zip(expected.bytes())
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
    api_schema_version: u16,
    engine_schema_version: u16,
}

#[derive(Debug, Serialize)]
struct CapabilityResponse {
    api_schema_version: u16,
    engine_schema_version: u16,
    operations: Vec<&'static str>,
    max_body_bytes: usize,
}

#[derive(Debug, Serialize)]
struct ProblemResponse {
    code: &'static str,
    message: &'static str,
}

#[cfg(test)]
mod tests {
    use axum::{
        body::{to_bytes, Body},
        http::{header, Request, StatusCode},
    };
    use serde_json::{json, Value};
    use tower::ServiceExt;

    use super::*;

    const TOKEN: &str = "test-token-0123456789-ABCDEFGHIJ";

    fn authenticated(body: Value) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri("/v1/engine")
            .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .expect("request")
    }

    fn option_request() -> Value {
        json!({
            "schema_version": 1,
            "request_id": "api:option:1",
            "operation": "price_option",
            "input": {
                "symbol": "AAPL",
                "right": "call",
                "spot_micros": 190000000,
                "strike_micros": 200000000,
                "days_to_expiry": 30,
                "volatility_bps": 2500,
                "risk_free_rate_bps": 500,
                "dividend_yield_bps": 0,
                "contract_multiplier": 100
            }
        })
    }

    async fn json_body(response: Response) -> Value {
        let bytes = to_bytes(response.into_body(), MAX_MAX_BODY_BYTES)
            .await
            .expect("response body");
        serde_json::from_slice(&bytes).expect("JSON response")
    }

    #[tokio::test]
    async fn health_is_public_but_engine_and_capabilities_require_authentication() {
        let app = router(ApiConfig::new(TOKEN).unwrap());
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/healthz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(json_body(response).await["status"], "ok");

        for uri in ["/v1/capabilities", "/v1/engine"] {
            let response = app
                .clone()
                .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
            assert_eq!(
                response.headers()[header::WWW_AUTHENTICATE],
                "Bearer realm=\"market-terminal\""
            );
        }
    }

    #[tokio::test]
    async fn authenticated_engine_request_maps_typed_result_and_security_headers() {
        let app = router(ApiConfig::new(TOKEN).unwrap());
        let response = app.oneshot(authenticated(option_request())).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["x-request-id"], "api:option:1");
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(response.headers()["x-content-type-options"], "nosniff");
        let body = json_body(response).await;
        assert_eq!(body["status"], "ok");
        assert_eq!(body["result_type"], "option_analytics");
        assert_eq!(body["data"]["model_version"], "BLACK-SCHOLES-EUROPEAN-V1");
    }

    #[tokio::test]
    async fn deployment_policy_rejects_disabled_operation_before_execution() {
        let policy = OperationPolicy::from_names(["analyze_bond"]).unwrap();
        let app = router(ApiConfig::new(TOKEN).unwrap().with_operation_policy(policy));
        let response = app.oneshot(authenticated(option_request())).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(json_body(response).await["code"], "operation_forbidden");
    }

    #[tokio::test]
    async fn transport_and_domain_failures_have_distinct_statuses() {
        let app = router(ApiConfig::new(TOKEN).unwrap());
        let mut unsupported = option_request();
        unsupported["schema_version"] = json!(2);
        let response = app
            .clone()
            .oneshot(authenticated(unsupported))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let mut invalid = option_request();
        invalid["input"]["spot_micros"] = json!(0);
        let response = app.clone().oneshot(authenticated(invalid)).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

        let request = Request::builder()
            .method("POST")
            .uri("/v1/engine")
            .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
            .header(header::CONTENT_TYPE, "text/plain")
            .body(Body::from("not json"))
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
        assert_eq!(json_body(response).await["code"], "unsupported_media_type");
    }

    #[tokio::test]
    async fn oversized_payload_is_rejected_before_deserialization() {
        let config = ApiConfig::new(TOKEN)
            .unwrap()
            .with_max_body_bytes(MIN_MAX_BODY_BYTES)
            .unwrap();
        let app = router(config);
        let request = Request::builder()
            .method("POST")
            .uri("/v1/engine")
            .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from("x".repeat(MIN_MAX_BODY_BYTES + 1)))
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(json_body(response).await["code"], "payload_too_large");
    }

    #[test]
    fn configuration_is_bounded_and_never_debugs_the_secret() {
        assert!(ApiConfig::new("short").is_err());
        assert!(ApiConfig::new(format!("{} ", TOKEN)).is_err());
        assert!(OperationPolicy::from_names([]).is_err());
        assert!(OperationPolicy::from_names(["unknown"]).is_err());
        let config = ApiConfig::new(TOKEN).unwrap();
        assert!(!format!("{config:?}").contains(TOKEN));
    }
}
