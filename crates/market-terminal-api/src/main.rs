use std::{env, net::SocketAddr, sync::Arc};

use market_terminal_api::{
    router_with_host_config, router_with_services, ApiConfig, ApiHostConfig, ArtifactReadPolicy,
    ExecutionBudget, OperationPolicy, DEFAULT_ARTIFACT_DEADLINE_MILLIS, DEFAULT_BURST_REQUESTS,
    DEFAULT_ENGINE_DEADLINE_MILLIS, DEFAULT_MAX_ARTIFACT_IN_FLIGHT, DEFAULT_MAX_BODY_BYTES,
    DEFAULT_MAX_ENGINE_IN_FLIGHT, DEFAULT_MAX_TRACKED_ACTORS, DEFAULT_REQUESTS_PER_MINUTE,
};
use market_terminal_application::ResearchArtifactQuery;
use market_terminal_artifact_store::LocalArtifactQuery;
use market_terminal_credential_store::LocalCredentialResolver;
use tracing::info;
use tracing_subscriber::EnvFilter;

const DEFAULT_BIND: &str = "127.0.0.1:8080";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let bind = env::var("MARKET_TERMINAL_API_BIND").unwrap_or_else(|_| DEFAULT_BIND.to_owned());
    let bind = bind.parse::<SocketAddr>()?;
    if !bind.ip().is_loopback()
        && env::var("MARKET_TERMINAL_API_ALLOW_REMOTE").as_deref() != Ok("1")
    {
        return Err(
            "non-loopback binding requires MARKET_TERMINAL_API_ALLOW_REMOTE=1 and a trusted TLS proxy"
                .into(),
        );
    }
    let max_body_bytes = match env::var("MARKET_TERMINAL_API_MAX_BODY_BYTES") {
        Ok(value) => value.parse::<usize>()?,
        Err(env::VarError::NotPresent) => DEFAULT_MAX_BODY_BYTES,
        Err(error) => return Err(error.into()),
    };
    let host_config = ApiHostConfig::new()
        .with_max_body_bytes(max_body_bytes)?
        .with_admission_policy(
            parse_optional_u32(
                "MARKET_TERMINAL_API_REQUESTS_PER_MINUTE",
                DEFAULT_REQUESTS_PER_MINUTE,
            )?,
            parse_optional_u32("MARKET_TERMINAL_API_BURST_REQUESTS", DEFAULT_BURST_REQUESTS)?,
            parse_optional_usize(
                "MARKET_TERMINAL_API_MAX_TRACKED_ACTORS",
                DEFAULT_MAX_TRACKED_ACTORS,
            )?,
        )?
        .with_deadlines(
            parse_optional_u64(
                "MARKET_TERMINAL_API_ENGINE_DEADLINE_MS",
                DEFAULT_ENGINE_DEADLINE_MILLIS,
            )?,
            parse_optional_u64(
                "MARKET_TERMINAL_API_ARTIFACT_DEADLINE_MS",
                DEFAULT_ARTIFACT_DEADLINE_MILLIS,
            )?,
        )?
        .with_concurrency_limits(
            parse_optional_usize(
                "MARKET_TERMINAL_API_MAX_ENGINE_IN_FLIGHT",
                DEFAULT_MAX_ENGINE_IN_FLIGHT,
            )?,
            parse_optional_usize(
                "MARKET_TERMINAL_API_MAX_ARTIFACT_IN_FLIGHT",
                DEFAULT_MAX_ARTIFACT_IN_FLIGHT,
            )?,
        )?;
    let credentials_file = optional_env("MARKET_TERMINAL_API_CREDENTIALS_FILE")?;
    let catalog_mode = credentials_file.is_some();
    let artifact_root = configured_artifact_root(catalog_mode)?;
    let artifact_query = artifact_root
        .as_ref()
        .map(LocalArtifactQuery::open)
        .transpose()?
        .map(|query| Arc::new(query) as Arc<dyn ResearchArtifactQuery>);
    let listener = tokio::net::TcpListener::bind(bind).await?;
    let app = match credentials_file {
        Some(path) => {
            reject_legacy_credential_configuration()?;
            let resolver = LocalCredentialResolver::open(path)?;
            let credential_count = resolver.credential_count();
            info!(
                bind = %bind,
                max_body_bytes,
                credential_mode = "hashed_catalog",
                credential_count,
                artifact_routes = artifact_root.is_some(),
                requests_per_minute = host_config.admission_policy().requests_per_minute(),
                burst_requests = host_config.admission_policy().burst_requests(),
                engine_deadline_millis = host_config.engine_deadline_millis(),
                artifact_deadline_millis = host_config.artifact_deadline_millis(),
                max_engine_in_flight = host_config.max_engine_in_flight(),
                max_artifact_in_flight = host_config.max_artifact_in_flight(),
                "market terminal API listening"
            );
            router_with_services(host_config, Arc::new(resolver), artifact_query)
        }
        None => {
            let token = env::var("MARKET_TERMINAL_API_TOKEN").map_err(|_| {
                "MARKET_TERMINAL_API_TOKEN is required when no credential catalog is configured"
            })?;
            let tenant_id =
                env::var("MARKET_TERMINAL_API_TENANT").unwrap_or_else(|_| "local".to_owned());
            let principal_id =
                env::var("MARKET_TERMINAL_API_PRINCIPAL").unwrap_or_else(|_| "api".to_owned());
            let policy = configured_operation_policy()?;
            let default_budget = ExecutionBudget::default();
            let execution_budget = ExecutionBudget::new(
                parse_optional_usize(
                    "MARKET_TERMINAL_API_MAX_BACKTEST_BARS",
                    default_budget.max_backtest_bars(),
                )?,
                parse_optional_usize(
                    "MARKET_TERMINAL_API_MAX_COMPARISON_POINTS",
                    default_budget.max_comparison_points(),
                )?,
            )?;
            let mut config = ApiConfig::for_principal(token, tenant_id, principal_id)?
                .with_max_body_bytes(max_body_bytes)?
                .with_operation_policy(policy)
                .with_execution_budget(execution_budget);
            if artifact_root.is_some() {
                config = config.with_artifact_policy(ArtifactReadPolicy::read_only());
            }
            info!(
                bind = %bind,
                max_body_bytes = config.max_body_bytes(),
                credential_mode = "single_development",
                operations = ?config.operation_policy().allowed_names(),
                tenant_id = %config.execution_context().tenant_id(),
                principal_id = %config.execution_context().principal_id(),
                max_backtest_bars = config.execution_context().budget().max_backtest_bars(),
                max_comparison_points = config.execution_context().budget().max_comparison_points(),
                artifact_routes = artifact_root.is_some(),
                requests_per_minute = host_config.admission_policy().requests_per_minute(),
                burst_requests = host_config.admission_policy().burst_requests(),
                engine_deadline_millis = host_config.engine_deadline_millis(),
                artifact_deadline_millis = host_config.artifact_deadline_millis(),
                max_engine_in_flight = host_config.max_engine_in_flight(),
                max_artifact_in_flight = host_config.max_artifact_in_flight(),
                "market terminal API listening"
            );
            router_with_host_config(config, host_config, artifact_query)
        }
    };
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

