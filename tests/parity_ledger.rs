use std::{collections::BTreeSet, fs, path::PathBuf};

use serde_json::Value;

const PINNED_REFERENCE: &str = "fc16fd646405aec7a5525387be89c0cb376137c5";

fn ledger() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("docs")
        .join("openterminalui-parity-ledger.json");
    let raw = fs::read_to_string(path).expect("parity ledger should be readable");
    serde_json::from_str(&raw).expect("parity ledger should be valid JSON")
}

fn strings<'a>(value: &'a Value, field: &str) -> Vec<&'a str> {
    value[field]
        .as_array()
        .unwrap_or_else(|| panic!("{field} should be an array"))
        .iter()
        .map(|entry| {
            entry
                .as_str()
                .unwrap_or_else(|| panic!("{field} entries should be strings"))
        })
        .collect()
}

#[test]
fn parity_ledger_is_pinned_complete_and_machine_validated() {
    let ledger = ledger();
    assert_eq!(ledger["schema_version"], 1);
    assert_eq!(ledger["reference"]["commit"], PINNED_REFERENCE);
    assert_eq!(
        ledger["reference"]["source_inventory"]["react_route_declarations"],
        115
    );
    assert_eq!(
        ledger["reference"]["source_inventory"]["mounted_api_routers"],
        44
    );

    let allowed_statuses = strings(&ledger, "statuses")
        .into_iter()
        .collect::<BTreeSet<_>>();
    let allowed_maturities = strings(&ledger, "upstream_maturities")
        .into_iter()
        .collect::<BTreeSet<_>>();
    let capabilities = ledger["capabilities"]
        .as_array()
        .expect("capabilities should be an array");
    assert!(
        capabilities.len() >= 40,
        "the source audit should not collapse the reference into a short feature list"
    );

    let mut ids = BTreeSet::new();
    let mut priorities = BTreeSet::new();
    let mut acceptance_ids = BTreeSet::new();
    let mut all_references = BTreeSet::new();

    for (index, capability) in capabilities.iter().enumerate() {
        let expected_id = format!("OTUI-{:03}", index + 1);
        let id = capability["id"].as_str().expect("capability id");
        assert_eq!(
            id, expected_id,
            "capability IDs must stay stable and contiguous"
        );
        assert!(ids.insert(id), "duplicate capability id {id}");

        for field in ["capability", "owner", "gap"] {
            assert!(
                capability[field]
                    .as_str()
                    .is_some_and(|value| !value.trim().is_empty()),
                "{id} should have a non-empty {field}"
            );
        }

        let priority = capability["priority"].as_str().expect("priority");
        assert!(
            matches!(
                priority,
                "P0" | "P1" | "P2" | "P3" | "P4" | "P5" | "P6" | "P7" | "P8"
            ),
            "{id} has invalid priority {priority}"
        );
        priorities.insert(priority);

        let status = capability["market_status"].as_str().expect("market status");
        assert!(
            allowed_statuses.contains(status),
            "{id} has invalid status {status}"
        );
        let maturity = capability["upstream_maturity"]
            .as_str()
            .expect("upstream maturity");
        assert!(
            allowed_maturities.contains(maturity),
            "{id} has invalid maturity {maturity}"
        );

        let references = strings(capability, "reference_files");
        assert!(!references.is_empty(), "{id} needs source evidence");
        for reference in references {
            assert!(
                !reference.starts_with('/')
                    && !reference.contains("..")
                    && !reference.contains("://"),
                "{id} source evidence must be a repository-relative path: {reference}"
            );
            all_references.insert(reference);
        }

        let tests = strings(capability, "acceptance_tests");
        assert!(!tests.is_empty(), "{id} needs acceptance-test IDs");
        for test in tests {
            assert!(
                test.chars()
                    .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '-'),
                "{id} acceptance-test ID must be stable uppercase kebab-case: {test}"
            );
            assert!(
                acceptance_ids.insert(test),
                "duplicate acceptance-test ID {test}"
            );
        }
    }

    assert_eq!(
        priorities.len(),
        9,
        "the ledger should cover every roadmap priority"
    );
    for required in [
        "frontend/src/App.tsx",
        "backend/api/router.py",
        ".github/workflows/ci.yml",
    ] {
        assert!(
            all_references.contains(required),
            "missing baseline source evidence {required}"
        );
    }
}
