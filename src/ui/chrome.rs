use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::{
    app::{App, InputMode},
    ui::theme::{AMBER, BG, CYAN, FOOTER_BG, GREEN, INK, MUTED, NAV_BG},
};

pub fn render_header(frame: &mut Frame, area: Rect, app: &App) {
    let columns = Layout::horizontal([
        Constraint::Length(31),
        Constraint::Min(35),
        Constraint::Length(25),
    ])
    .split(area);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" MT ", Style::new().bg(AMBER).fg(BG).bold()),
            Span::styled(" MARKET TERMINAL ", Style::new().fg(AMBER).bold()),
            Span::styled("RUST", Style::new().fg(MUTED)),
        ]))
        .block(Block::new().borders(Borders::BOTTOM).border_style(MUTED)),
        columns[0],
    );

    let command = if app.command.is_empty() {
        if app.input_mode == InputMode::Command {
            "TYPE FUNCTION OR SECURITY"
        } else {
            "PRESS / FOR COMMAND"
        }
    } else {
        app.command.as_str()
    };
    let command_border = if app.input_mode == InputMode::Command { CYAN } else { MUTED };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" ⌘ ", AMBER),
            Span::styled(command, if app.command.is_empty() { Style::new().fg(MUTED) } else { Style::new().fg(INK) }),
            Span::styled("  GO ", Style::new().bg(CYAN).fg(BG).bold()),
        ]))
        .block(Block::new().borders(Borders::ALL).border_style(command_border)),
        columns[1],
    );

    let seconds = (app.ticks / 5) % 60;
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("● LIVE  ", GREEN),
            Span::styled(format!("10:42:{seconds:02}  "), INK),
            Span::styled("NYC", MUTED),
        ]))
        .alignment(Alignment::Right)
        .block(Block::new().borders(Borders::BOTTOM).border_style(MUTED)),
        columns[2],
    );
}

pub fn render_navigation(frame: &mut Frame, area: Rect, app: &App) {
    let mut spans = Vec::new();
    for (index, descriptor) in app.workspaces.descriptors().enumerate() {
        let text = format!(
            " {} {} [{}] ",
            index + 1,
            descriptor.label,
            descriptor.hotkey.to_ascii_uppercase()
        );
        let style = if descriptor.id == app.active_workspace {
            Style::new().bg(CYAN).fg(BG).bold()
        } else {
            Style::new().fg(INK)
        };
        spans.push(Span::styled(text, style));
    }
    spans.extend([
        Span::styled("  SPX 5,304.72 ", MUTED),
        Span::styled("+0.86%", GREEN),
        Span::styled("  NDX 18,658.32 ", MUTED),
        Span::styled("+1.00%", GREEN),
    ]);
    frame.render_widget(Paragraph::new(Line::from(spans)).style(Style::new().bg(NAV_BG)), area);
}

pub fn render_footer(frame: &mut Frame, area: Rect) {
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" Q/ESC ", AMBER),
            Span::raw("QUIT   "),
            Span::styled("G/M/S/P/N ", AMBER),
            Span::raw("WORKSPACES   "),
            Span::styled("/ ", AMBER),
            Span::raw("COMMAND   "),
            Span::styled("↑↓/JK ", AMBER),
            Span::raw("MOVE"),
            Span::styled("   DELAYED DEMO DATA · NOT INVESTMENT ADVICE", MUTED),
        ]))
        .style(Style::new().bg(FOOTER_BG)),
        area,
    );
}
