use std::{io, time::Duration};

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind},
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
        app.set_wall_clock(chrono::Utc::now());
        let frame_area = terminal.draw(|frame| render(frame, &app))?.area;
        app.set_terminal_area(frame_area);
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn header_displays_host_time_and_does_not_claim_live_market_data() {
        use chrono::{TimeZone, Utc};
        use ratatui::{backend::TestBackend, Terminal};

        for gallery in [true, false] {
            let mut settings = crate::app::RuntimeSettingsSummary::demo();
            settings.gallery_replay = gallery;
            let mut app = crate::bootstrap::demo_app().with_runtime_settings(settings);
            for code in std::iter::once(crossterm::event::KeyCode::Char('/'))
                .chain("RISK".chars().map(crossterm::event::KeyCode::Char))
                .chain(std::iter::once(crossterm::event::KeyCode::Enter))
            {
                app.handle_key(crossterm::event::KeyEvent::new(
                    code,
                    crossterm::event::KeyModifiers::NONE,
                ));
            }
            app.set_wall_clock(Utc.with_ymd_and_hms(2026, 9, 5, 12, 34, 56).unwrap());
            let mut terminal = Terminal::new(TestBackend::new(160, 48)).unwrap();
            terminal.draw(|frame| render(frame, &app)).unwrap();
            let header = terminal.backend().buffer().content[..480]
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>();
            assert!(header.contains("12:34:56 UTC"), "{header}");
            assert!(header.contains(if gallery { "DEMO" } else { "LOCAL" }));
            assert!(!header.contains("LIVE"));
            assert!(!header.contains("NYC"));
        }
    }
}
