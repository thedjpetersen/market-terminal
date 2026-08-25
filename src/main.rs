use std::io;

use market_terminal::{bootstrap, runtime};

fn main() -> io::Result<()> {
    ratatui::run(|terminal| runtime::run(bootstrap::persistent_app(), terminal))
}
