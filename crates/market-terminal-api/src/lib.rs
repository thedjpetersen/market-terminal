//! Authenticated HTTP transport for `market-terminal-engine`.
//!
//! This crate is a host adapter, not a second analytical implementation. It
//! owns HTTP extraction, authentication, authorization, size limits, status
//! mapping, and response headers; every successful operation is delegated to
//! the deterministic engine crate.

use std::{
    fmt,
    sync::Arc,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use axum::{
    extract::{
        rejection::{JsonRejection, QueryRejection},
        DefaultBodyLimit, Extension, Path, Query, Request, State,
    },
    http::{header, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use market_terminal_application::{
    AnalyticalApplicationService, ApplicationConfigError, ApplicationError, ApplicationErrorCode,
    ArtifactListRequest, CapabilitySet, EngineErrorCode, EngineOutcome, EngineRequest,
    EngineResponse, ExecutionContext, PrincipalId, ResearchArtifactApplicationService,
    ResearchArtifactKind, ResearchArtifactQuery, TenantId, APPLICATION_SCHEMA_VERSION,
    ENGINE_API_SCHEMA_VERSION,
};
use market_terminal_auth::{
    valid_bearer_token, CredentialId, CredentialResolveFailure, CredentialResolver,
    ResolvedCredential,
};
use serde::{Deserialize, Serialize};
use tracing::info;

pub use market_terminal_application::ArtifactCapabilitySet as ArtifactReadPolicy;
pub use market_terminal_application::{CapabilitySet as OperationPolicy, ExecutionBudget};

pub const API_SCHEMA_VERSION: u16 = 2;
pub const DEFAULT_MAX_BODY_BYTES: usize = 4 * 1024 * 1024;
pub const MIN_MAX_BODY_BYTES: usize = 1_024;
pub const MAX_MAX_BODY_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone)]
pub struct ApiConfig {
    bearer_token: Arc<str>,
    max_body_bytes: usize,
    execution_context: ExecutionContext,
}

impl ApiConfig {
    pub fn new(bearer_token: impl Into<String>) -> Result<Self, ApiConfigError> {
        Self::for_principal(bearer_token, "local", "api")
    }

    pub fn for_principal(
        bearer_token: impl Into<String>,
        tenant_id: impl Into<String>,
        principal_id: impl Into<String>,
    ) -> Result<Self, ApiConfigError> {
        let bearer_token = bearer_token.into();
        validate_token(&bearer_token)?;
        Ok(Self {
            bearer_token: Arc::from(bearer_token),
            max_body_bytes: DEFAULT_MAX_BODY_BYTES,
            execution_context: ExecutionContext::new(
                TenantId::new(tenant_id).map_err(ApiConfigError::Application)?,
                PrincipalId::new(principal_id).map_err(ApiConfigError::Application)?,
                CapabilitySet::all(),
                ExecutionBudget::default(),
            ),
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
        self.execution_context = self.execution_context.with_capabilities(operation_policy);
        self
    }

    pub fn with_execution_budget(mut self, execution_budget: ExecutionBudget) -> Self {
        self.execution_context = self.execution_context.with_budget(execution_budget);
        self
    }

    pub fn with_artifact_policy(mut self, policy: ArtifactReadPolicy) -> Self {
        self.execution_context = self.execution_context.with_artifact_capabilities(policy);
        self
    }

    pub const fn max_body_bytes(&self) -> usize {
        self.max_body_bytes
    }

    pub const fn operation_policy(&self) -> OperationPolicy {
        self.execution_context.capabilities()
    }

    pub const fn execution_context(&self) -> &ExecutionContext {
        &self.execution_context
    }
}

impl fmt::Debug for ApiConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ApiConfig")
            .field("bearer_token", &"[REDACTED]")
            .field("max_body_bytes", &self.max_body_bytes)
            .field("execution_context", &self.execution_context)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiConfigError {
    InvalidToken,
    InvalidBodyLimit(usize),
    Application(ApplicationConfigError),
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
            Self::Application(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ApiConfigError {}

impl From<ApplicationConfigError> for ApiConfigError {
    fn from(error: ApplicationConfigError) -> Self {
        Self::Application(error)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiHostConfig {
    max_body_bytes: usize,
}

impl ApiHostConfig {
    pub const fn new() -> Self {
        Self {
            max_body_bytes: DEFAULT_MAX_BODY_BYTES,
        }
    }

    pub fn with_max_body_bytes(mut self, max_body_bytes: usize) -> Result<Self, ApiConfigError> {
        validate_body_limit(max_body_bytes)?;
        self.max_body_bytes = max_body_bytes;
        Ok(self)
    }

    pub const fn max_body_bytes(self) -> usize {
        self.max_body_bytes
    }
}

impl Default for ApiHostConfig {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
struct StaticCredentialResolver {
    bearer_token: Arc<str>,
    resolved: ResolvedCredential,
}

impl CredentialResolver for StaticCredentialResolver {
    fn resolve(
        &self,
        bearer_token: &str,
        _: u64,
    ) -> Result<Option<ResolvedCredential>, CredentialResolveFailure> {
        Ok(constant_time_eq(bearer_token, &self.bearer_token).then(|| self.resolved.clone()))
    }
}

#[derive(Clone)]
struct ApiState {
    max_body_bytes: usize,
    credential_resolver: Arc<dyn CredentialResolver>,
    service: AnalyticalApplicationService,
    artifact_service: Option<ResearchArtifactApplicationService>,
}

pub fn router(config: ApiConfig) -> Router {
    build_router(
        config.max_body_bytes,
        static_credential_resolver(&config),
        None,
    )
}

/// Builds the authenticated transport with read-only artifact routes backed by
/// a host-owned adapter. The adapter never receives a client-supplied tenant.
pub fn router_with_artifact_query(
    config: ApiConfig,
    query: Arc<dyn ResearchArtifactQuery>,
) -> Router {
    build_router(
        config.max_body_bytes,
        static_credential_resolver(&config),
        Some(ResearchArtifactApplicationService::new(query)),
    )
}

/// Builds the reusable transport with a host-owned credential resolver and an
/// optional read-only artifact adapter. Neither dependency can mutate product
/// state, and all resolved contexts remain subject to application policy.
pub fn router_with_services(
    config: ApiHostConfig,
    credential_resolver: Arc<dyn CredentialResolver>,
    artifact_query: Option<Arc<dyn ResearchArtifactQuery>>,
) -> Router {
    build_router(
        config.max_body_bytes,
        credential_resolver,
        artifact_query.map(ResearchArtifactApplicationService::new),
    )
}

fn build_router(
    max_body_bytes: usize,
    credential_resolver: Arc<dyn CredentialResolver>,
    artifact_service: Option<ResearchArtifactApplicationService>,
) -> Router {
    let state = ApiState {
        max_body_bytes,
        credential_resolver,
        service: AnalyticalApplicationService,
        artifact_service,
    };
    let mut protected = Router::new()
        .route("/v1/capabilities", get(capabilities))
        .route("/v1/engine", post(run_engine));
    if state.artifact_service.is_some() {
        protected = protected
            .route("/v1/artifacts", get(list_artifacts))
            .route("/v1/artifacts/{artifact_id}", get(get_artifact));
    }
    let protected = protected
        .route_layer(middleware::from_fn_with_state(state.clone(), authorize))
        .layer(DefaultBodyLimit::max(state.max_body_bytes));

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
            application_schema_version: APPLICATION_SCHEMA_VERSION,
            engine_schema_version: ENGINE_API_SCHEMA_VERSION,
        }),
    ))
}

async fn capabilities(
    State(state): State<ApiState>,
    Extension(context): Extension<ExecutionContext>,
) -> Response {
    let budget = context.budget();
    secure_response((
        StatusCode::OK,
        Json(CapabilityResponse {
            api_schema_version: API_SCHEMA_VERSION,
            application_schema_version: APPLICATION_SCHEMA_VERSION,
            engine_schema_version: ENGINE_API_SCHEMA_VERSION,
            tenant_id: context.tenant_id().as_str().to_owned(),
            principal_id: context.principal_id().as_str().to_owned(),
            operations: context.capabilities().allowed_names(),
            artifact_operations: context.artifact_capabilities().allowed_names(),
            max_body_bytes: state.max_body_bytes,
            max_backtest_bars: budget.max_backtest_bars(),
            max_comparison_points: budget.max_comparison_points(),
        }),
    ))
}

#[derive(Debug, Deserialize)]
struct ArtifactListParams {
    kind: Option<ResearchArtifactKind>,
    cursor: Option<String>,
    limit: Option<usize>,
}

async fn list_artifacts(
    State(state): State<ApiState>,
    Extension(context): Extension<ExecutionContext>,
    params: Result<Query<ArtifactListParams>, QueryRejection>,
) -> Response {
    let Some(service) = &state.artifact_service else {
        return not_found().await;
    };
    let Query(params) = match params {
        Ok(params) => params,
        Err(_) => {
            return problem(
                StatusCode::BAD_REQUEST,
                "invalid_artifact_request",
                "artifact query parameters are invalid",
            )
        }
    };
    match service.list(
        &context,
        ArtifactListRequest {
            kind: params.kind,
            cursor: params.cursor,
            limit: params.limit,
        },
    ) {
        Ok(page) => secure_response((StatusCode::OK, Json(page))),
        Err(error) => application_rejection(error),
    }
}

async fn get_artifact(
    State(state): State<ApiState>,
    Extension(context): Extension<ExecutionContext>,
    Path(artifact_id): Path<String>,
) -> Response {
    let Some(service) = &state.artifact_service else {
        return not_found().await;
    };
    match service.get(&context, artifact_id) {
        Ok(Some(document)) => secure_response((StatusCode::OK, Json(document))),
        Ok(None) => problem(
            StatusCode::NOT_FOUND,
            "artifact_not_found",
            "artifact was not found",
        ),
        Err(error) => application_rejection(error),
    }
}

async fn run_engine(
    State(state): State<ApiState>,
    Extension(context): Extension<ExecutionContext>,
    payload: Result<Json<EngineRequest>, JsonRejection>,
) -> Response {
    let request = match payload {
        Ok(Json(request)) => request,
        Err(rejection) => return json_rejection(rejection),
    };
    let response = match state.service.execute(&context, request) {
        Ok(response) => response,
        Err(error) => return application_rejection(error),
    };
    let status = engine_status(&response);
    let request_id = HeaderValue::from_str(&response.request_id).ok();
    let mut response = secure_response((status, Json(response)));
    if let Some(request_id) = request_id {
        response.headers_mut().insert("x-request-id", request_id);
    }
    response
}

async fn authorize(State(state): State<ApiState>, mut request: Request, next: Next) -> Response {
    let candidate = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    let observed_at_epoch_seconds = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_secs(),
        Err(_) => return authentication_unavailable(),
    };
    let resolved = match candidate.filter(|candidate| valid_bearer_token(candidate)) {
        Some(candidate) => match state
            .credential_resolver
            .resolve(candidate, observed_at_epoch_seconds)
        {
            Ok(resolved) => resolved,
            Err(CredentialResolveFailure::Unavailable) => return authentication_unavailable(),
        },
        None => None,
    };
    let Some(resolved) = resolved else {
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
    };
    let context = resolved.execution_context().clone();
    request.extensions_mut().insert(context.clone());
    let mut response = secure_response(next.run(request).await);
    response.extensions_mut().insert(resolved);
    response
}

fn authentication_unavailable() -> Response {
    problem(
        StatusCode::SERVICE_UNAVAILABLE,
        "authentication_unavailable",
        "authentication service is unavailable",
    )
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
    let resolved = response.extensions().get::<ResolvedCredential>();
    let tenant_id = resolved
        .map(|resolved| resolved.execution_context().tenant_id().as_str())
        .unwrap_or("unauthenticated");
    let principal_id = resolved
        .map(|resolved| resolved.execution_context().principal_id().as_str())
        .unwrap_or("unauthenticated");
    let credential_id = resolved
        .map(|resolved| resolved.credential_id().as_str())
        .unwrap_or("unauthenticated");
    info!(
        method = %method,
        path = %path,
        status = response.status().as_u16(),
        elapsed_micros = started.elapsed().as_micros() as u64,
        request_id,
        tenant_id,
        principal_id,
        credential_id,
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

fn application_rejection(error: ApplicationError) -> Response {
    match error.code {
        ApplicationErrorCode::CapabilityDenied => {
            problem(StatusCode::FORBIDDEN, "capability_denied", error.message)
        }
        ApplicationErrorCode::WorkloadBudgetExceeded => problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "workload_budget_exceeded",
            error.message,
        ),
        ApplicationErrorCode::InvalidArtifactRequest => problem(
            StatusCode::BAD_REQUEST,
            "invalid_artifact_request",
            error.message,
        ),
        ApplicationErrorCode::ArtifactServiceUnavailable => problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "artifact_service_unavailable",
            error.message,
        ),
        ApplicationErrorCode::ArtifactContractViolation => problem(
            StatusCode::BAD_GATEWAY,
            "artifact_contract_violation",
            error.message,
        ),
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

fn problem(status: StatusCode, code: &'static str, message: impl Into<String>) -> Response {
    secure_response((
        status,
        Json(ProblemResponse {
            code,
            message: message.into(),
        }),
    ))
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
    if !valid_bearer_token(token) {
        return Err(ApiConfigError::InvalidToken);
    }
    Ok(())
}

fn validate_body_limit(max_body_bytes: usize) -> Result<(), ApiConfigError> {
    if !(MIN_MAX_BODY_BYTES..=MAX_MAX_BODY_BYTES).contains(&max_body_bytes) {
        Err(ApiConfigError::InvalidBodyLimit(max_body_bytes))
    } else {
        Ok(())
    }
}

fn static_credential_resolver(config: &ApiConfig) -> Arc<dyn CredentialResolver> {
    Arc::new(StaticCredentialResolver {
        bearer_token: config.bearer_token.clone(),
        resolved: ResolvedCredential::new(
            CredentialId::new("configured").expect("static credential identity"),
            config.execution_context.clone(),
        ),
    })
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
    application_schema_version: u16,
    engine_schema_version: u16,
}

#[derive(Debug, Serialize)]
struct CapabilityResponse {
    api_schema_version: u16,
    application_schema_version: u16,
    engine_schema_version: u16,
    tenant_id: String,
    principal_id: String,
    operations: Vec<&'static str>,
    artifact_operations: Vec<&'static str>,
    max_body_bytes: usize,
    max_backtest_bars: usize,
    max_comparison_points: usize,
}

#[derive(Debug, Serialize)]
struct ProblemResponse {
    code: &'static str,
    message: String,
}

#[cfg(test)]
mod tests {
    use axum::{
        body::{to_bytes, Body},
        http::{header, Request, StatusCode},
    };
    use serde_json::{json, Value};
    use tower::ServiceExt;

    use market_terminal_application::{
        ArtifactQueryFailure, ResearchArtifactDocument, ResearchArtifactPage,
        ResearchArtifactSummary, TenantArtifactKey, TenantArtifactListKey,
        ARTIFACT_QUERY_SCHEMA_VERSION,
    };

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

    fn authenticated_get(uri: &str) -> Request<Body> {
        bearer_get(uri, TOKEN)
    }

    fn bearer_get(uri: &str, token: &str) -> Request<Body> {
        Request::builder()
            .uri(uri)
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .body(Body::empty())
            .expect("request")
    }

    #[derive(Clone)]
    struct FixtureCredentialResolver {
        entries: Vec<(String, ResolvedCredential)>,
    }

    impl CredentialResolver for FixtureCredentialResolver {
        fn resolve(
            &self,
            bearer_token: &str,
            _: u64,
        ) -> Result<Option<ResolvedCredential>, CredentialResolveFailure> {
            Ok(self
                .entries
                .iter()
                .find(|(token, _)| constant_time_eq(token, bearer_token))
                .map(|(_, resolved)| resolved.clone()))
        }
    }

    struct UnavailableCredentialResolver;

    impl CredentialResolver for UnavailableCredentialResolver {
        fn resolve(
            &self,
            _: &str,
            _: u64,
        ) -> Result<Option<ResolvedCredential>, CredentialResolveFailure> {
            Err(CredentialResolveFailure::Unavailable)
        }
    }

    fn resolved_credential(
        credential_id: &str,
        tenant: &str,
        principal: &str,
        capabilities: CapabilitySet,
        artifact_read: bool,
        budget: ExecutionBudget,
    ) -> ResolvedCredential {
        let mut context = ExecutionContext::new(
            TenantId::new(tenant).unwrap(),
            PrincipalId::new(principal).unwrap(),
            capabilities,
            budget,
        );
        if artifact_read {
            context = context.with_artifact_capabilities(ArtifactReadPolicy::read_only());
        }
        ResolvedCredential::new(CredentialId::new(credential_id).unwrap(), context)
    }

    #[derive(Clone)]
    struct FixtureArtifactQuery {
        documents: Vec<ResearchArtifactDocument>,
    }

    impl ResearchArtifactQuery for FixtureArtifactQuery {
        fn list(
            &self,
            key: &TenantArtifactListKey,
        ) -> Result<ResearchArtifactPage, ArtifactQueryFailure> {
            let items = self
                .documents
                .iter()
                .filter(|document| {
                    document.summary.tenant_id == *key.tenant_id()
                        && key.kind().is_none_or(|kind| document.summary.kind == kind)
                })
                .take(key.limit())
                .map(|document| document.summary.clone())
                .collect();
            Ok(ResearchArtifactPage {
                schema_version: ARTIFACT_QUERY_SCHEMA_VERSION,
                items,
                next_cursor: None,
            })
        }

        fn get(
            &self,
            key: &TenantArtifactKey,
        ) -> Result<Option<ResearchArtifactDocument>, ArtifactQueryFailure> {
            Ok(self
                .documents
                .iter()
                .find(|document| {
                    document.summary.tenant_id == *key.tenant_id()
                        && document.summary.artifact_id == key.artifact_id()
                })
                .cloned())
        }
    }

    fn artifact(
        tenant: &str,
        artifact_id: &str,
        kind: ResearchArtifactKind,
    ) -> ResearchArtifactDocument {
        ResearchArtifactDocument {
            summary: ResearchArtifactSummary {
                schema_version: ARTIFACT_QUERY_SCHEMA_VERSION,
                tenant_id: TenantId::new(tenant).unwrap(),
                artifact_id: artifact_id.to_owned(),
                kind,
                title: format!("Research {artifact_id}"),
                created_at_epoch_ms: 1_725_000_000_000,
                input_version: "fixture-v1".to_owned(),
                source: "fixture".to_owned(),
                quality: "verified".to_owned(),
                content_digest: "ART-FNV1A64-0123456789ABCDEF".to_owned(),
            },
            content: json!({"artifact_id": artifact_id}),
        }
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

    fn backtest_request(bar_count: usize) -> Value {
        let bars = (0..bar_count)
            .map(|index| {
                json!({
                    "timestamp": index,
                    "open_micros": 100000000,
                    "high_micros": 101000000,
                    "low_micros": 99000000,
                    "close_micros": 100000000,
                    "volume": 1000
                })
            })
            .collect::<Vec<_>>();
        json!({
            "schema_version": 1,
            "request_id": "api:backtest:budget",
            "operation": "run_backtest",
            "input": {
                "config": {
                    "instrument_id": "us:xnas:aapl",
                    "symbol": "AAPL",
                    "fast_window": 2,
                    "slow_window": 3,
                    "execution_cost_bps": 3,
                    "commission_micros": 1000000,
                    "initial_cash_micros": 100000000000_i64
                },
                "bars": bars,
                "source": "fixture",
                "quality": "verified",
                "input_version": "v1"
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
        assert_eq!(json_body(response).await["code"], "capability_denied");
    }

    #[tokio::test]
    async fn capabilities_are_bound_to_the_authenticated_actor_and_budget() {
        let policy = OperationPolicy::from_names(["price_option"]).unwrap();
        let budget = ExecutionBudget::new(17, 31).unwrap();
        let config = ApiConfig::for_principal(TOKEN, "tenant-a", "researcher-7")
            .unwrap()
            .with_operation_policy(policy)
            .with_execution_budget(budget);
        let app = router(config);
        let request = Request::builder()
            .uri("/v1/capabilities")
            .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = json_body(response).await;
        assert_eq!(body["application_schema_version"], 2);
        assert_eq!(body["tenant_id"], "tenant-a");
        assert_eq!(body["principal_id"], "researcher-7");
        assert_eq!(body["operations"], json!(["price_option"]));
        assert_eq!(body["max_backtest_bars"], 17);
        assert_eq!(body["max_comparison_points"], 31);
        assert_eq!(body["artifact_operations"], json!([]));
    }

    #[tokio::test]
    async fn artifact_routes_are_read_only_tenant_scoped_and_capability_scoped() {
        let query = Arc::new(FixtureArtifactQuery {
            documents: vec![
                artifact("tenant-a", "run-a", ResearchArtifactKind::BacktestRun),
                artifact("tenant-b", "run-b", ResearchArtifactKind::BacktestRun),
            ],
        });
        let config = ApiConfig::for_principal(TOKEN, "tenant-a", "researcher-7")
            .unwrap()
            .with_artifact_policy(ArtifactReadPolicy::read_only());
        let app = router_with_artifact_query(config, query.clone());

        let response = app
            .clone()
            .oneshot(authenticated_get(
                "/v1/artifacts?kind=backtest_run&limit=10",
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = json_body(response).await;
        assert_eq!(body["items"].as_array().unwrap().len(), 1);
        assert_eq!(body["items"][0]["artifact_id"], "run-a");
        assert_eq!(body["items"][0]["tenant_id"], "tenant-a");

        let response = app
            .clone()
            .oneshot(authenticated_get("/v1/artifacts/run-a"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(json_body(response).await["content"]["artifact_id"], "run-a");

        let response = app
            .clone()
            .oneshot(authenticated_get("/v1/artifacts/run-b"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = json_body(response).await;
        assert_eq!(body["code"], "artifact_not_found");
        assert!(!body.to_string().contains("tenant-b"));

        let response = app
            .oneshot(authenticated_get("/v1/capabilities"))
            .await
            .unwrap();
        assert_eq!(
            json_body(response).await["artifact_operations"],
            json!(["read_research_artifacts"])
        );

        let denied = router_with_artifact_query(ApiConfig::new(TOKEN).unwrap(), query);
        let response = denied
            .oneshot(authenticated_get("/v1/artifacts"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(json_body(response).await["code"], "capability_denied");
    }

    #[tokio::test]
    async fn artifact_request_and_adapter_contract_failures_are_distinct() {
        struct CrossTenantQuery;

        impl ResearchArtifactQuery for CrossTenantQuery {
            fn list(
                &self,
                _key: &TenantArtifactListKey,
            ) -> Result<ResearchArtifactPage, ArtifactQueryFailure> {
                Ok(ResearchArtifactPage {
                    schema_version: ARTIFACT_QUERY_SCHEMA_VERSION,
                    items: vec![
                        artifact("tenant-b", "leaked", ResearchArtifactKind::SecurityResearch)
                            .summary,
                    ],
                    next_cursor: None,
                })
            }

            fn get(
                &self,
                _key: &TenantArtifactKey,
            ) -> Result<Option<ResearchArtifactDocument>, ArtifactQueryFailure> {
                Ok(None)
            }
        }

        let config = ApiConfig::for_principal(TOKEN, "tenant-a", "researcher-7")
            .unwrap()
            .with_artifact_policy(ArtifactReadPolicy::read_only());
        let app = router_with_artifact_query(config, Arc::new(CrossTenantQuery));
        let response = app
            .clone()
            .oneshot(authenticated_get("/v1/artifacts?limit=101"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            json_body(response).await["code"],
            "invalid_artifact_request"
        );

        let response = app
            .clone()
            .oneshot(authenticated_get("/v1/artifacts?kind=not_a_kind"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            json_body(response).await["code"],
            "invalid_artifact_request"
        );

        let response = app
            .oneshot(authenticated_get("/v1/artifacts"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let body = json_body(response).await;
        assert_eq!(body["code"], "artifact_contract_violation");
        assert!(!body.to_string().contains("tenant-b"));

        struct UnavailableQuery;

        impl ResearchArtifactQuery for UnavailableQuery {
            fn list(
                &self,
                _key: &TenantArtifactListKey,
            ) -> Result<ResearchArtifactPage, ArtifactQueryFailure> {
                Err(ArtifactQueryFailure::Unavailable)
            }

            fn get(
                &self,
                _key: &TenantArtifactKey,
            ) -> Result<Option<ResearchArtifactDocument>, ArtifactQueryFailure> {
                Err(ArtifactQueryFailure::Unavailable)
            }
        }

        let config = ApiConfig::for_principal(TOKEN, "tenant-a", "researcher-7")
            .unwrap()
            .with_artifact_policy(ArtifactReadPolicy::read_only());
        let app = router_with_artifact_query(config, Arc::new(UnavailableQuery));
        let response = app
            .oneshot(authenticated_get("/v1/artifacts"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            json_body(response).await["code"],
            "artifact_service_unavailable"
        );
    }

    #[tokio::test]
    async fn actor_workload_budget_rejects_oversized_valid_requests() {
        let budget = ExecutionBudget::new(2, 10).unwrap();
        let app = router(ApiConfig::new(TOKEN).unwrap().with_execution_budget(budget));
        let response = app
            .oneshot(authenticated(backtest_request(3)))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body = json_body(response).await;
        assert_eq!(body["code"], "workload_budget_exceeded");
        assert!(body["message"].as_str().unwrap().contains("3 bars"));
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
        assert!(ApiConfig::for_principal(TOKEN, "bad tenant", "principal").is_err());
        assert!(ApiConfig::for_principal(TOKEN, "tenant", "bad/principal").is_err());
        let config = ApiConfig::new(TOKEN).unwrap();
        assert!(!format!("{config:?}").contains(TOKEN));
    }

    #[tokio::test]
    async fn injected_credentials_resolve_independent_actor_policies() {
        let token_a = "tenant-a-router-token-0123456789-ABCDE";
        let token_b = "tenant-b-router-token-0123456789-ABCDE";
        let resolver = FixtureCredentialResolver {
            entries: vec![
                (
                    token_a.to_owned(),
                    resolved_credential(
                        "browser-a",
                        "tenant-a",
                        "principal-a",
                        CapabilitySet::all(),
                        true,
                        ExecutionBudget::new(100, 500).unwrap(),
                    ),
                ),
                (
                    token_b.to_owned(),
                    resolved_credential(
                        "browser-b",
                        "tenant-b",
                        "principal-b",
                        CapabilitySet::from_names(["price_option"]).unwrap(),
                        false,
                        ExecutionBudget::new(50, 200).unwrap(),
                    ),
                ),
            ],
        };
        let app = router_with_services(ApiHostConfig::new(), Arc::new(resolver), None);

        let response = app
            .clone()
            .oneshot(bearer_get("/v1/capabilities", token_a))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = json_body(response).await;
        assert_eq!(body["tenant_id"], "tenant-a");
        assert_eq!(body["principal_id"], "principal-a");
        assert_eq!(
            body["artifact_operations"],
            json!(["read_research_artifacts"])
        );
        assert_eq!(body["max_backtest_bars"], 100);

        let response = app
            .clone()
            .oneshot(bearer_get("/v1/capabilities", token_b))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = json_body(response).await;
        assert_eq!(body["tenant_id"], "tenant-b");
        assert_eq!(body["operations"], json!(["price_option"]));
        assert_eq!(body["artifact_operations"], json!([]));
        assert_eq!(body["max_backtest_bars"], 50);

        let response = app
            .oneshot(bearer_get(
                "/v1/capabilities",
                "unknown-router-token-0123456789-ABCDE",
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(json_body(response).await["code"], "unauthorized");
    }

    #[tokio::test]
    async fn credential_backend_failure_is_distinct_and_secret_free() {
        let app = router_with_services(
            ApiHostConfig::new(),
            Arc::new(UnavailableCredentialResolver),
            None,
        );
        let response = app
            .oneshot(bearer_get("/v1/capabilities", TOKEN))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = json_body(response).await;
        assert_eq!(body["code"], "authentication_unavailable");
        assert!(!body.to_string().contains(TOKEN));
    }
}
