use std::io;

use market_terminal::{bootstrap, runtime};

fn main() -> io::Result<()> {
    let _ = dotenvy::dotenv();
    init_telemetry();
    ratatui::run(|terminal| runtime::run(bootstrap::persistent_app(), terminal))
}

/// Enables newline-delimited structured logs only when `RUST_LOG` is set.
///
/// The opt-in keeps normal terminal rendering pristine while allowing operators
/// to capture bounded diagnostics with the standard tracing filter syntax.
fn init_telemetry() {
    let Ok(filter) = tracing_subscriber::EnvFilter::try_from_default_env() else {
        return;
    };
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .json()
        .with_writer(io::stderr)
        .try_init();
}
