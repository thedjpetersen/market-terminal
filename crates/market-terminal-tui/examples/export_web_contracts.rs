#[path = "../tests/support/web_contract_fixtures.rs"]
mod web_contract_fixtures;

use std::{env, error::Error, fs, path::PathBuf};

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("TUI crate should live under <repository>/crates")
        .join("contracts/web/v3/engine-fixtures.json")
}

fn main() -> Result<(), Box<dyn Error>> {
    let mode = env::args().nth(1).unwrap_or_else(|| "--check".to_owned());
    let rendered = web_contract_fixtures::render_contract_fixture();
    let fixture_path = fixture_path();
    match mode.as_str() {
        "--check" => {
            let checked_in = fs::read_to_string(&fixture_path)?;
            if checked_in != rendered {
                return Err(format!(
                    "{} drifted; review Rust contract changes, then run `cargo run -p market-terminal-tui --example export_web_contracts -- --write`",
                    fixture_path.display()
                )
                .into());
            }
        }
        "--write" => {
            fs::create_dir_all(fixture_path.parent().expect("fixture parent"))?;
            fs::write(fixture_path, rendered)?;
        }
        _ => return Err("expected --check or --write".into()),
    }
    Ok(())
}
