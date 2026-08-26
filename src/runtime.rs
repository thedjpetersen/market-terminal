use std::{io, time::Duration};

use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind,
    },
    execute,
};
use ratatui::DefaultTerminal;

use crate::{app::App, ui};

const POLL_INTERVAL: Duration = Duration::from_millis(180);

/// Runs an application in the native terminal host.
///
/// Terminal I/O and rendering live here so the application layer remains a
/// deterministic state machine with no dependency on the presentation layer.
pub fn run(mut app: App, terminal: &mut DefaultTerminal) -> io::Result<()> {
    let _mouse_capture = MouseCaptureGuard::enable()?;
    while !app.should_quit() {
        let frame_area = terminal.draw(|frame| render(frame, &app))?.area;
        if let Some(input) = read_input_event()? {
            match input {
                Event::Key(key) if key.kind == KeyEventKind::Press => app.handle_key(key),
                Event::Mouse(mouse) => app.handle_mouse(mouse, frame_area),
                _ => {}
            }
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

fn read_input_event() -> io::Result<Option<Event>> {
    if !event::poll(POLL_INTERVAL)? {
        return Ok(None);
    }
    Ok(Some(event::read()?))
}

struct MouseCaptureGuard;

impl MouseCaptureGuard {
    fn enable() -> io::Result<Self> {
        execute!(io::stdout(), EnableMouseCapture)?;
        Ok(Self)
    }
}

impl Drop for MouseCaptureGuard {
    fn drop(&mut self) {
        let _ = execute!(io::stdout(), DisableMouseCapture);
    }
}
