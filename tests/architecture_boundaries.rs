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
        let facade = root.join(format!("src/features/{feature}/domain.rs"));
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
        assert!(
            source.contains("market_terminal_engine::execute"),
            "{} must be the sole policy boundary that dispatches engine work",
            path.display()
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
fn bounded_contexts_do_not_import_adapters_or_each_other() {
    let features = manifest_root().join("src/features");
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
    let features = manifest_root().join("src/features");

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
    let root = manifest_root();
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
