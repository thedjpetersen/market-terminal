use std::{io, time::Duration};

use crossterm::event::{self, Event, KeyEvent, KeyEventKind};
use ratatui::DefaultTerminal;

use crate::{app::App, ui};

const POLL_INTERVAL: Duration = Duration::from_millis(180);

/// Runs an application in the native terminal host.
///
/// Terminal I/O and rendering live here so the application layer remains a
/// deterministic state machine with no dependency on the presentation layer.
pub fn run(mut app: App, terminal: &mut DefaultTerminal) -> io::Result<()> {
    while !app.should_quit() {
        terminal.draw(|frame| render(frame, &app))?;
        if let Some(key) = read_pressed_key()? {
            app.handle_key(key);
        }
        app.advance_tick();
    }
    Ok(())
}

/// Renders one deterministic application frame.
///
/// Native hosts, tests, and documentation capture tools share this entry point
/// so screenshots exercise the same presentation path as the interactive app.
pub fn render(frame: &mut ratatui::Frame, app: &App) {
    ui::render(frame, app);
}

fn read_pressed_key() -> io::Result<Option<KeyEvent>> {
    if !event::poll(POLL_INTERVAL)? {
        return Ok(None);
    }
    let Event::Key(key) = event::read()? else {
        return Ok(None);
    };
    Ok((key.kind == KeyEventKind::Press).then_some(key))
}
