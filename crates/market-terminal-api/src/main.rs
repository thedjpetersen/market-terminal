use std::{env, net::SocketAddr, sync::Arc};

use market_terminal_api::{
    router, router_with_artifact_query, ApiConfig, ArtifactReadPolicy, ExecutionBudget,
    OperationPolicy, DEFAULT_MAX_BODY_BYTES,
};
use market_terminal_artifact_store::LocalArtifactQuery;
use tracing::info;
use tracing_subscriber::EnvFilter;

const DEFAULT_BIND: &str = "127.0.0.1:8080";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let token = env::var("MARKET_TERMINAL_API_TOKEN")
        .map_err(|_| "MARKET_TERMINAL_API_TOKEN is required")?;
    let tenant_id = env::var("MARKET_TERMINAL_API_TENANT").unwrap_or_else(|_| "local".to_owned());
    let principal_id =
        env::var("MARKET_TERMINAL_API_PRINCIPAL").unwrap_or_else(|_| "api".to_owned());
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
    let policy = match env::var("MARKET_TERMINAL_API_OPERATIONS") {
        Ok(value) => {
            let names = value.split(',').collect::<Vec<_>>();
            OperationPolicy::from_names(names)
        }?,
        Err(env::VarError::NotPresent) => OperationPolicy::all(),
        Err(error) => return Err(error.into()),
    };
    let default_budget = ExecutionBudget::default();
    let max_backtest_bars = parse_optional_usize(
        "MARKET_TERMINAL_API_MAX_BACKTEST_BARS",
        default_budget.max_backtest_bars(),
    )?;
    let max_comparison_points = parse_optional_usize(
        "MARKET_TERMINAL_API_MAX_COMPARISON_POINTS",
        default_budget.max_comparison_points(),
    )?;
    let execution_budget = ExecutionBudget::new(max_backtest_bars, max_comparison_points)?;
    let mut config = ApiConfig::for_principal(token, tenant_id, principal_id)?
        .with_max_body_bytes(max_body_bytes)?
        .with_operation_policy(policy)
        .with_execution_budget(execution_budget);
    let artifact_root = configured_artifact_root()?;
    if artifact_root.is_some() {
        config = config.with_artifact_policy(ArtifactReadPolicy::read_only());
    }
    let artifact_query = artifact_root
        .as_ref()
        .map(LocalArtifactQuery::open)
        .transpose()?;
    let listener = tokio::net::TcpListener::bind(bind).await?;
    info!(
        bind = %bind,
        max_body_bytes = config.max_body_bytes(),
        operations = ?config.operation_policy().allowed_names(),
        tenant_id = %config.execution_context().tenant_id(),
        principal_id = %config.execution_context().principal_id(),
        max_backtest_bars = config.execution_context().budget().max_backtest_bars(),
        max_comparison_points = config.execution_context().budget().max_comparison_points(),
        artifact_read = artifact_root.is_some(),
        "market terminal API listening"
    );
    let app = match artifact_query {
        Some(query) => router_with_artifact_query(config, Arc::new(query)),
        None => router(config),
    };
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

fn configured_artifact_root() -> Result<Option<String>, Box<dyn std::error::Error>> {
    let root = env::var("MARKET_TERMINAL_API_ARTIFACT_ROOT");
    let read_enabled = env::var("MARKET_TERMINAL_API_ARTIFACT_READ");
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
