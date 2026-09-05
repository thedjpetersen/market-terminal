//! Authenticated HTTP transport for `market-terminal-engine`.
//!
//! This crate is a host adapter, not a second analytical implementation. It
//! owns HTTP extraction, authentication, authorization, size limits, status
//! mapping, and response headers; every successful operation is delegated to
//! the deterministic engine crate.

use std::{
    fmt,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
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
use market_terminal_admission::{
    ActorAdmissionKey, AdmissionConfigError, AdmissionController, AdmissionDecision,
    AdmissionFailure, AdmissionPolicy, InMemoryAdmissionController,
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
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tracing::info;

pub use market_terminal_application::ArtifactCapabilitySet as ArtifactReadPolicy;
pub use market_terminal_application::{CapabilitySet as OperationPolicy, ExecutionBudget};

pub const API_SCHEMA_VERSION: u16 = 3;
pub const DEFAULT_MAX_BODY_BYTES: usize = 4 * 1024 * 1024;
pub const MIN_MAX_BODY_BYTES: usize = 1_024;
pub const MAX_MAX_BODY_BYTES: usize = 8 * 1024 * 1024;
pub const DEFAULT_REQUESTS_PER_MINUTE: u32 = 600;
pub const DEFAULT_BURST_REQUESTS: u32 = 100;
pub const DEFAULT_MAX_TRACKED_ACTORS: usize = 4_096;
pub const DEFAULT_ENGINE_DEADLINE_MILLIS: u64 = 5_000;
pub const DEFAULT_ARTIFACT_DEADLINE_MILLIS: u64 = 2_000;
pub const MIN_DEADLINE_MILLIS: u64 = 1;
pub const MAX_DEADLINE_MILLIS: u64 = 60_000;
pub const DEFAULT_MAX_ENGINE_IN_FLIGHT: usize = 4;
pub const DEFAULT_MAX_ARTIFACT_IN_FLIGHT: usize = 8;
pub const MIN_MAX_IN_FLIGHT: usize = 1;
pub const MAX_MAX_IN_FLIGHT: usize = 64;
pub const API_PROBLEM_CODES: [&str; 17] = [
    "unauthorized",
    "authentication_unavailable",
    "admission_unavailable",
    "rate_limit_exceeded",
    "concurrency_limit_exceeded",
    "request_deadline_exceeded",
    "execution_unavailable",
    "invalid_json",
    "unsupported_media_type",
    "payload_too_large",
    "capability_denied",
    "workload_budget_exceeded",
    "invalid_artifact_request",
    "artifact_not_found",
    "artifact_contract_violation",
    "artifact_service_unavailable",
    "not_found",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiProblemCode {
    Unauthorized,
    AuthenticationUnavailable,
    AdmissionUnavailable,
    RateLimitExceeded,
    ConcurrencyLimitExceeded,
    RequestDeadlineExceeded,
    ExecutionUnavailable,
    InvalidJson,
    UnsupportedMediaType,
    PayloadTooLarge,
    CapabilityDenied,
    WorkloadBudgetExceeded,
    InvalidArtifactRequest,
    ArtifactNotFound,
    ArtifactContractViolation,
    ArtifactServiceUnavailable,
    NotFound,
}

pub const API_PROBLEM_VARIANTS: [ApiProblemCode; 17] = [
    ApiProblemCode::Unauthorized,
    ApiProblemCode::AuthenticationUnavailable,
    ApiProblemCode::AdmissionUnavailable,
    ApiProblemCode::RateLimitExceeded,
    ApiProblemCode::ConcurrencyLimitExceeded,
    ApiProblemCode::RequestDeadlineExceeded,
    ApiProblemCode::ExecutionUnavailable,
    ApiProblemCode::InvalidJson,
    ApiProblemCode::UnsupportedMediaType,
    ApiProblemCode::PayloadTooLarge,
    ApiProblemCode::CapabilityDenied,
    ApiProblemCode::WorkloadBudgetExceeded,
    ApiProblemCode::InvalidArtifactRequest,
    ApiProblemCode::ArtifactNotFound,
    ApiProblemCode::ArtifactContractViolation,
    ApiProblemCode::ArtifactServiceUnavailable,
    ApiProblemCode::NotFound,
];

impl ApiProblemCode {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Unauthorized => API_PROBLEM_CODES[0],
            Self::AuthenticationUnavailable => API_PROBLEM_CODES[1],
            Self::AdmissionUnavailable => API_PROBLEM_CODES[2],
            Self::RateLimitExceeded => API_PROBLEM_CODES[3],
            Self::ConcurrencyLimitExceeded => API_PROBLEM_CODES[4],
            Self::RequestDeadlineExceeded => API_PROBLEM_CODES[5],
            Self::ExecutionUnavailable => API_PROBLEM_CODES[6],
            Self::InvalidJson => API_PROBLEM_CODES[7],
            Self::UnsupportedMediaType => API_PROBLEM_CODES[8],
            Self::PayloadTooLarge => API_PROBLEM_CODES[9],
            Self::CapabilityDenied => API_PROBLEM_CODES[10],
            Self::WorkloadBudgetExceeded => API_PROBLEM_CODES[11],
            Self::InvalidArtifactRequest => API_PROBLEM_CODES[12],
            Self::ArtifactNotFound => API_PROBLEM_CODES[13],
            Self::ArtifactContractViolation => API_PROBLEM_CODES[14],
            Self::ArtifactServiceUnavailable => API_PROBLEM_CODES[15],
            Self::NotFound => API_PROBLEM_CODES[16],
        }
    }
}

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
    InvalidDeadline { kind: &'static str, millis: u64 },
    InvalidConcurrency { kind: &'static str, value: usize },
    Admission(AdmissionConfigError),
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
            Self::InvalidDeadline { kind, millis } => write!(
                formatter,
                "{kind} deadline {millis}ms must be between {MIN_DEADLINE_MILLIS}ms and {MAX_DEADLINE_MILLIS}ms"
            ),
            Self::InvalidConcurrency { kind, value } => write!(
                formatter,
                "{kind} in-flight limit {value} must be between {MIN_MAX_IN_FLIGHT} and {MAX_MAX_IN_FLIGHT}"
            ),
            Self::Admission(error) => error.fmt(formatter),
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

impl From<AdmissionConfigError> for ApiConfigError {
    fn from(error: AdmissionConfigError) -> Self {
        Self::Admission(error)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiHostConfig {
    max_body_bytes: usize,
    admission_policy: AdmissionPolicy,
    engine_deadline_millis: u64,
    artifact_deadline_millis: u64,
    max_engine_in_flight: usize,
    max_artifact_in_flight: usize,
}

impl ApiHostConfig {
    pub fn new() -> Self {
        Self {
            max_body_bytes: DEFAULT_MAX_BODY_BYTES,
            admission_policy: AdmissionPolicy::new(
                DEFAULT_REQUESTS_PER_MINUTE,
                DEFAULT_BURST_REQUESTS,
                DEFAULT_MAX_TRACKED_ACTORS,
            )
            .expect("default admission policy"),
            engine_deadline_millis: DEFAULT_ENGINE_DEADLINE_MILLIS,
            artifact_deadline_millis: DEFAULT_ARTIFACT_DEADLINE_MILLIS,
            max_engine_in_flight: DEFAULT_MAX_ENGINE_IN_FLIGHT,
            max_artifact_in_flight: DEFAULT_MAX_ARTIFACT_IN_FLIGHT,
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

    pub fn with_admission_policy(
        mut self,
        requests_per_minute: u32,
        burst_requests: u32,
        max_tracked_actors: usize,
    ) -> Result<Self, ApiConfigError> {
        self.admission_policy =
            AdmissionPolicy::new(requests_per_minute, burst_requests, max_tracked_actors)?;
        Ok(self)
    }

    pub fn with_deadlines(
        mut self,
        engine_deadline_millis: u64,
        artifact_deadline_millis: u64,
    ) -> Result<Self, ApiConfigError> {
        validate_deadline("engine", engine_deadline_millis)?;
        validate_deadline("artifact", artifact_deadline_millis)?;
        self.engine_deadline_millis = engine_deadline_millis;
        self.artifact_deadline_millis = artifact_deadline_millis;
        Ok(self)
    }

    pub fn with_concurrency_limits(
        mut self,
        max_engine_in_flight: usize,
        max_artifact_in_flight: usize,
    ) -> Result<Self, ApiConfigError> {
        validate_concurrency("engine", max_engine_in_flight)?;
        validate_concurrency("artifact", max_artifact_in_flight)?;
        self.max_engine_in_flight = max_engine_in_flight;
        self.max_artifact_in_flight = max_artifact_in_flight;
        Ok(self)
    }

    pub const fn admission_policy(self) -> AdmissionPolicy {
        self.admission_policy
    }

    pub const fn engine_deadline_millis(self) -> u64 {
        self.engine_deadline_millis
    }

    pub const fn artifact_deadline_millis(self) -> u64 {
        self.artifact_deadline_millis
    }

    pub const fn max_engine_in_flight(self) -> usize {
        self.max_engine_in_flight
    }

    pub const fn max_artifact_in_flight(self) -> usize {
        self.max_artifact_in_flight
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
    admission_controller: Arc<dyn AdmissionController>,
    admission_policy: AdmissionPolicy,
    admission_started_at: Instant,
    engine_deadline: Duration,
    artifact_deadline: Duration,
    engine_semaphore: Arc<Semaphore>,
    artifact_semaphore: Arc<Semaphore>,
    max_engine_in_flight: usize,
    max_artifact_in_flight: usize,
    service: AnalyticalApplicationService,
    artifact_service: Option<ResearchArtifactApplicationService>,
}

pub fn router(config: ApiConfig) -> Router {
    router_with_host_config(config, ApiHostConfig::new(), None)
}

/// Builds the legacy single-credential transport with explicit host controls.
/// This preserves local-development compatibility while applying the same
/// admission, deadline, and concurrency policy as multi-actor composition.
pub fn router_with_host_config(
    config: ApiConfig,
    host_config: ApiHostConfig,
    artifact_query: Option<Arc<dyn ResearchArtifactQuery>>,
) -> Router {
    let host_config = host_config
        .with_max_body_bytes(config.max_body_bytes)
        .expect("legacy API body limit was already validated");
    router_with_services(
        host_config,
        static_credential_resolver(&config),
        artifact_query,
    )
}

/// Builds the authenticated transport with read-only artifact routes backed by
/// a host-owned adapter. The adapter never receives a client-supplied tenant.
pub fn router_with_artifact_query(
    config: ApiConfig,
    query: Arc<dyn ResearchArtifactQuery>,
) -> Router {
    router_with_host_config(config, ApiHostConfig::new(), Some(query))
}

/// Builds the reusable transport with a host-owned credential resolver and an
/// optional read-only artifact adapter. Neither dependency can mutate product
/// state, and all resolved contexts remain subject to application policy.
pub fn router_with_services(
    config: ApiHostConfig,
    credential_resolver: Arc<dyn CredentialResolver>,
    artifact_query: Option<Arc<dyn ResearchArtifactQuery>>,
) -> Router {
    let admission_controller = Arc::new(InMemoryAdmissionController::new(config.admission_policy));
    router_with_admission_services(
        config,
        credential_resolver,
        artifact_query,
        admission_controller,
    )
}

/// Builds the transport with fully injected authentication, aggregate
/// admission, and optional artifact services. Distributed gateways can replace
/// the bounded in-memory admission controller without changing routes or the
/// application/engine layers.
pub fn router_with_admission_services(
    config: ApiHostConfig,
    credential_resolver: Arc<dyn CredentialResolver>,
    artifact_query: Option<Arc<dyn ResearchArtifactQuery>>,
    admission_controller: Arc<dyn AdmissionController>,
) -> Router {
    build_router(
        config,
        credential_resolver,
        artifact_query.map(ResearchArtifactApplicationService::new),
        admission_controller,
    )
}

fn build_router(
    config: ApiHostConfig,
    credential_resolver: Arc<dyn CredentialResolver>,
    artifact_service: Option<ResearchArtifactApplicationService>,
    admission_controller: Arc<dyn AdmissionController>,
) -> Router {
    let state = ApiState {
        max_body_bytes: config.max_body_bytes,
        credential_resolver,
        admission_controller,
        admission_policy: config.admission_policy,
        admission_started_at: Instant::now(),
        engine_deadline: Duration::from_millis(config.engine_deadline_millis),
        artifact_deadline: Duration::from_millis(config.artifact_deadline_millis),
        engine_semaphore: Arc::new(Semaphore::new(config.max_engine_in_flight)),
        artifact_semaphore: Arc::new(Semaphore::new(config.max_artifact_in_flight)),
        max_engine_in_flight: config.max_engine_in_flight,
        max_artifact_in_flight: config.max_artifact_in_flight,
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
            requests_per_minute: state.admission_policy.requests_per_minute(),
            burst_requests: state.admission_policy.burst_requests(),
            engine_deadline_millis: state.engine_deadline.as_millis() as u64,
            artifact_deadline_millis: state.artifact_deadline.as_millis() as u64,
            max_engine_in_flight: state.max_engine_in_flight,
            max_artifact_in_flight: state.max_artifact_in_flight,
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
                ApiProblemCode::InvalidArtifactRequest,
                "artifact query parameters are invalid",
            )
        }
    };
    let service = service.clone();
    let execution = execute_bounded(
        state.artifact_semaphore.clone(),
        state.artifact_deadline,
        move || {
            service.list(
                &context,
                ArtifactListRequest {
                    kind: params.kind,
                    cursor: params.cursor,
                    limit: params.limit,
                },
            )
        },
    )
    .await;
    match execution {
        Ok(Ok(page)) => secure_response((StatusCode::OK, Json(page))),
        Ok(Err(error)) => application_rejection(error),
        Err(error) => bounded_execution_rejection(error),
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
    let service = service.clone();
    let execution = execute_bounded(
        state.artifact_semaphore.clone(),
        state.artifact_deadline,
        move || service.get(&context, artifact_id),
    )
    .await;
    match execution {
        Ok(Ok(Some(document))) => secure_response((StatusCode::OK, Json(document))),
        Ok(Ok(None)) => problem(
            StatusCode::NOT_FOUND,
            ApiProblemCode::ArtifactNotFound,
            "artifact was not found",
        ),
        Ok(Err(error)) => application_rejection(error),
        Err(error) => bounded_execution_rejection(error),
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
    let request_id = request_id_header(&request.request_id);
    let service = state.service;
    let execution = execute_bounded(
        state.engine_semaphore.clone(),
        state.engine_deadline,
        move || service.execute(&context, request),
    )
    .await;
    let response = match execution {
        Ok(Ok(response)) => response,
        Ok(Err(error)) => return application_rejection(error),
        Err(error) => {
            let mut response = bounded_execution_rejection(error);
            if let Some(request_id) = request_id {
                response.headers_mut().insert("x-request-id", request_id);
            }
            return response;
        }
    };
    let status = engine_status(&response);
    let mut response = secure_response((status, Json(response)));
    if let Some(request_id) = request_id {
        response.headers_mut().insert("x-request-id", request_id);
    }
    response
}

// Only bounded correlation tokens can reach response headers and trace fields.
// The engine still reports its typed validation error for invalid input IDs.
fn request_id_header(id: &str) -> Option<HeaderValue> {
    if id.is_empty()
        || id.len() > 128
        || !id.bytes().all(|value| {
            value.is_ascii_alphanumeric() || matches!(value, b'-' | b'_' | b'.' | b':')
        })
    {
        return None;
    }
    HeaderValue::from_str(id).ok()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BoundedExecutionFailure {
    ConcurrencyLimit,
    DeadlineExceeded,
    Unavailable,
}

async fn execute_bounded<T, F>(
    semaphore: Arc<Semaphore>,
    deadline: Duration,
    operation: F,
) -> Result<T, BoundedExecutionFailure>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let permit: OwnedSemaphorePermit = semaphore
        .try_acquire_owned()
        .map_err(|_| BoundedExecutionFailure::ConcurrencyLimit)?;
    let task = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        operation()
    });
    match tokio::time::timeout(deadline, task).await {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(_)) => Err(BoundedExecutionFailure::Unavailable),
        Err(_) => Err(BoundedExecutionFailure::DeadlineExceeded),
    }
}

fn bounded_execution_rejection(error: BoundedExecutionFailure) -> Response {
    match error {
        BoundedExecutionFailure::ConcurrencyLimit => {
            let mut response = problem(
                StatusCode::TOO_MANY_REQUESTS,
                ApiProblemCode::ConcurrencyLimitExceeded,
                "authenticated work concurrency limit exceeded",
            );
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
            response
        }
        BoundedExecutionFailure::DeadlineExceeded => problem(
            StatusCode::GATEWAY_TIMEOUT,
            ApiProblemCode::RequestDeadlineExceeded,
            "request work exceeded its configured response deadline",
        ),
        BoundedExecutionFailure::Unavailable => problem(
            StatusCode::SERVICE_UNAVAILABLE,
            ApiProblemCode::ExecutionUnavailable,
            "request execution service is unavailable",
        ),
    }
}

/*
    The application service remains synchronous and host-neutral. Blocking work
    runs outside the async reactor. If the response deadline elapses, the client
    wait is cancelled while the bounded blocking task retains its semaphore
    permit through completion; this prevents timed-out work from bypassing the
    in-flight ceiling without claiming cooperative CPU cancellation.
*/

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
            ApiProblemCode::Unauthorized,
            "a valid bearer token is required",
        );
        response.headers_mut().insert(
            header::WWW_AUTHENTICATE,
            HeaderValue::from_static("Bearer realm=\"market-terminal\""),
        );
        return response;
    };
    let context = resolved.execution_context().clone();
    let actor = ActorAdmissionKey::new(context.tenant_id().clone(), context.principal_id().clone());
    let observed_at_millis = state
        .admission_started_at
        .elapsed()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64;
    let admission = match state.admission_controller.admit(&actor, observed_at_millis) {
        Ok(decision) => decision,
        Err(AdmissionFailure::Unavailable) => {
            let mut response = problem(
                StatusCode::SERVICE_UNAVAILABLE,
                ApiProblemCode::AdmissionUnavailable,
                "request admission service is unavailable",
            );
            response.extensions_mut().insert(resolved);
            return response;
        }
    };
    if let AdmissionDecision::Limited {
        limit,
        retry_after_millis,
    } = admission
    {
        let mut response = problem(
            StatusCode::TOO_MANY_REQUESTS,
            ApiProblemCode::RateLimitExceeded,
            "authenticated actor request rate exceeded",
        );
        apply_rate_headers(&mut response, limit, 0, Some(retry_after_millis));
        response.extensions_mut().insert(resolved);
        return response;
    }
    request.extensions_mut().insert(context.clone());
    let mut response = secure_response(next.run(request).await);
    if let AdmissionDecision::Allowed { limit, remaining } = admission {
        apply_rate_headers(&mut response, limit, remaining, None);
    }
    response.extensions_mut().insert(resolved);
    response
}

fn apply_rate_headers(
    response: &mut Response,
    limit: u32,
    remaining: u32,
    retry_after_millis: Option<u64>,
) {
    if let Ok(value) = HeaderValue::from_str(&limit.to_string()) {
        response.headers_mut().insert("ratelimit-limit", value);
    }
    if let Ok(value) = HeaderValue::from_str(&remaining.to_string()) {
        response.headers_mut().insert("ratelimit-remaining", value);
    }
    if let Some(retry_after_millis) = retry_after_millis {
        let retry_after_seconds = retry_after_millis.div_ceil(1_000).max(1);
        if let Ok(value) = HeaderValue::from_str(&retry_after_seconds.to_string()) {
            response.headers_mut().insert(header::RETRY_AFTER, value);
        }
        if let Ok(value) = HeaderValue::from_str(&retry_after_seconds.to_string()) {
            response.headers_mut().insert("ratelimit-reset", value);
        }
    }
}

fn authentication_unavailable() -> Response {
    problem(
        StatusCode::SERVICE_UNAVAILABLE,
        ApiProblemCode::AuthenticationUnavailable,
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
        ApiProblemCode::NotFound,
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
        ApplicationErrorCode::CapabilityDenied => problem(
            StatusCode::FORBIDDEN,
            ApiProblemCode::CapabilityDenied,
            error.message,
        ),
        ApplicationErrorCode::WorkloadBudgetExceeded => problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            ApiProblemCode::WorkloadBudgetExceeded,
            error.message,
        ),
        ApplicationErrorCode::InvalidArtifactRequest => problem(
            StatusCode::BAD_REQUEST,
            ApiProblemCode::InvalidArtifactRequest,
            error.message,
        ),
        ApplicationErrorCode::ArtifactServiceUnavailable => problem(
            StatusCode::SERVICE_UNAVAILABLE,
            ApiProblemCode::ArtifactServiceUnavailable,
            error.message,
        ),
        ApplicationErrorCode::ArtifactContractViolation => problem(
            StatusCode::BAD_GATEWAY,
            ApiProblemCode::ArtifactContractViolation,
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
            ApiProblemCode::PayloadTooLarge,
            "request body exceeds the configured limit",
        ),
        StatusCode::UNSUPPORTED_MEDIA_TYPE => (
            ApiProblemCode::UnsupportedMediaType,
            "content-type must be application/json",
        ),
        _ => (
            ApiProblemCode::InvalidJson,
            "request body is not a valid engine request",
        ),
    };
    problem(status, code, message)
}

fn problem(status: StatusCode, code: ApiProblemCode, message: impl Into<String>) -> Response {
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

fn validate_deadline(kind: &'static str, millis: u64) -> Result<(), ApiConfigError> {
    if !(MIN_DEADLINE_MILLIS..=MAX_DEADLINE_MILLIS).contains(&millis) {
        Err(ApiConfigError::InvalidDeadline { kind, millis })
    } else {
        Ok(())
    }
}

fn validate_concurrency(kind: &'static str, value: usize) -> Result<(), ApiConfigError> {
    if !(MIN_MAX_IN_FLIGHT..=MAX_MAX_IN_FLIGHT).contains(&value) {
        Err(ApiConfigError::InvalidConcurrency { kind, value })
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
    requests_per_minute: u32,
    burst_requests: u32,
    engine_deadline_millis: u64,
    artifact_deadline_millis: u64,
    max_engine_in_flight: usize,
    max_artifact_in_flight: usize,
}

#[derive(Debug, Serialize)]
struct ProblemResponse {
    code: ApiProblemCode,
    message: String,
}

#[cfg(test)]
mod tests {
    use std::{thread, time::Duration};

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

    struct UnavailableAdmissionController;

    impl AdmissionController for UnavailableAdmissionController {
        fn admit(
            &self,
            _: &ActorAdmissionKey,
            _: u64,
        ) -> Result<AdmissionDecision, AdmissionFailure> {
            Err(AdmissionFailure::Unavailable)
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

    #[derive(Clone)]
    struct SlowArtifactQuery {
        delay: Duration,
    }

    impl ResearchArtifactQuery for SlowArtifactQuery {
        fn list(
            &self,
            key: &TenantArtifactListKey,
        ) -> Result<ResearchArtifactPage, ArtifactQueryFailure> {
            thread::sleep(self.delay);
            Ok(ResearchArtifactPage {
                schema_version: ARTIFACT_QUERY_SCHEMA_VERSION,
                items: Vec::new(),
                next_cursor: key.cursor().map(str::to_owned),
            })
        }

        fn get(
            &self,
            _: &TenantArtifactKey,
        ) -> Result<Option<ResearchArtifactDocument>, ArtifactQueryFailure> {
            thread::sleep(self.delay);
            Ok(None)
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
    async fn invalid_request_ids_never_escape_into_headers_or_trace_fields() {
        for id in ["x".repeat(129), "x".repeat(100_000), "bad id".to_owned()] {
            let mut request = option_request();
            request["request_id"] = json!(id);
            let response = router(ApiConfig::new(TOKEN).unwrap())
                .oneshot(authenticated(request))
                .await
                .unwrap();
            assert!(!response.headers().contains_key("x-request-id"));
            assert_eq!(
                json_body(response).await["error"]["code"],
                "invalid_request_id"
            );
        }
        assert!(request_id_header(&"x".repeat(128)).is_some());
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
        assert!(ApiConfig::new(format!("{TOKEN} ")).is_err());
        assert!(OperationPolicy::from_names([]).is_err());
        assert!(OperationPolicy::from_names(["unknown"]).is_err());
        assert!(ApiConfig::for_principal(TOKEN, "bad tenant", "principal").is_err());
        assert!(ApiConfig::for_principal(TOKEN, "tenant", "bad/principal").is_err());
        let config = ApiConfig::new(TOKEN).unwrap();
        assert!(!format!("{config:?}").contains(TOKEN));
        assert!(ApiHostConfig::new().with_admission_policy(0, 1, 1).is_err());
        assert!(ApiHostConfig::new().with_deadlines(0, 1).is_err());
        assert!(ApiHostConfig::new().with_concurrency_limits(0, 1).is_err());
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

    #[tokio::test]
    async fn aggregate_rate_policy_is_actor_scoped_across_credentials() {
        let token_a = "aggregate-a-router-token-0123456789-ABCDE";
        let token_b = "aggregate-b-router-token-0123456789-ABCDE";
        let token_c = "aggregate-c-router-token-0123456789-ABCDE";
        let shared_a = resolved_credential(
            "credential-a",
            "tenant-a",
            "principal-a",
            CapabilitySet::all(),
            false,
            ExecutionBudget::default(),
        );
        let shared_b = resolved_credential(
            "credential-b",
            "tenant-a",
            "principal-a",
            CapabilitySet::all(),
            false,
            ExecutionBudget::default(),
        );
        let independent = resolved_credential(
            "credential-c",
            "tenant-b",
            "principal-a",
            CapabilitySet::all(),
            false,
            ExecutionBudget::default(),
        );
        let resolver = FixtureCredentialResolver {
            entries: vec![
                (token_a.to_owned(), shared_a),
                (token_b.to_owned(), shared_b),
                (token_c.to_owned(), independent),
            ],
        };
        let host = ApiHostConfig::new().with_admission_policy(1, 1, 8).unwrap();
        let app = router_with_services(host, Arc::new(resolver), None);

        let response = app
            .clone()
            .oneshot(bearer_get("/v1/capabilities", token_a))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["ratelimit-limit"], "1");
        assert_eq!(response.headers()["ratelimit-remaining"], "0");
        let body = json_body(response).await;
        assert_eq!(body["requests_per_minute"], 1);
        assert_eq!(body["burst_requests"], 1);

        let response = app
            .clone()
            .oneshot(bearer_get("/v1/capabilities", token_b))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(response.headers()[header::RETRY_AFTER], "60");
        assert_eq!(json_body(response).await["code"], "rate_limit_exceeded");

        let response = app
            .oneshot(bearer_get("/v1/capabilities", token_c))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn artifact_deadline_retains_concurrency_permit_until_work_finishes() {
        let config = ApiConfig::for_principal(TOKEN, "tenant-a", "researcher-7")
            .unwrap()
            .with_artifact_policy(ArtifactReadPolicy::read_only());
        let host = ApiHostConfig::new()
            .with_deadlines(DEFAULT_ENGINE_DEADLINE_MILLIS, 1)
            .unwrap()
            .with_concurrency_limits(DEFAULT_MAX_ENGINE_IN_FLIGHT, 1)
            .unwrap();
        let app = router_with_host_config(
            config,
            host,
            Some(Arc::new(SlowArtifactQuery {
                delay: Duration::from_millis(40),
            })),
        );

        let response = app
            .clone()
            .oneshot(authenticated_get("/v1/artifacts"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
        assert_eq!(
            json_body(response).await["code"],
            "request_deadline_exceeded"
        );

        let response = app
            .oneshot(authenticated_get("/v1/artifacts"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(
            json_body(response).await["code"],
            "concurrency_limit_exceeded"
        );
    }

    #[tokio::test]
    async fn admission_backend_failure_is_actor_aware_and_secret_free() {
        let resolved = resolved_credential(
            "credential-a",
            "tenant-a",
            "principal-a",
            CapabilitySet::all(),
            false,
            ExecutionBudget::default(),
        );
        let resolver = FixtureCredentialResolver {
            entries: vec![(TOKEN.to_owned(), resolved)],
        };
        let app = router_with_admission_services(
            ApiHostConfig::new(),
            Arc::new(resolver),
            None,
            Arc::new(UnavailableAdmissionController),
        );
        let response = app
            .oneshot(authenticated_get("/v1/capabilities"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = json_body(response).await;
        assert_eq!(body["code"], "admission_unavailable");
        assert!(!body.to_string().contains(TOKEN));
    }
}
