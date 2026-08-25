use std::{io, time::Duration};

use crossterm::event::{self, Event, KeyEventKind};
use ratatui::DefaultTerminal;

use crate::{app::App, ui};

const POLL_INTERVAL: Duration = Duration::from_millis(180);

/// Runs an application in the native terminal host.
///
/// Terminal I/O and rendering live here so the application layer remains a
/// deterministic state machine with no dependency on the presentation layer.
pub fn run(mut app: App, terminal: &mut DefaultTerminal) -> io::Result<()> {
    while !app.should_quit() {
        terminal.draw(|frame| ui::render(frame, &app))?;
        if event::poll(POLL_INTERVAL)?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            app.handle_key(key);
        }
        app.advance_tick();
    }
    Ok(())
}
