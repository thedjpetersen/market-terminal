use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::{
    app::{App, CommandEditMode, InputMode, ShellAction, WorkspaceNavigationItem},
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
            Span::styled(" MT ", Style::new().bg(AMBER.into()).fg(BG.into()).bold()),
            Span::styled(" MARKET TERMINAL ", Style::new().fg(AMBER.into()).bold()),
            Span::styled("RUST", Style::new().fg(MUTED.into())),
        ]))
        .block(Block::new().borders(Borders::BOTTOM).border_style(MUTED)),
        columns[0],
    );

    let command = if let Some((input, _)) = app.shell_hints() {
        if input.is_empty() {
            "FOLLOW HINT · TYPE A LABEL"
        } else {
            "FOLLOW HINT · KEEP TYPING"
        }
    } else if app.panel_focus() {
        "PANEL FOCUS · ARROWS MOVE · ENTER INTERACT · F HINTS"
    } else if app.tmux_prefix_pending() {
        "TMUX PREFIX · ←/→ OR N/P · 1–9/0 SELECT · ? HELP"
    } else if let Some(feedback) = app.command_feedback() {
        feedback
    } else if app.command.is_empty() {
        "PRESS / FOR COMMAND"
    } else {
        app.command.as_str()
    };
    let command_border = if app.input_mode == InputMode::Command
        || app.tmux_prefix_pending()
        || app.panel_focus()
        || app.shell_hints().is_some()
    {
        CYAN
    } else {
        MUTED
    };
    let command_area = columns[1];
    frame.render_widget(
        Block::new()
            .borders(Borders::ALL)
            .border_style(command_border),
        command_area,
    );
    let command_inner = Rect::new(
        command_area.x.saturating_add(1),
        command_area.y.saturating_add(1),
        command_area.width.saturating_sub(2),
        command_area.height.saturating_sub(2),
    );
    let command_parts = Layout::horizontal([
        Constraint::Length(3),
        Constraint::Min(1),
        Constraint::Length(5),
    ])
    .split(command_inner);
    frame.render_widget(Paragraph::new(Span::styled(" ⌘ ", AMBER)), command_parts[0]);
    if app.input_mode == InputMode::Command {
        frame.render_widget(Paragraph::new(command_editor_line(app)), command_parts[1]);
    } else {
        frame.render_widget(
            Paragraph::new(Span::styled(
                command,
                if app.command.is_empty() && app.command_feedback().is_none() {
                    Style::new().fg(MUTED.into())
                } else {
                    Style::new().fg(INK.into())
                },
            )),
            command_parts[1],
        );
    }
    frame.render_widget(
        Paragraph::new(Span::styled(
            " GO ",
            Style::new().bg(CYAN.into()).fg(BG.into()).bold(),
        ))
        .alignment(Alignment::Center),
        command_parts[2],
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

fn command_editor_line(app: &App) -> Line<'_> {
    let mode_style = match app.command_edit_mode() {
        CommandEditMode::Insert => Style::new().bg(GREEN.into()).fg(BG.into()).bold(),
        CommandEditMode::Normal => Style::new().bg(AMBER.into()).fg(BG.into()).bold(),
    };
    let mode = match app.command_edit_mode() {
        CommandEditMode::Insert => " INSERT ",
        CommandEditMode::Normal => " NORMAL ",
    };
    let command = app.command();
    let cursor = app.command_cursor().min(command.len());
    let (before, rest) = command.split_at(cursor);
    let (under_cursor, after) = rest
        .chars()
        .next()
        .map(|character| rest.split_at(character.len_utf8()))
        .unwrap_or((" ", ""));
    let cursor_style = match app.command_edit_mode() {
        CommandEditMode::Insert => Style::new().bg(CYAN.into()).fg(BG.into()),
        CommandEditMode::Normal => Style::new().bg(AMBER.into()).fg(BG.into()),
    };
    let mut spans = vec![
        Span::styled(mode, mode_style),
        Span::styled(before, INK),
        Span::styled(under_cursor, cursor_style),
        Span::styled(after, INK),
    ];
    if command.is_empty() {
        spans.push(Span::styled(" TYPE FUNCTION OR SECURITY", MUTED));
    }
    Line::from(spans)
}

