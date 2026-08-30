#[path = "../tests/support/web_contract_fixtures.rs"]
mod web_contract_fixtures;

use std::{env, error::Error, fs, path::Path};

const FIXTURE_PATH: &str = "contracts/web/v3/engine-fixtures.json";

fn main() -> Result<(), Box<dyn Error>> {
    let mode = env::args().nth(1).unwrap_or_else(|| "--check".to_owned());
    let rendered = web_contract_fixtures::render_contract_fixture();
    match mode.as_str() {
        "--check" => {
            let checked_in = fs::read_to_string(FIXTURE_PATH)?;
            if checked_in != rendered {
                return Err(format!(
                    "{FIXTURE_PATH} drifted; review Rust contract changes, then run `cargo run --example export_web_contracts -- --write`"
                )
                .into());
            }
        }
        "--write" => {
            let path = Path::new(FIXTURE_PATH);
            fs::create_dir_all(path.parent().expect("fixture parent"))?;
            fs::write(path, rendered)?;
        }
        _ => return Err("expected --check or --write".into()),
    }
    Ok(())
}
