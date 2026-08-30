use std::{
    fs,
    path::{Path, PathBuf},
};

fn rust_sources(root: &Path) -> Vec<PathBuf> {
    let mut pending = vec![root.to_owned()];
    let mut sources = Vec::new();

    while let Some(path) = pending.pop() {
        for entry in fs::read_dir(&path)
            .unwrap_or_else(|error| panic!("could not read {}: {error}", path.display()))
        {
            let path = entry.expect("source directory entry").path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                sources.push(path);
            }
        }
    }

    sources.sort();
    sources
}

fn production_source(path: &Path) -> String {
    let source = fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("could not read {}: {error}", path.display()));
    source
        .split_once("#[cfg(test)]")
        .map_or(source.as_str(), |(production, _)| production)
        .to_owned()
}

fn assert_absent(path: &Path, source: &str, needle: &str, reason: &str) {
    if let Some((index, _)) = source
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains(needle))
    {
        panic!(
            "{}:{} violates the architecture boundary: {reason} (found `{needle}`)",
            path.display(),
            index + 1
        );
    }
}

fn manifest_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("TUI crate should live under <repository>/crates")
        .to_owned()
}

fn tui_root() -> PathBuf {
    manifest_root().join("crates/market-terminal-tui")
}

fn manifest_section<'a>(manifest: &'a str, section: &str) -> &'a str {
    let heading = format!("[{section}]");
    manifest
        .split_once(&heading)
        .map(|(_, remainder)| remainder.split("\n[").next().unwrap_or(remainder))
        .unwrap_or("")
}

#[test]
fn virtual_workspace_separates_the_native_and_web_hosts() {
    let root = manifest_root();
    let workspace_manifest =
        fs::read_to_string(root.join("Cargo.toml")).expect("workspace manifest");
    assert_absent(
        &root.join("Cargo.toml"),
        &workspace_manifest,
        "[package]",
        "the repository root must remain a virtual workspace, not an implicit native host",
    );
    for member in [
        "market-terminal-engine",
        "market-terminal-application",
        "market-terminal-api",
        "market-terminal-tui",
    ] {
        assert!(
            workspace_manifest.contains(&format!("\"crates/{member}\"")),
            "virtual workspace must register {member}"
        );
    }

    let tui = tui_root();
    let tui_manifest = fs::read_to_string(tui.join("Cargo.toml")).expect("TUI manifest");
    assert!(tui_manifest.contains("name = \"market-terminal-tui\""));
    assert!(tui_manifest.contains("name = \"market-terminal\""));
    let tui_dependencies = manifest_section(&tui_manifest, "dependencies");
    assert!(
        tui_dependencies.contains("market-terminal-engine"),
        "native analytics must use the shared engine"
    );
    for forbidden in [
        "market-terminal-api",
        "market-terminal-admission",
        "market-terminal-auth",
        "market-terminal-credential-store",
        "market-terminal-artifact-store",
    ] {
        assert_absent(
            &tui.join("Cargo.toml"),
            tui_dependencies,
            forbidden,
            "the native host cannot depend on web transport or web deployment adapters",
        );
    }

    let api_manifest = fs::read_to_string(root.join("crates/market-terminal-api/Cargo.toml"))
        .expect("API manifest");
    assert_absent(
        &root.join("crates/market-terminal-api/Cargo.toml"),
        manifest_section(&api_manifest, "dependencies"),
        "market-terminal-tui",
        "the web host cannot import the native terminal host",
    );
}

