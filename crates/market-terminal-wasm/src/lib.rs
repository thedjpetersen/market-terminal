//! Local browser computation. This actor grants no remote data or storage access.
use market_terminal_application::{
    AnalyticalApplicationService, CapabilitySet, EngineRequest, ExecutionBudget, ExecutionContext,
    PrincipalId, TenantId,
};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn execute_research(request: &str) -> Result<String, String> {
    if request.len() > 4 * 1024 * 1024 {
        return Err("Research input exceeds 4 MiB".to_owned());
    }
    let request: EngineRequest = serde_json::from_str(request).map_err(|e| e.to_string())?;
    let context = ExecutionContext::new(
        TenantId::new("local-browser").expect("fixed identity"),
        PrincipalId::new("device-user").expect("fixed identity"),
        CapabilitySet::all(),
        ExecutionBudget::default(),
    );
    let response = AnalyticalApplicationService
        .execute(&context, request)
        .map_err(|e| e.to_string())?;
    serde_json::to_string(&response).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn browser_adapter_replays_native_fixtures() {
        let fixtures: serde_json::Value = serde_json::from_str(include_str!(
            "../../../contracts/web/v3/engine-fixtures.json"
        ))
        .unwrap();
        for case in fixtures["cases"].as_array().unwrap() {
            let response = super::execute_research(&case["request"].to_string()).unwrap();
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&response).unwrap(),
                case["response"]
            );
        }
    }
}