pub fn render_navigation(frame: &mut Frame, area: Rect, app: &App) {
    let mut spans = Vec::new();
    for (index, item) in app.workspaces.navigation_items().enumerate() {
        let text = navigation_item_text(index, item);
        let style = if item.id == app.active_workspace {
            Style::new()
                .bg(if app.panel_focus() {
                    AMBER.into()
                } else {
                    CYAN.into()
                })
                .fg(BG.into())
                .bold()
        } else if app.assistant_drawer_visible() && Some(item.id) == app.assistant_workspace() {
            Style::new().bg(AMBER.into()).fg(BG.into()).bold()
        } else {
            Style::new().fg(INK.into())
        };
        spans.push(Span::styled(text, style));
    }
    spans.extend([
        Span::styled("  SPX 5,304.72 ", MUTED),
        Span::styled("+0.86%", GREEN),
        Span::styled("  NDX 18,658.32 ", MUTED),
        Span::styled("+1.00%", GREEN),
    ]);
    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::new().bg(NAV_BG.into())),
        area,
    );
}

pub(super) fn navigation_item_text(index: usize, item: WorkspaceNavigationItem) -> String {
    let shortcut = item
        .hotkey
        .map(|hotkey| format!(" [{}]", hotkey.to_ascii_uppercase()))
        .unwrap_or_default();
    format!(" {} {}{} ", index + 1, item.label, shortcut)
}