#[test]
fn extracted_engine_is_host_neutral_and_terminal_facades_are_thin() {
    let root = manifest_root();
    let engine = root.join("crates/market-terminal-engine");
    let manifest = fs::read_to_string(engine.join("Cargo.toml")).expect("engine manifest");
    for dependency in [
        "crossterm",
        "ratatui",
        "tokio",
        "reqwest",
        "chrono",
        "csv",
        "dotenvy",
    ] {
        assert_absent(
            &engine.join("Cargo.toml"),
            &manifest,
            dependency,
            "the reusable engine may depend only on host-neutral libraries",
        );
    }

    for path in rust_sources(&engine.join("src")) {
        let source = production_source(&path);
        let compact = source
            .chars()
            .filter(|value| !value.is_whitespace())
            .collect::<String>();
        for (needle, reason) in [
            (
                "market_terminal::",
                "engine code cannot depend on the native host",
            ),
            (
                "crate::app",
                "engine code cannot depend on application-shell state",
            ),
            ("std::fs", "engine code cannot perform filesystem I/O"),
            ("fs::", "engine code cannot perform filesystem I/O"),
            ("std::net", "engine code cannot perform network I/O"),
            ("net::", "engine code cannot perform network I/O"),
            ("std::env", "engine code cannot read process configuration"),
            ("env::", "engine code cannot read process configuration"),
            ("std::time", "engine code cannot consult a host clock"),
            ("SystemTime", "engine code cannot consult a host clock"),
            ("Instant", "engine code cannot consult a host clock"),
            ("std::process", "engine code cannot launch host processes"),
            ("process::", "engine code cannot launch host processes"),
            ("std::thread", "engine code cannot launch host threads"),
            ("thread::", "engine code cannot launch host threads"),
        ] {
            assert!(
                !compact.contains(needle),
                "{} violates the architecture boundary: {reason} (found `{needle}`)",
                path.display()
            );
        }
    }

    for feature in ["backtesting", "options", "fixed_income"] {
        let facade = tui_root().join(format!("src/features/{feature}/domain.rs"));
        let source = production_source(&facade);
        assert!(
            source.contains("pub use market_terminal_engine::"),
            "{} must remain a compatibility facade over the extracted engine",
            facade.display()
        );
        assert_eq!(
            source.matches("pub use market_terminal_engine::").count(),
            1,
            "{} should not reacquire domain behavior",
            facade.display()
        );
    }
}

#[test]
fn application_services_are_host_neutral_and_own_engine_execution_policy() {
    let root = manifest_root();
    let application = root.join("crates/market-terminal-application");
    let manifest =
        fs::read_to_string(application.join("Cargo.toml")).expect("application manifest");
    assert!(
        manifest.contains("market-terminal-engine"),
        "application services must execute the shared analytical engine"
    );
    for dependency in [
        "market-terminal =",
        "market-terminal-tui",
        "axum",
        "tokio",
        "reqwest",
        "ratatui",
        "crossterm",
        "chrono",
    ] {
        assert_absent(
            &application.join("Cargo.toml"),
            &manifest,
            dependency,
            "application services must remain reusable across HTTP, workers, MCP, and native hosts",
        );
    }

    for path in rust_sources(&application.join("src")) {
        let source = production_source(&path);
        let compact = source
            .chars()
            .filter(|value| !value.is_whitespace())
            .collect::<String>();
        for (needle, reason) in [
            (
                "market_terminal::",
                "application services cannot depend on the native product",
            ),
            (
                "std::fs",
                "application services cannot perform filesystem I/O",
            ),
            ("fs::", "application services cannot perform filesystem I/O"),
            (
                "std::net",
                "application services cannot perform network I/O",
            ),
            ("net::", "application services cannot perform network I/O"),
            (
                "std::env",
                "application services cannot read process configuration",
            ),
            (
                "env::",
                "application services cannot read process configuration",
            ),
            (
                "std::time",
                "application services cannot consult a host clock",
            ),
            (
                "SystemTime",
                "application services cannot consult a host clock",
            ),
            (
                "Instant",
                "application services cannot consult a host clock",
            ),
            (
                "crate::features",
                "application services cannot bypass native feature ports",
            ),
            (
                "crate::infrastructure",
                "application services cannot depend on adapters",
            ),
        ] {
            assert!(
                !compact.contains(needle),
                "{} violates the architecture boundary: {reason} (found `{needle}`)",
                path.display()
            );
        }
    }

    let contract = production_source(&application.join("src/lib.rs"));
    assert!(
        contract.contains("market_terminal_engine::execute"),
        "application services must be the sole policy boundary that dispatches engine work"
    );
    let artifact_contract = production_source(&application.join("src/artifacts.rs"));
    for required in [
        "pub trait ResearchArtifactQuery",
        "TenantArtifactListKey",
        "TenantArtifactKey",
        "ArtifactCapabilitySet",
        "MAX_ARTIFACT_PAGE_SIZE",
        "MAX_ARTIFACT_DOCUMENT_BYTES",
    ] {
        assert!(
            artifact_contract.contains(required),
            "application services must retain the tenant-owned bounded artifact contract `{required}`"
        );
    }
}

