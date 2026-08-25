mod app;
mod bootstrap;
mod features;
mod infrastructure;
mod ui;

use std::io;

fn main() -> io::Result<()> {
    ratatui::run(|terminal| bootstrap::demo_app().run(terminal))
}