pub fn render_footer(frame: &mut Frame, area: Rect, app: &App) {
    if app.settings_visible() {
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    format!(
                        " ESC/{} ",
                        app.key_labels(&[ShellAction::Quit, ShellAction::Settings])
                    ),
                    AMBER,
                ),
                Span::raw("CLOSE SETTINGS   "),
                Span::styled(
                    format!("{} ", app.key_labels(&[ShellAction::OpenCommand])),
                    AMBER,
                ),
                Span::raw("COMMAND   "),
                Span::styled(format!("{} ", app.key_labels(&[ShellAction::Help])), AMBER),
                Span::raw("HELP"),
            ]))
            .style(Style::new().bg(FOOTER_BG.into())),
            area,
        );
        return;
    }
    if app.help_visible() {
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    format!(
                        " ESC/{} ",
                        app.key_labels(&[ShellAction::Quit, ShellAction::Help])
                    ),
                    AMBER,
                ),
                Span::raw("CLOSE DISCOVERY   "),
                Span::styled(
                    format!("{} ", app.key_labels(&[ShellAction::OpenCommand])),
                    AMBER,
                ),
                Span::raw("COMMAND   "),
                Span::styled("CLICK ", AMBER),
                Span::raw("WORKSPACE TAB TO LEAVE"),
            ]))
            .style(Style::new().bg(FOOTER_BG.into())),
            area,
        );
        return;
    }
    if app.assistant_drawer_visible() {
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    " AI DRAWER ",
                    Style::new().bg(CYAN.into()).fg(BG.into()).bold(),
                ),
                Span::raw("  TYPE PROMPT   ENTER SEND   ESC CLOSE   CLICK OUTSIDE CLOSES"),
            ]))
            .style(Style::new().bg(FOOTER_BG.into())),
            area,
        );
        return;
    }
    if app.tmux_prefix_pending() {
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    " TMUX PREFIX ",
                    Style::new().bg(CYAN.into()).fg(BG.into()).bold(),
                ),
                Span::raw("  ←/→ N/P PREV/NEXT   1–9/0 SELECT   ? HELP   ESC CANCEL"),
            ]))
            .style(Style::new().bg(FOOTER_BG.into())),
            area,
        );
        return;
    }
    if app.shell_hints().is_some() {
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    " FOLLOW HINT ",
                    Style::new().bg(AMBER.into()).fg(BG.into()).bold(),
                ),
                Span::raw("  TYPE LABEL   BACKSPACE EDIT   ESC CANCEL"),
            ]))
            .style(Style::new().bg(FOOTER_BG.into())),
            area,
        );
        return;
    }
    if app.panel_focus() {
        let target = app
            .focused_action_label()
            .unwrap_or_else(|| "WORKSPACE RAIL".to_owned());
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    " PANEL FOCUS ",
                    Style::new().bg(AMBER.into()).fg(BG.into()).bold(),
                ),
                Span::styled(format!("  {target}  "), Style::new().fg(CYAN.into()).bold()),
                Span::raw("ARROWS MOVE   ENTER OPEN   F HINTS   ESC RETURN"),
            ]))
            .style(Style::new().bg(FOOTER_BG.into())),
            area,
        );
        return;
    }
    if app.input_mode() == InputMode::Command {
        let (mode, bindings) = match app.command_edit_mode() {
            CommandEditMode::Insert => (
                " INSERT ",
                "TYPE NORMALLY   ESC NORMAL   ENTER RUN   ↑/↓ HISTORY   ^W WORD   ^U CLEAR",
            ),
            CommandEditMode::Normal => (
                " NORMAL ",
                "h/l 0/$ w/b MOVE   i/a/I/A INSERT   x D dd DELETE   ENTER RUN   ESC CANCEL",
            ),
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(mode, Style::new().bg(CYAN.into()).fg(BG.into()).bold()),
                Span::raw(format!("  {bindings}")),
            ]))
            .style(Style::new().bg(FOOTER_BG.into())),
            area,
        );
        return;
    }
    let gallery_replay = app.runtime_settings().gallery_replay;
    let provenance = match app.active_workspace().as_str() {
        "overview" if gallery_replay => "GALLERY ANALYTICS · NOT LIVE",
        "overview" => "IMPORTED PORTFOLIO + LIVE NEWS · PERFORMANCE MAY BE UNAVAILABLE",
        "desk" if gallery_replay => "GALLERY SPLIT DESK · NOT LIVE",
        "desk" => "COMPOSITE LIVE DESK · EACH PANE RETAINS SOURCE PROVENANCE",
        "markets" if gallery_replay => "GALLERY MARKET ANALYTICS · NOT LIVE",
        "markets" => "EXTERNAL LISTED-INSTRUMENT SNAPSHOTS · SOURCE LIMITS SHOWN",
        "news" if gallery_replay => "GALLERY NEWS SNAPSHOT · NOT LIVE",
        "news" => "LIVE NEWS · VERIFY PUBLISHER SOURCE",
        "portfolio" if gallery_replay => "GALLERY PORTFOLIO SNAPSHOT · NOT YOUR POSITIONS",
        "portfolio" => {
            "VERSIONED POSITIONS + ACTIVITY + PERFORMANCE + LOTS + FILLS + ATTRIBUTION · VERIFY EACH SOURCE"
        }
        "instrument_search" if gallery_replay => "GALLERY INSTRUMENT MASTER · NOT LIVE",
        "instrument_search" => "LIVE SEC INSTRUMENT MASTER",
        "watchlist" | "charting" | "security" | "alerts" | "spreadsheet" if gallery_replay => {
            "GALLERY MARKET-DATA REPLAY · NOT LIVE"
        }
        "spreadsheet" => "LOCAL WORKBOOK + EXTERNAL MARKET DATA · VERIFY SOURCE QUALITY",
        "watchlist" | "charting" | "security" | "alerts" => {
            "EXTERNAL MARKET DATA · VERIFY PROVIDER QUALITY"
        }
        "chat" if gallery_replay => "LOCAL GALLERY CHAT · NOT LIVE",
        "chat" => "EXTERNAL IRC · VERIFY PARTICIPANTS",
        _ => "NOT INVESTMENT ADVICE",
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(format!(" {} ", app.key_labels(&[ShellAction::Quit])), AMBER),
            Span::raw("QUIT   "),
            Span::styled("[KEY] ", AMBER),
            Span::raw("FAVORITES   "),
            Span::styled(
                format!("{} ", app.key_labels(&[ShellAction::OpenCommand])),
                AMBER,
            ),
            Span::raw("COMMAND   "),
            Span::styled(format!("{} ", app.key_labels(&[ShellAction::Help])), AMBER),
            Span::raw("HELP   "),
            Span::styled(
                format!("{} ", app.key_labels(&[ShellAction::Settings])),
                AMBER,
            ),
            Span::raw("SETUP   "),
            Span::styled(
                format!(
                    "{}/^B ",
                    app.key_labels(&[ShellAction::PreviousPanel, ShellAction::NextPanel])
                ),
                AMBER,
            ),
            Span::raw("PANELS   "),
            Span::styled("ESC ", AMBER),
            Span::raw("FOCUS   "),
            Span::styled("F ", AMBER),
            Span::raw("HINTS   "),
            Span::styled(
                format!(
                    "{}/JK ",
                    app.key_labels(&[ShellAction::Up, ShellAction::Down])
                ),
                AMBER,
            ),
            Span::raw("MOVE"),
            Span::styled(format!("   {provenance}"), MUTED),
        ]))
        .style(Style::new().bg(FOOTER_BG.into())),
        area,
    );
}
