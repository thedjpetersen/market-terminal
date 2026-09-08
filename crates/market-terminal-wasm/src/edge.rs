//! Cloudflare host entry point. Identity and capabilities come only from the
//! Worker's verified session and server configuration, never the request body.
use market_terminal_application::{
    AnalyticalApplicationService, CapabilitySet, EngineRequest, ExecutionBudget, ExecutionContext,
    PrincipalId, TenantId,
};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn execute_authenticated_research(
    request: &str,
    tenant: &str,
    principal: &str,
    capabilities: &str,
) -> Result<String, String> {
    if request.len() > 4 * 1024 * 1024 {
        return Err("Research input exceeds 4 MiB".into());
    }
    let context = ExecutionContext::new(
        TenantId::new(tenant).map_err(|_| "Invalid tenant")?,
        PrincipalId::new(principal).map_err(|_| "Invalid principal")?,
        CapabilitySet::from_names(capabilities.split(',')).map_err(|_| "Invalid capabilities")?,
        ExecutionBudget::default(),
    );
    let request: EngineRequest =
        serde_json::from_str(request).map_err(|_| "Invalid research JSON")?;
    let response = AnalyticalApplicationService
        .execute(&context, request)
        .map_err(|e| e.to_string())?;
    serde_json::to_string(&response).map_err(|_| "Unable to encode research result".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edge_host_preserves_fixtures_and_enforces_server_capabilities() {
        let fixtures: serde_json::Value = serde_json::from_str(include_str!(
            "../../../contracts/web/v3/engine-fixtures.json"
        ))
        .unwrap();
        for case in fixtures["cases"].as_array().unwrap() {
            let request = case["request"].to_string();
            let response = execute_authenticated_research(
                &request,
                "market-terminal",
                "user-1",
                "run_backtest,compare_backtests,price_option,analyze_bond",
            )
            .unwrap();
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&response).unwrap(),
                case["response"]
            );
            let operation = case["request"]["operation"].as_str();
            // The deliberately narrower grant must reject other operations.
            if operation.is_some_and(|value| value != "price_option") {
                assert!(execute_authenticated_research(
                    &request,
                    "market-terminal",
                    "user-1",
                    "price_option"
                )
                .is_err());
            }
        }
        assert!(execute_authenticated_research("{}", "", "user-1", "price_option").is_err());
        assert!(execute_authenticated_research(
            "{}",
            "market-terminal",
            "bad/user",
            "price_option"
        )
        .is_err());
        assert!(
            execute_authenticated_research("{}", "market-terminal", "user-1", "admin").is_err()
        );
    }
}
