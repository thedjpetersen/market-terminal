use std::io;

use market_terminal::{bootstrap, runtime};

fn main() -> io::Result<()> {
    let _ = dotenvy::dotenv();
    ratatui::run(|terminal| runtime::run(bootstrap::persistent_app(), terminal))
}
