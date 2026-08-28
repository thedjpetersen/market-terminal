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