fn configured_operation_policy() -> Result<OperationPolicy, Box<dyn std::error::Error>> {
    match env::var("MARKET_TERMINAL_API_OPERATIONS") {
        Ok(value) => Ok(OperationPolicy::from_names(value.split(','))?),
        Err(env::VarError::NotPresent) => Ok(OperationPolicy::all()),
        Err(error) => Err(error.into()),
    }
}

fn configured_artifact_root(
    catalog_mode: bool,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let root = env::var("MARKET_TERMINAL_API_ARTIFACT_ROOT");
    let read_enabled = env::var("MARKET_TERMINAL_API_ARTIFACT_READ");
    if catalog_mode {
        return match (root, read_enabled) {
            (Err(env::VarError::NotPresent), Err(env::VarError::NotPresent)) => Ok(None),
            (Ok(root), Err(env::VarError::NotPresent)) if !root.is_empty() => Ok(Some(root)),
            (Ok(_), Err(env::VarError::NotPresent)) => {
                Err("MARKET_TERMINAL_API_ARTIFACT_ROOT must not be empty".into())
            }
            (_, Ok(_)) => Err(
                "MARKET_TERMINAL_API_ARTIFACT_READ is a legacy single-credential setting; catalog entries own artifact_read"
                    .into(),
            ),
            (Err(error), _) | (_, Err(error)) => Err(error.into()),
        };
    }
    match (root, read_enabled) {
        (Err(env::VarError::NotPresent), Err(env::VarError::NotPresent)) => Ok(None),
        (Ok(root), Ok(value)) if value == "1" && !root.is_empty() => Ok(Some(root)),
        (Ok(_), Err(env::VarError::NotPresent)) => Err(
            "MARKET_TERMINAL_API_ARTIFACT_ROOT requires MARKET_TERMINAL_API_ARTIFACT_READ=1".into(),
        ),
        (Err(env::VarError::NotPresent), Ok(_)) => Err(
            "MARKET_TERMINAL_API_ARTIFACT_READ requires MARKET_TERMINAL_API_ARTIFACT_ROOT".into(),
        ),
        (Ok(_), Ok(_)) => Err("MARKET_TERMINAL_API_ARTIFACT_READ must equal 1".into()),
        (Err(error), _) | (_, Err(error)) => Err(error.into()),
    }
}

fn reject_legacy_credential_configuration() -> Result<(), Box<dyn std::error::Error>> {
    for variable in [
        "MARKET_TERMINAL_API_TOKEN",
        "MARKET_TERMINAL_API_TENANT",
        "MARKET_TERMINAL_API_PRINCIPAL",
        "MARKET_TERMINAL_API_OPERATIONS",
        "MARKET_TERMINAL_API_MAX_BACKTEST_BARS",
        "MARKET_TERMINAL_API_MAX_COMPARISON_POINTS",
    ] {
        match env::var(variable) {
            Ok(_) => {
                return Err(format!(
                    "{variable} cannot be combined with MARKET_TERMINAL_API_CREDENTIALS_FILE"
                )
                .into())
            }
            Err(env::VarError::NotPresent) => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn optional_env(variable: &'static str) -> Result<Option<String>, Box<dyn std::error::Error>> {
    match env::var(variable) {
        Ok(value) if !value.is_empty() => Ok(Some(value)),
        Ok(_) => Err(format!("{variable} must not be empty").into()),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn parse_optional_usize(
    variable: &'static str,
    default: usize,
) -> Result<usize, Box<dyn std::error::Error>> {
    match env::var(variable) {
        Ok(value) => Ok(value.parse::<usize>()?),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(error.into()),
    }
}

fn parse_optional_u32(
    variable: &'static str,
    default: u32,
) -> Result<u32, Box<dyn std::error::Error>> {
    match env::var(variable) {
        Ok(value) => Ok(value.parse::<u32>()?),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(error.into()),
    }
}

fn parse_optional_u64(
    variable: &'static str,
    default: u64,
) -> Result<u64, Box<dyn std::error::Error>> {
    match env::var(variable) {
        Ok(value) => Ok(value.parse::<u64>()?),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(error.into()),
    }
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("install SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = terminate.recv() => {},
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
