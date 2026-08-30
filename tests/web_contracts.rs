#[path = "support/web_contract_fixtures.rs"]
mod web_contract_fixtures;

use std::{collections::BTreeSet, fs, path::PathBuf};

use market_terminal_api::{API_PROBLEM_CODES, API_PROBLEM_VARIANTS};
use market_terminal_engine::{execute, EngineRequest, ENGINE_OPERATION_NAMES, ENGINE_RESULT_NAMES};
use serde_json::Value;

fn repository_file(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path)
}

#[test]
fn checked_in_web_fixtures_replay_every_engine_contract_exactly() {
    let fixture_path = repository_file("contracts/web/v3/engine-fixtures.json");
    let checked_in = fs::read_to_string(&fixture_path).expect("checked-in engine fixtures");
    assert_eq!(
        checked_in,
        web_contract_fixtures::render_contract_fixture(),
        "web engine fixtures drifted; review the change and regenerate intentionally"
    );

    let document: Value = serde_json::from_str(&checked_in).expect("fixture JSON");
    assert_eq!(document["contract_schema_version"], 1);
    let cases = document["cases"].as_array().expect("contract cases");
    let operations = cases
        .iter()
        .map(|case| case["operation"].as_str().expect("operation"))
        .collect::<BTreeSet<_>>();
    let results = cases
        .iter()
        .map(|case| case["result_type"].as_str().expect("result type"))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        operations,
        ENGINE_OPERATION_NAMES.into_iter().collect(),
        "every compiler-visible operation needs a fixture"
    );
    assert_eq!(
        results,
        ENGINE_RESULT_NAMES.into_iter().collect(),
        "every compiler-visible result needs a fixture"
    );

    for case in cases {
        let request: EngineRequest =
            serde_json::from_value(case["request"].clone()).expect("typed fixture request");
        let actual = serde_json::to_value(execute(request)).expect("typed fixture response");
        assert_eq!(actual, case["response"]);
    }
}

#[test]
fn typescript_discriminators_cover_the_compiler_visible_registry() {
    let source = fs::read_to_string(repository_file("contracts/web/v3/market-terminal-api.ts"))
        .expect("TypeScript contract");
    for name in ENGINE_OPERATION_NAMES {
        assert!(
            source.contains(&format!("operation: \"{name}\"")),
            "TypeScript request union is missing {name}"
        );
    }
    for name in ENGINE_RESULT_NAMES {
        assert!(
            source.contains(&format!("result_type: \"{name}\"")),
            "TypeScript response union is missing {name}"
        );
    }
    for name in API_PROBLEM_CODES {
        assert!(
            source.contains(&format!("| \"{name}\"")),
            "TypeScript problem union is missing {name}"
        );
    }
    let serialized_problem_names = API_PROBLEM_VARIANTS
        .into_iter()
        .map(|code| {
            serde_json::to_value(code)
                .expect("serialize problem code")
                .as_str()
                .expect("problem code string")
                .to_owned()
        })
        .collect::<Vec<_>>();
    assert_eq!(serialized_problem_names, API_PROBLEM_CODES);
    for required in [
        "export type EngineRequest",
        "export type EngineResponse",
        "export interface CapabilityResponse",
        "export interface ProblemResponse",
        "readonly schema_version: 1",
    ] {
        assert!(
            source.contains(required),
            "missing TypeScript contract `{required}`"
        );
    }
}