#[test]
fn web_api_depends_on_application_services_without_importing_the_native_product() {
    let root = manifest_root();
    let api = root.join("crates/market-terminal-api");
    let manifest = fs::read_to_string(api.join("Cargo.toml")).expect("API manifest");
    assert!(
        manifest.contains("market-terminal-application"),
        "the web host must enter through tenant-aware application services"
    );
    for dependency in [
        "market-terminal =",
        "market-terminal-tui",
        "market-terminal-engine",
        "ratatui",
        "crossterm",
        "reqwest",
    ] {
        assert_absent(
            &api.join("Cargo.toml"),
            &manifest,
            dependency,
            "the API host cannot depend on the native product, terminal, or provider clients",
        );
    }

    for path in rust_sources(&api.join("src")) {
        let source = production_source(&path);
        for (needle, reason) in [
            (
                "market_terminal::",
                "the API cannot depend on the native product",
            ),
            (
                "market_terminal_engine::",
                "the API cannot bypass tenant-aware application services",
            ),
            (
                "crate::features",
                "the API cannot bypass the engine through native feature modules",
            ),
            (
                "crate::infrastructure",
                "the API cannot bypass feature ports through native adapters",
            ),
            ("ratatui", "the API cannot depend on terminal rendering"),
            ("crossterm", "the API cannot depend on terminal input"),
        ] {
            assert_absent(&path, &source, needle, reason);
        }
    }
}

#[test]
fn local_artifact_store_implements_the_application_port_at_the_host_edge() {
    let root = manifest_root();
    let application = root.join("crates/market-terminal-application");
    let store = root.join("crates/market-terminal-artifact-store");
    let api = root.join("crates/market-terminal-api");
    let manifest = fs::read_to_string(store.join("Cargo.toml")).expect("artifact store manifest");
    assert!(
        manifest.contains("market-terminal-application"),
        "the concrete artifact adapter must implement the application-owned port"
    );
    for dependency in [
        "market-terminal =",
        "market-terminal-tui",
        "market-terminal-engine",
        "market-terminal-api",
        "axum",
        "tokio",
        "reqwest",
        "ratatui",
        "crossterm",
    ] {
        assert_absent(
            &store.join("Cargo.toml"),
            &manifest,
            dependency,
            "the local artifact adapter may depend only on the application contract and serialization",
        );
    }

    let store_source = production_source(&store.join("src/lib.rs"));
    for required in [
        "impl ResearchArtifactQuery for LocalArtifactQuery",
        "symlink_metadata",
        "MAX_ARTIFACT_DOCUMENT_BYTES",
        "MAX_ARTIFACTS_PER_TENANT",
    ] {
        assert!(
            store_source.contains(required),
            "the local adapter must retain its fail-closed boundary `{required}`"
        );
    }
    let application_source = production_source(&application.join("src/lib.rs"));
    assert_absent(
        &application.join("src/lib.rs"),
        &application_source,
        "market_terminal_artifact_store",
        "the application contract cannot depend on a concrete repository",
    );
    let api_library = production_source(&api.join("src/lib.rs"));
    assert_absent(
        &api.join("src/lib.rs"),
        &api_library,
        "market_terminal_artifact_store",
        "the reusable API router must remain adapter-injected",
    );
    let api_binary = production_source(&api.join("src/main.rs"));
    assert!(
        api_binary.contains("market_terminal_artifact_store::LocalArtifactQuery"),
        "only the API composition root should select the concrete local adapter"
    );
}

