pub(crate) mod components;
mod chrome;
pub(crate) mod theme;

use ratatui::{
    layout::{Constraint, Layout},
    style::Style,
    widgets::Block,
    Frame,
};

use crate::app::App;
use crate::app::{InputMode, ShellChrome};

pub fn render(frame: &mut Frame, app: &App) {
    frame.render_widget(
        Block::new().style(Style::new().bg(theme::BG).fg(theme::INK)),
        frame.area(),
    );
    if app.input_mode() == InputMode::Navigation
        && app.workspaces.shell_chrome(app.active_workspace()) == ShellChrome::Immersive
    {
        app.workspaces.render(app.active_workspace(), frame, frame.area());
        return;
    }

    let rows = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(2),
        Constraint::Min(12),
        Constraint::Length(1),
    ])
    .split(frame.area());

    chrome::render_header(frame, rows[0], app);
    chrome::render_navigation(frame, rows[1], app);
    app.workspaces.render(app.active_workspace, frame, rows[2]);
    chrome::render_footer(frame, rows[3]);
}
