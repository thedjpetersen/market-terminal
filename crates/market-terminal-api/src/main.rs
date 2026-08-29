use std::{env, net::SocketAddr};

use market_terminal_api::{
    router, ApiConfig, ExecutionBudget, OperationPolicy, DEFAULT_MAX_BODY_BYTES,
};
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
    let config = ApiConfig::for_principal(token, tenant_id, principal_id)?
        .with_max_body_bytes(max_body_bytes)?
        .with_operation_policy(policy)
        .with_execution_budget(execution_budget);
    let listener = tokio::net::TcpListener::bind(bind).await?;
    info!(
        bind = %bind,
        max_body_bytes = config.max_body_bytes(),
        operations = ?config.operation_policy().allowed_names(),
        tenant_id = %config.execution_context().tenant_id(),
        principal_id = %config.execution_context().principal_id(),
        max_backtest_bars = config.execution_context().budget().max_backtest_bars(),
        max_comparison_points = config.execution_context().budget().max_comparison_points(),
        "market terminal API listening"
    );
    axum::serve(listener, router(config))
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
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