#[test]
fn credential_resolution_is_host_neutral_and_the_store_stays_at_the_host_edge() {
    let root = manifest_root();
    let application = root.join("crates/market-terminal-application");
    let auth = root.join("crates/market-terminal-auth");
    let store = root.join("crates/market-terminal-credential-store");
    let api = root.join("crates/market-terminal-api");

    let auth_manifest = fs::read_to_string(auth.join("Cargo.toml")).expect("auth manifest");
    assert!(
        auth_manifest.contains("market-terminal-application"),
        "credential resolution must produce the application-owned actor context"
    );
    for dependency in [
        "market-terminal =",
        "market-terminal-tui",
        "market-terminal-engine",
        "market-terminal-api",
        "axum",
        "tokio",
        "serde",
        "sha2",
        "reqwest",
    ] {
        assert_absent(
            &auth.join("Cargo.toml"),
            &auth_manifest,
            dependency,
            "the authentication contract must remain host-neutral and mechanism-free",
        );
    }
    let auth_source = production_source(&auth.join("src/lib.rs"));
    for required in [
        "pub trait CredentialResolver",
        "pub struct ResolvedCredential",
        "pub struct CredentialId",
        "observed_at_epoch_seconds",
    ] {
        assert!(
            auth_source.contains(required),
            "the host-neutral authentication contract must retain `{required}`"
        );
    }
    for needle in [
        "std::fs",
        "std::net",
        "std::env",
        "SystemTime",
        "Instant",
        "Sha256",
    ] {
        assert_absent(
            &auth.join("src/lib.rs"),
            &auth_source,
            needle,
            "the authentication contract cannot own host I/O, clocks, or hashing",
        );
    }

    let store_manifest =
        fs::read_to_string(store.join("Cargo.toml")).expect("credential store manifest");
    for required in [
        "market-terminal-application",
        "market-terminal-auth",
        "sha2",
    ] {
        assert!(
            store_manifest.contains(required),
            "the concrete credential adapter must retain `{required}`"
        );
    }
    for dependency in [
        "market-terminal =",
        "market-terminal-tui",
        "market-terminal-engine",
        "market-terminal-api",
        "axum",
        "tokio",
        "reqwest",
        "ratatui",
        "crossterm",
    ] {
        assert_absent(
            &store.join("Cargo.toml"),
            &store_manifest,
            dependency,
            "the credential adapter may depend only on host-neutral contracts and local decoding",
        );
    }
    let store_source = production_source(&store.join("src/lib.rs"));
    for required in [
        "impl CredentialResolver for LocalCredentialResolver",
        "Sha256::digest",
        "constant_time_digest_eq",
        "symlink_metadata",
        "MAX_CREDENTIALS",
    ] {
        assert!(
            store_source.contains(required),
            "the local credential adapter must retain its fail-closed boundary `{required}`"
        );
    }

    let application_source = production_source(&application.join("src/lib.rs"));
    assert_absent(
        &application.join("src/lib.rs"),
        &application_source,
        "market_terminal_auth",
        "the application contract cannot depend on host authentication",
    );
    let api_library = production_source(&api.join("src/lib.rs"));
    assert_absent(
        &api.join("src/lib.rs"),
        &api_library,
        "market_terminal_credential_store",
        "the reusable API router must receive credential resolution by injection",
    );
    let api_binary = production_source(&api.join("src/main.rs"));
    assert!(
        api_binary.contains("market_terminal_credential_store::LocalCredentialResolver"),
        "only the API composition root should select the concrete credential adapter"
    );
}

