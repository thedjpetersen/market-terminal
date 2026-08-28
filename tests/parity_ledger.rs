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

fn evidence() -> Value {
    let path = repository_root()
        .join("docs")
        .join("capability-evidence.json");
    let raw = fs::read_to_string(path).expect("capability evidence should be readable");
    serde_json::from_str(&raw).expect("capability evidence should be valid JSON")
}

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
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

#[test]
fn covered_capabilities_resolve_complete_local_evidence() {
    validate_capability_evidence(&ledger(), &evidence()).expect("covered evidence should resolve");
}

#[test]
fn evidence_validator_rejects_a_missing_required_category() {
    let mut incomplete = evidence();
    incomplete["capabilities"][0]
        .as_object_mut()
        .expect("evidence entry")
        .remove("performance_cases");

    let error = validate_capability_evidence(&ledger(), &incomplete)
        .expect_err("a covered capability without performance evidence must fail");
    assert!(
        error.contains("OTUI-001") && error.contains("performance_cases"),
        "failure should identify the capability and missing category: {error}"
    );
}

fn validate_capability_evidence(ledger: &Value, evidence: &Value) -> Result<(), String> {
    if evidence["schema_version"] != 1 {
        return Err("capability evidence schema_version must be 1".to_owned());
    }
    let required_status = evidence["required_status"]
        .as_str()
        .ok_or_else(|| "capability evidence needs required_status".to_owned())?;
    if required_status != "covered" {
        return Err("capability evidence must guard the covered status".to_owned());
    }
    let sizes = required_strings(evidence, "semantic_sizes", "evidence root")?;
    if sizes != ["80x24", "120x36", "160x48"] {
        return Err("semantic_sizes must name all three supported viewports".to_owned());
    }

    let covered = ledger["capabilities"]
        .as_array()
        .ok_or_else(|| "ledger capabilities should be an array".to_owned())?
        .iter()
        .filter(|capability| capability["market_status"] == required_status)
        .map(|capability| {
            capability["id"]
                .as_str()
                .ok_or_else(|| "covered capability needs an id".to_owned())
        })
        .collect::<Result<BTreeSet<_>, _>>()?;

    let entries = evidence["capabilities"]
        .as_array()
        .ok_or_else(|| "capability evidence entries should be an array".to_owned())?;
    let mut evidenced = BTreeSet::new();
    let help_commands = market_terminal::bootstrap::demo_app()
        .help_commands()
        .into_iter()
        .map(|entry| entry.command)
        .collect::<BTreeSet<_>>();
    let golden_source = read_repository_file("tests/semantic_golden.rs")?;
    let performance_source = read_repository_file("examples/performance_gate.rs")?;

    for entry in entries {
        let id = entry["id"]
            .as_str()
            .ok_or_else(|| "capability evidence entry needs an id".to_owned())?;
        if !evidenced.insert(id) {
            return Err(format!("duplicate capability evidence for {id}"));
        }
        if !covered.contains(id) {
            return Err(format!("{id} has evidence but is not marked covered"));
        }

        for command in required_strings(entry, "commands", id)? {
            let function = command
                .split_whitespace()
                .next()
                .ok_or_else(|| format!("{id} has an empty command"))?;
            if !help_commands.contains(function) {
                return Err(format!(
                    "{id} command {command:?} is absent from the application Help catalog"
                ));
            }
        }
        for help_entry in required_strings(entry, "help_entries", id)? {
            if !help_commands.contains(help_entry) {
                return Err(format!(
                    "{id} Help entry {help_entry:?} is absent from the application catalog"
                ));
            }
        }
        for path in required_strings(entry, "implementation_files", id)? {
            read_repository_file(path)
                .map_err(|error| format!("{id} implementation evidence: {error}"))?;
        }
        for golden in required_strings(entry, "semantic_goldens", id)? {
            let marker = format!("name: \"{golden}\"");
            if !golden_source.contains(&marker) {
                return Err(format!("{id} semantic golden {golden:?} does not resolve"));
            }
        }

        let contracts = required_objects(entry, "contract_tests", id)?;
        for contract in contracts {
            let path = required_string(contract, "file", id)?;
            let test = required_string(contract, "test", id)?;
            let source = read_repository_file(path)
                .map_err(|error| format!("{id} contract test evidence: {error}"))?;
            if !source.contains(&format!("fn {test}(")) {
                return Err(format!(
                    "{id} contract test {path}::{test} does not resolve"
                ));
            }
        }

        let registers = required_objects(entry, "data_source_sections", id)?;
        for register in registers {
            let path = required_string(register, "file", id)?;
            let section = required_string(register, "section", id)?;
            let source = read_repository_file(path)
                .map_err(|error| format!("{id} data-source evidence: {error}"))?;
            if !source.contains(&format!("## {section}")) {
                return Err(format!(
                    "{id} data-source section {section:?} does not resolve in {path}"
                ));
            }
        }
        for case in required_strings(entry, "performance_cases", id)? {
            let marker = format!("name: \"{case}\"");
            if !performance_source.contains(&marker) {
                return Err(format!("{id} performance case {case:?} does not resolve"));
            }
        }
    }

    if evidenced != covered {
        let missing = covered.difference(&evidenced).copied().collect::<Vec<_>>();
        let stale = evidenced.difference(&covered).copied().collect::<Vec<_>>();
        return Err(format!(
            "covered capability evidence mismatch; missing={missing:?} stale={stale:?}"
        ));
    }
    Ok(())
}

fn required_strings<'a>(
    value: &'a Value,
    field: &str,
    context: &str,
) -> Result<Vec<&'a str>, String> {
    let entries = value[field]
        .as_array()
        .ok_or_else(|| format!("{context} needs {field}"))?;
    if entries.is_empty() {
        return Err(format!("{context} needs non-empty {field}"));
    }
    entries
        .iter()
        .map(|entry| {
            entry
                .as_str()
                .filter(|entry| !entry.trim().is_empty())
                .ok_or_else(|| format!("{context} has invalid {field} entry"))
        })
        .collect()
}

fn required_objects<'a>(
    value: &'a Value,
    field: &str,
    context: &str,
) -> Result<Vec<&'a Value>, String> {
    let entries = value[field]
        .as_array()
        .ok_or_else(|| format!("{context} needs {field}"))?;
    if entries.is_empty() || entries.iter().any(|entry| !entry.is_object()) {
        return Err(format!(
            "{context} needs non-empty object entries in {field}"
        ));
    }
    Ok(entries.iter().collect())
}

fn required_string<'a>(value: &'a Value, field: &str, context: &str) -> Result<&'a str, String> {
    value[field]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{context} needs {field}"))
}

fn read_repository_file(path: &str) -> Result<String, String> {
    if path.starts_with('/') || path.contains("..") || path.contains("://") {
        return Err(format!("unsafe repository-relative path {path:?}"));
    }
    fs::read_to_string(repository_root().join(path))
        .map_err(|error| format!("cannot read {path}: {error}"))
}