#[test]
fn aggregate_admission_is_host_neutral_and_precedes_application_dispatch() {
    let root = manifest_root();
    let admission = root.join("crates/market-terminal-admission");
    let application = root.join("crates/market-terminal-application");
    let api = root.join("crates/market-terminal-api");
    let manifest = fs::read_to_string(admission.join("Cargo.toml")).expect("admission manifest");
    assert!(
        manifest.contains("market-terminal-application"),
        "aggregate admission must key validated application-owned actor identities"
    );
    for dependency in [
        "market-terminal =",
        "market-terminal-tui",
        "market-terminal-engine",
        "market-terminal-api",
        "market-terminal-auth",
        "axum",
        "tokio",
        "serde",
        "reqwest",
        "ratatui",
        "crossterm",
    ] {
        assert_absent(
            &admission.join("Cargo.toml"),
            &manifest,
            dependency,
            "admission policy must remain reusable and host-neutral",
        );
    }

    let admission_source = production_source(&admission.join("src/lib.rs"));
    for required in [
        "pub trait AdmissionController",
        "pub struct AdmissionPolicy",
        "pub struct ActorAdmissionKey",
        "pub struct InMemoryAdmissionController",
        "MAX_TRACKED_ACTORS",
        "AdmissionDecision::Limited",
    ] {
        assert!(
            admission_source.contains(required),
            "aggregate admission must retain its bounded contract `{required}`"
        );
    }
    for needle in [
        "std::fs",
        "std::net",
        "std::env",
        "SystemTime",
        "Instant",
        "tokio::",
    ] {
        assert_absent(
            &admission.join("src/lib.rs"),
            &admission_source,
            needle,
            "admission receives time from its host and cannot own I/O or runtime policy",
        );
    }

    let application_source = production_source(&application.join("src/lib.rs"));
    assert_absent(
        &application.join("src/lib.rs"),
        &application_source,
        "market_terminal_admission",
        "deterministic application services cannot depend on host admission",
    );
    let api_source = production_source(&api.join("src/lib.rs"));
    for required in [
        "router_with_admission_services",
        "admission_controller.admit",
        "execute_bounded",
        "tokio::time::timeout",
        "try_acquire_owned",
        "spawn_blocking",
    ] {
        assert!(
            api_source.contains(required),
            "the HTTP host must retain its admission/deadline boundary `{required}`"
        );
    }
}

#[test]
fn bounded_contexts_do_not_import_adapters_or_each_other() {
    let features = tui_root().join("src/features");
    let mut contexts = fs::read_dir(&features)
        .expect("feature directory")
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            path.is_dir()
                .then(|| path.file_name()?.to_str().map(str::to_owned))?
        })
        .collect::<Vec<_>>();
    contexts.sort();

    for context in &contexts {
        let context_root = features.join(context);
        for path in rust_sources(&context_root) {
            let source = production_source(&path);
            assert_absent(
                &path,
                &source,
                "crate::infrastructure",
                "features must consume their own ports, never concrete adapters",
            );
            assert_absent(
                &path,
                &source,
                "crate::features::{",
                "grouped feature imports obscure bounded-context ownership",
            );
            for other in contexts.iter().filter(|candidate| *candidate != context) {
                assert_absent(
                    &path,
                    &source,
                    &format!("crate::features::{other}"),
                    "cross-feature data must pass through a consumer-owned port and composition-root translator",
                );
            }
        }
    }
}

#[test]
fn domain_and_port_layers_do_not_depend_on_shell_or_rendering() {
    let features = tui_root().join("src/features");

    for path in rust_sources(&features) {
        let is_domain = path
            .components()
            .any(|component| component.as_os_str() == "domain");
        let is_port = path
            .file_name()
            .is_some_and(|name| name == "port.rs" || name == "ports.rs");
        if !is_domain && !is_port {
            continue;
        }

        let source = production_source(&path);
        for (needle, reason) in [
            (
                "crate::app",
                "domain and port layers must not depend on the application kernel",
            ),
            (
                "crate::ui",
                "domain and port layers must not depend on rendering",
            ),
            (
                "crate::infrastructure",
                "domain and port layers must not depend on adapters",
            ),
        ] {
            assert_absent(&path, &source, needle, reason);
        }
    }
}

#[test]
fn foundation_and_ui_keep_their_stable_dependency_direction() {
    let root = tui_root();
    for path in rust_sources(&root.join("src/foundation")) {
        let source = production_source(&path);
        for needle in [
            "crate::app",
            "crate::features",
            "crate::infrastructure",
            "crate::ui",
        ] {
            assert_absent(
                &path,
                &source,
                needle,
                "foundation value objects must remain independent",
            );
        }
    }

    for path in rust_sources(&root.join("src/ui")) {
        let source = production_source(&path);
        for needle in ["crate::features", "crate::infrastructure"] {
            assert_absent(
                &path,
                &source,
                needle,
                "shared UI may know shell contracts but not business contexts or adapters",
            );
        }
    }

    let library = fs::read_to_string(root.join("src/lib.rs")).expect("library root");
    assert!(library.contains("mod infrastructure;"));
    assert!(library.contains("mod ui;"));
    assert!(!library.contains("pub mod infrastructure;"));
    assert!(!library.contains("pub mod ui;"));
}
