mod chrome;
pub(crate) mod components;
pub(crate) mod theme;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

use crate::app::{App, InputMode, ShellAction, ShellChrome, ShellHintTarget, WorkspaceId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ShellLayout {
    pub header: Rect,
    pub navigation: Rect,
    pub workspace: Rect,
    pub footer: Rect,
    pub command: Rect,
    pub command_go: Rect,
}

impl ShellLayout {
    pub fn new(area: Rect) -> Self {
        let rows = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(2),
            Constraint::Min(12),
            Constraint::Length(1),
        ])
        .split(area);
        let header_columns = Layout::horizontal([
            Constraint::Length(31),
            Constraint::Min(35),
            Constraint::Length(25),
        ])
        .split(rows[0]);
        let command = header_columns[1];
        let command_go = Rect::new(
            command.x.saturating_add(command.width.saturating_sub(6)),
            command.y.saturating_add(1),
            command.width.saturating_sub(2).min(5),
            command.height.saturating_sub(2),
        );
        Self {
            header: rows[0],
            navigation: rows[1],
            workspace: rows[2],
            footer: rows[3],
            command,
            command_go,
        }
    }

    pub(crate) fn for_app(app: &App, area: Rect) -> Self {
        if uses_immersive_shell(app) {
            Self {
                header: Rect::default(),
                navigation: Rect::default(),
                workspace: area,
                footer: Rect::default(),
                command: Rect::default(),
                command_go: Rect::default(),
            }
        } else {
            Self::new(area)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShellClickTarget {
    CommandInput,
    CommandGo,
    Navigation(WorkspaceId),
    Workspace(Rect),
    AssistantDrawer(Rect),
    AssistantClose,
    AssistantBackdrop,
    HelpClose,
    HelpOverlay,
    SettingsClose,
    SettingsThemePrevious,
    SettingsThemeNext,
    SettingsOverlay,
    Quit,
}

pub(crate) fn hit_test(app: &App, area: Rect, column: u16, row: u16) -> Option<ShellClickTarget> {
    let layout = ShellLayout::for_app(app, area);
    if app.settings_visible() && contains(settings_close_area(layout.workspace), column, row) {
        return Some(ShellClickTarget::SettingsClose);
    }
    if app.settings_visible()
        && contains(settings_theme_previous_area(layout.workspace), column, row)
    {
        return Some(ShellClickTarget::SettingsThemePrevious);
    }
    if app.settings_visible() && contains(settings_theme_next_area(layout.workspace), column, row) {
        return Some(ShellClickTarget::SettingsThemeNext);
    }
    if app.settings_visible() && contains(layout.workspace, column, row) {
        return Some(ShellClickTarget::SettingsOverlay);
    }
    if app.help_visible() && contains(help_close_area(layout.workspace), column, row) {
        return Some(ShellClickTarget::HelpClose);
    }
    if app.help_visible() && contains(layout.workspace, column, row) {
        return Some(ShellClickTarget::HelpOverlay);
    }
    if contains(layout.command_go, column, row) {
        return Some(ShellClickTarget::CommandGo);
    }
    if contains(layout.command, column, row) {
        return Some(ShellClickTarget::CommandInput);
    }
    if contains(layout.navigation, column, row) {
        let mut item_x = layout.navigation.x;
        for (index, item) in app.workspaces.navigation_items().enumerate() {
            let width = chrome::navigation_item_text(index, item).chars().count() as u16;
            let item_area = Rect::new(item_x, layout.navigation.y, width, layout.navigation.height);
            if contains(item_area, column, row) {
                return Some(ShellClickTarget::Navigation(item.id));
            }
            item_x = item_x.saturating_add(width);
        }
    }
    if contains(layout.header, column, row) && column < layout.command.x {
        if let Some(home) = app.workspaces.navigation_items().next() {
            return Some(ShellClickTarget::Navigation(home.id));
        }
    }
    if app.assistant_drawer_visible()
        && contains(assistant_close_area(layout.workspace), column, row)
    {
        return Some(ShellClickTarget::AssistantClose);
    }
    if app.assistant_drawer_visible() {
        let drawer = assistant_drawer_area(layout.workspace);
        let inner = assistant_drawer_inner(drawer);
        if contains(inner, column, row) {
            return Some(ShellClickTarget::AssistantDrawer(inner));
        }
        if contains(layout.workspace, column, row) {
            return Some(ShellClickTarget::AssistantBackdrop);
        }
    }
    if contains(layout.workspace, column, row) {
        return Some(ShellClickTarget::Workspace(layout.workspace));
    }
    if contains(
        Rect::new(
            layout.footer.x,
            layout.footer.y,
            7.min(layout.footer.width),
            layout.footer.height,
        ),
        column,
        row,
    ) {
        return Some(if app.settings_visible() {
            ShellClickTarget::SettingsClose
        } else if app.help_visible() {
            ShellClickTarget::HelpClose
        } else {
            ShellClickTarget::Quit
        });
    }
    None
}

pub(crate) fn assistant_drawer_area(workspace: Rect) -> Rect {
    let preferred = workspace.width.saturating_mul(42) / 100;
    let width = preferred.max(52.min(workspace.width)).min(workspace.width);
    Rect::new(
        workspace
            .x
            .saturating_add(workspace.width.saturating_sub(width)),
        workspace.y,
        width,
        workspace.height,
    )
}

fn assistant_drawer_inner(drawer: Rect) -> Rect {
    Rect::new(
        drawer.x.saturating_add(1),
        drawer.y.saturating_add(1),
        drawer.width.saturating_sub(2),
        drawer.height.saturating_sub(2),
    )
}

pub(crate) fn assistant_close_area(workspace: Rect) -> Rect {
    let drawer = assistant_drawer_area(workspace);
    let width = drawer.width.min(13);
    Rect::new(
        drawer
            .x
            .saturating_add(drawer.width.saturating_sub(width + 1)),
        drawer.y,
        width,
        1.min(drawer.height),
    )
}

fn help_panel_area(area: Rect) -> Rect {
    let horizontal_margin = if area.width >= 80 { 2 } else { 0 };
    let vertical_margin = if area.height >= 20 { 1 } else { 0 };
    Rect::new(
        area.x.saturating_add(horizontal_margin),
        area.y.saturating_add(vertical_margin),
        area.width.saturating_sub(horizontal_margin * 2),
        area.height.saturating_sub(vertical_margin * 2),
    )
}

pub(crate) fn help_close_area(workspace: Rect) -> Rect {
    let panel = help_panel_area(workspace);
    let width = panel.width.min(13);
    Rect::new(
        panel
            .x
            .saturating_add(panel.width.saturating_sub(width + 1)),
        panel.y.saturating_add(1),
        width,
        1.min(panel.height),
    )
}

pub(crate) fn settings_close_area(workspace: Rect) -> Rect {
    let panel = help_panel_area(workspace);
    let width = panel.width.min(13);
    Rect::new(
        panel
            .x
            .saturating_add(panel.width.saturating_sub(width + 1)),
        panel.y.saturating_add(1),
        width,
        1.min(panel.height),
    )
}

pub(crate) fn settings_theme_previous_area(workspace: Rect) -> Rect {
    let panel = help_panel_area(workspace);
    Rect::new(
        panel.x.saturating_add(panel.width.saturating_sub(31)),
        panel.y.saturating_add(panel.height.saturating_sub(2)),
        14.min(panel.width),
        1.min(panel.height),
    )
}

pub(crate) fn settings_theme_next_area(workspace: Rect) -> Rect {
    let panel = help_panel_area(workspace);
    Rect::new(
        panel.x.saturating_add(panel.width.saturating_sub(16)),
        panel.y.saturating_add(panel.height.saturating_sub(2)),
        14.min(panel.width),
        1.min(panel.height),
    )
}

pub(crate) const fn contains(area: Rect, column: u16, row: u16) -> bool {
    column >= area.x
        && column < area.x.saturating_add(area.width)
        && row >= area.y
        && row < area.y.saturating_add(area.height)
}

pub(crate) fn is_primary_click(event: MouseEvent, area: Rect) -> bool {
    matches!(event.kind, MouseEventKind::Down(MouseButton::Left))
        && contains(area, event.column, event.row)
}

pub(crate) fn scroll_key(event: MouseEvent, area: Rect) -> Option<KeyEvent> {
    if !contains(area, event.column, event.row) {
        return None;
    }
    let code = match event.kind {
        MouseEventKind::ScrollUp => KeyCode::Up,
        MouseEventKind::ScrollDown => KeyCode::Down,
        _ => return None,
    };
    Some(KeyEvent::new(code, KeyModifiers::NONE))
}

/// Finds a one-line table row below a bordered header with one line of margin.
pub(crate) fn table_row_at(event: MouseEvent, area: Rect, row_count: usize) -> Option<usize> {
    if !is_primary_click(event, area) {
        return None;
    }
    let first_row = area.y.saturating_add(3);
    let index = usize::from(event.row.saturating_sub(first_row));
    (event.row >= first_row && index < row_count).then_some(index)
}

/// Finds a one-line list item immediately inside a bordered block.
pub(crate) fn list_row_at(event: MouseEvent, area: Rect, row_count: usize) -> Option<usize> {
    if !is_primary_click(event, area) {
        return None;
    }
    let first_row = area.y.saturating_add(1);
    let index = usize::from(event.row.saturating_sub(first_row));
    (event.row >= first_row && index < row_count).then_some(index)
}

fn uses_immersive_shell(app: &App) -> bool {
    app.input_mode() == InputMode::Navigation
        && app.workspaces.shell_chrome(app.active_workspace()) == ShellChrome::Immersive
        && !app.help_visible()
        && !app.settings_visible()
        && app.workspace_preset_preview().is_none()
        && !app.panel_focus()
        && app.shell_hints().is_none()
}

pub fn render(frame: &mut Frame, app: &App) {
    frame.render_widget(
        Block::new().style(Style::new().bg(theme::BG.into()).fg(theme::INK.into())),
        frame.area(),
    );
    if uses_immersive_shell(app) {
        app.workspaces
            .render(app.active_workspace(), frame, frame.area());
        if app.assistant_drawer_visible() {
            render_assistant_drawer(frame, frame.area(), app);
        }
        return;
    }
    let layout = ShellLayout::new(frame.area());

    chrome::render_header(frame, layout.header, app);
    chrome::render_navigation(frame, layout.navigation, app);
    app.workspaces
        .render(app.active_workspace, frame, layout.workspace);
    render_spatial_focus(frame, layout.workspace, app);
    chrome::render_footer(frame, layout.footer, app);
    if app.assistant_drawer_visible() {
        render_assistant_drawer(frame, layout.workspace, app);
    }
    if app.help_visible() {
        render_help(frame, layout.workspace, app);
    }
    if app.settings_visible() {
        render_settings(frame, layout.workspace, app);
    }
    if app.workspace_preset_preview().is_some() {
        render_workspace_preset_preview(frame, layout.workspace, app);
    }
    if app.shell_hints().is_some() {
        render_shell_hints(frame, layout, app);
    }
}

fn render_workspace_preset_preview(frame: &mut Frame, area: Rect, app: &App) {
    let Some(preview) = app.workspace_preset_preview() else {
        return;
    };
    let panel = help_panel_area(area);
    frame.render_widget(Clear, panel);
    let title = if preview.restoring_custom {
        " RESTORE CUSTOM WORKSPACE "
    } else {
        " WORKSPACE PRESET PREVIEW "
    };
    let block = Block::new()
        .borders(Borders::ALL)
        .border_style(theme::AMBER)
        .title(Line::from(vec![
            Span::styled(
                title,
                Style::new()
                    .bg(theme::AMBER.into())
                    .fg(theme::BG.into())
                    .bold(),
            ),
            Span::styled(
                format!(
                    " {} · V{} ",
                    preview.label.to_ascii_uppercase(),
                    preview.version
                ),
                theme::AMBER,
            ),
        ]));
    let inner = block.inner(panel);
    frame.render_widget(block, panel);
    let rows = Layout::vertical([
        Constraint::Length(4),
        Constraint::Min(5),
        Constraint::Length(3),
    ])
    .split(inner);
    let unavailable = if preview.unavailable.is_empty() {
        "All destinations are available.".to_owned()
    } else {
        format!(
            "Unavailable destinations will be skipped: {}",
            preview.unavailable.join(", ")
        )
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(
                preview.description.clone(),
                Style::new().fg(theme::INK.into()).bold(),
            ),
            Line::styled(unavailable, theme::MUTED),
            Line::styled(
                "Nothing changes until you confirm. Applying a role preserves one crash-safe custom return point.",
                theme::MUTED,
            ),
        ])
        .wrap(Wrap { trim: true }),
        rows[0],
    );

    let columns =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).split(rows[1]);
    let available_rows = usize::from(columns[0].height.saturating_sub(2)).max(1);
    frame.render_widget(
        Paragraph::new(workspace_order_preview_lines(
            &preview.current_order,
            available_rows,
        ))
        .block(components::terminal_block("NOW", "CURRENT ORDER")),
        columns[0],
    );
    frame.render_widget(
        Paragraph::new(workspace_order_preview_lines(
            &preview.proposed_order,
            available_rows,
        ))
        .block(components::terminal_block("NEXT", "PROPOSED ORDER")),
        columns[1],
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(
                " ENTER / Y  APPLY     ESC / N  CANCEL ",
                Style::new()
                    .bg(theme::CYAN.into())
                    .fg(theme::BG.into())
                    .bold(),
            ),
            Line::styled(
                if preview.restoring_custom {
                    "Confirmation consumes the saved return point."
                } else {
                    "Run PRESET RETURN to recover your original workspace order."
                },
                theme::MUTED,
            ),
        ]),
        rows[2],
    );
}

fn workspace_order_preview_lines(order: &[String], available_rows: usize) -> Vec<Line<'static>> {
    let visible = order.len().min(available_rows);
    let mut lines = order
        .iter()
        .take(visible)
        .enumerate()
        .map(|(index, workspace)| {
            Line::from(vec![
                Span::styled(format!("{:>2}  ", index + 1), theme::AMBER),
                Span::styled(workspace.to_ascii_uppercase(), theme::INK),
            ])
        })
        .collect::<Vec<_>>();
    if order.len() > visible && !lines.is_empty() {
        let remaining = order.len() - visible;
        let last = lines.len() - 1;
        lines[last] = Line::styled(format!("    +{remaining} MORE"), theme::MUTED);
    }
    lines
}

fn render_spatial_focus(frame: &mut Frame, area: Rect, app: &App) {
    if !app.panel_focus() {
        return;
    }
    let Some(action) = app.focused_workspace_action(area) else {
        return;
    };
    frame.buffer_mut().set_style(
        action.area,
        Style::new()
            .bg(theme::CYAN.into())
            .fg(theme::BG.into())
            .bold(),
    );
}

fn render_shell_hints(frame: &mut Frame, layout: ShellLayout, app: &App) {
    let Some((input, hints)) = app.shell_hints() else {
        return;
    };
    let badge_style = Style::new()
        .bg(theme::AMBER.into())
        .fg(theme::BG.into())
        .bold();

    let mut item_x = layout.navigation.x;
    for (index, item) in app.workspaces.navigation_items().enumerate() {
        let width = chrome::navigation_item_text(index, item).chars().count() as u16;
        if let Some(hint) = hints.iter().find(|hint| {
            hint.code.starts_with(input) && hint.target == ShellHintTarget::Workspace(item.id)
        }) {
            let badge_width = (hint.code.len() as u16).saturating_add(2).min(width);
            if item_x < layout.navigation.right() && badge_width > 0 {
                frame.render_widget(
                    Paragraph::new(format!(" {} ", hint.code)).style(badge_style),
                    Rect::new(
                        item_x,
                        layout.navigation.y,
                        badge_width.min(layout.navigation.right().saturating_sub(item_x)),
                        1,
                    ),
                );
            }
        }
        item_x = item_x.saturating_add(width);
    }

    if let Some(hint) = hints
        .iter()
        .find(|hint| hint.code.starts_with(input) && hint.target == ShellHintTarget::Command)
    {
        let width = (hint.code.len() as u16)
            .saturating_add(2)
            .min(layout.command.width);
        frame.render_widget(
            Paragraph::new(format!(" {} ", hint.code)).style(badge_style),
            Rect::new(
                layout.command.x.saturating_add(1),
                layout.command.y.saturating_add(1),
                width,
                1,
            ),
        );
    }

    for action in app.workspace_actions(layout.workspace, hints.len()) {
        let target = ShellHintTarget::WorkspaceAction {
            workspace: app.active_workspace(),
            action: action.id.clone(),
        };
        let Some(hint) = hints
            .iter()
            .find(|hint| hint.code.starts_with(input) && hint.target == target)
        else {
            continue;
        };
        let width = (hint.code.len() as u16)
            .saturating_add(2)
            .min(action.area.width);
        if width > 0 {
            frame.render_widget(
                Paragraph::new(format!(" {} ", hint.code)).style(badge_style),
                Rect::new(action.area.x, action.area.y, width, 1),
            );
        }
    }

    let utility_hints = hints
        .iter()
        .filter(|hint| {
            hint.code.starts_with(input)
                && matches!(
                    hint.target,
                    ShellHintTarget::Help | ShellHintTarget::Settings | ShellHintTarget::Quit
                )
        })
        .map(|hint| {
            let label = match hint.target {
                ShellHintTarget::Help => "HELP",
                ShellHintTarget::Settings => "SETUP",
                ShellHintTarget::Quit => "QUIT",
                _ => unreachable!("utility hints are filtered above"),
            };
            format!(" {} {label} ", hint.code)
        })
        .collect::<String>();
    if !utility_hints.is_empty() {
        let width = (utility_hints.chars().count() as u16).min(layout.footer.width);
        let area = Rect::new(
            layout.footer.right().saturating_sub(width),
            layout.footer.y,
            width,
            layout.footer.height,
        );
        frame.render_widget(Paragraph::new(utility_hints).style(badge_style), area);
    }
}

fn render_assistant_drawer(frame: &mut Frame, area: Rect, app: &App) {
    let Some(assistant) = app.assistant_workspace() else {
        return;
    };
    let drawer = assistant_drawer_area(area);
    frame.render_widget(Clear, drawer);
    let block = Block::new()
        .borders(Borders::ALL)
        .border_style(theme::CYAN)
        .title(Line::from(vec![
            Span::styled(
                " AI ",
                Style::new()
                    .bg(theme::CYAN.into())
                    .fg(theme::BG.into())
                    .bold(),
            ),
            Span::styled(" ASSISTANT DRAWER ", theme::CYAN),
        ]));
    let inner = block.inner(drawer);
    frame.render_widget(block, drawer);
    app.workspaces.render(assistant, frame, inner);
    frame.render_widget(
        Paragraph::new(" [ CLOSE ] ").style(
            Style::new()
                .bg(theme::CYAN.into())
                .fg(theme::BG.into())
                .bold(),
        ),
        assistant_close_area(area),
    );
}

fn render_help(frame: &mut Frame, area: Rect, app: &App) {
    let panel = help_panel_area(area);
    frame.render_widget(Clear, panel);
    let block = Block::new()
        .borders(Borders::ALL)
        .border_style(theme::CYAN)
        .title(Line::from(vec![
            Span::styled(
                " DISCOVER ",
                Style::new()
                    .bg(theme::CYAN.into())
                    .fg(theme::BG.into())
                    .bold(),
            ),
            Span::styled(" UNIFIED DESTINATION DIRECTORY ", theme::CYAN),
        ]));
    let inner = block.inner(panel);
    frame.render_widget(block, panel);

    let rows = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(6),
        Constraint::Length(2),
    ])
    .split(inner);
    let search_mode = if app.help_searching() {
        "SEARCHING"
    } else {
        "BROWSE"
    };
    let query = if app.help_query().is_empty() {
        "ALL DESTINATIONS".to_owned()
    } else {
        format!("QUERY · {}", app.help_query())
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled(
                    format!(" {search_mode} "),
                    Style::new()
                        .bg(theme::AMBER.into())
                        .fg(theme::BG.into())
                        .bold(),
                ),
                Span::styled(
                    format!("  {query}"),
                    Style::new().fg(theme::INK.into()).bold(),
                ),
            ]),
            Line::styled(
                "Commands, workspaces, saved views, and Launchpad objects share one exact router.",
                theme::MUTED,
            ),
        ]),
        rows[0],
    );

    let columns =
        Layout::horizontal([Constraint::Percentage(58), Constraint::Percentage(42)]).split(rows[1]);
    let items = app.help_items();
    let selected = app.help_selected_index().min(items.len().saturating_sub(1));
    let visible_rows = usize::from(columns[0].height.saturating_sub(2)).max(1);
    let offset = selected
        .saturating_sub(visible_rows.saturating_sub(1))
        .min(items.len().saturating_sub(visible_rows));
    let end = (offset + visible_rows).min(items.len());
    let item_lines = items[offset..end]
        .iter()
        .enumerate()
        .map(|(visible_index, item)| {
            let index = offset + visible_index;
            let marker = if index == selected { '›' } else { ' ' };
            let line = format!(
                "{marker} {:<4} {:<22} {}",
                item.kind.short_label(),
                item.label,
                item.owner
            );
            if index == selected {
                Line::styled(
                    line,
                    Style::new()
                        .bg(theme::CYAN.into())
                        .fg(theme::BG.into())
                        .bold(),
                )
            } else {
                Line::styled(line, theme::INK)
            }
        })
        .collect::<Vec<_>>();
    let item_lines = if item_lines.is_empty() {
        vec![Line::styled("NO LITERAL-TOKEN MATCHES", theme::MUTED)]
    } else {
        item_lines
    };
    let start = usize::from(!items.is_empty()).saturating_add(offset);
    let directory_title = format!("RESULTS {start}-{end} / {}", items.len());
    frame.render_widget(
        Paragraph::new(item_lines).block(components::terminal_block("FIND", &directory_title)),
        columns[0],
    );
    if app.help_details_visible() {
        let detail_lines = items.get(selected).map_or_else(Vec::new, |item| {
            let aliases = if item.aliases.is_empty() {
                "—".to_owned()
            } else {
                item.aliases.join(" · ")
            };
            let revision = item
                .revision
                .map(|revision| revision.to_string())
                .unwrap_or_else(|| "—".to_owned());
            vec![
                Line::from(vec![
                    Span::styled("TYPE         ", theme::AMBER),
                    Span::styled(item.kind.label(), Style::new().fg(theme::INK.into()).bold()),
                ]),
                Line::from(vec![
                    Span::styled("LABEL        ", theme::AMBER),
                    Span::raw(item.label.clone()),
                ]),
                Line::from(vec![
                    Span::styled("COMMAND      ", theme::AMBER),
                    Span::raw(item.command.clone()),
                ]),
                Line::from(vec![
                    Span::styled("OWNER        ", theme::AMBER),
                    Span::raw(item.owner.clone()),
                ]),
                Line::from(vec![
                    Span::styled("ALIASES      ", theme::AMBER),
                    Span::raw(aliases),
                ]),
                Line::from(vec![
                    Span::styled("REVISION     ", theme::AMBER),
                    Span::raw(revision),
                ]),
                Line::raw(""),
                Line::styled("INFORMATION", theme::AMBER),
                Line::raw(item.description.clone()),
            ]
        });
        frame.render_widget(
            Paragraph::new(detail_lines)
                .wrap(Wrap { trim: true })
                .block(components::terminal_block(
                    "INFO",
                    "DESTINATION INFORMATION",
                )),
            columns[1],
        );
    } else {
        frame.render_widget(
            Paragraph::new(vec![
                Line::styled("SEARCH AND BROWSE", theme::AMBER),
                Line::raw("/                  Search literal tokens"),
                Line::raw("↑/↓ or J/K         Select destination"),
                Line::raw("PgUp/PgDn/Wheel   Scroll results"),
                Line::raw("Enter              Inspect, then run"),
                Line::raw("X twice            Delete selected saved view"),
                Line::raw(""),
                Line::styled("OPEN AND NAVIGATE", theme::AMBER),
                Line::raw(format!(
                    "{:<11} Open command bar",
                    app.key_labels(&[ShellAction::OpenCommand])
                )),
                Line::raw("Esc+arrows Panel focus; Enter interacts"),
                Line::raw("F          Follow visible hints"),
                Line::raw("A          Toggle AI drawer"),
                Line::raw(format!(
                    "{:<11} Effective settings",
                    app.key_labels(&[ShellAction::Settings])
                )),
                Line::raw(format!(
                    "{:<11} Change color theme",
                    app.key_labels(&[ShellAction::NextTheme, ShellAction::PreviousTheme])
                )),
                Line::raw(""),
                Line::styled("USEFUL COMMANDS", theme::AMBER),
                Line::raw("DISCOVER <QUERY>     Open filtered directory"),
                Line::raw("HELP                 Open full directory"),
                Line::raw("PORT IMPORT <CSV>    Import positions"),
                Line::raw("SHEET IMPORT <CSV>   Replace active sheet"),
                Line::raw("CHART <SYMBOL>       Provider OHLC history"),
                Line::raw("NEWS                 Live headlines"),
                Line::raw("AI <REQUEST>         Ask the assistant"),
                Line::raw(""),
                Line::styled("Ctrl+C always quits the terminal.", theme::MUTED),
            ])
            .wrap(Wrap { trim: true })
            .block(components::terminal_block("KEY", "QUICK START")),
            columns[1],
        );
    }
    let footer = if app.help_delete_armed() {
        " DELETE ARMED FOR EXACT SAVED-VIEW REVISION · X CONFIRM · ESC CANCEL "
    } else if app.help_searching() {
        " TYPE TO FILTER · BACKSPACE EDIT · ^U CLEAR · ↑/↓ SELECT · ENTER INFO · ESC BROWSE "
    } else if app.help_details_visible() {
        " ↑/↓ SELECT · ENTER RUN DESTINATION · ESC BACK "
    } else {
        " / SEARCH · ↑/↓ SELECT · ENTER INFO · X DELETE SAVED VIEW · ESC CLOSE "
    };
    frame.render_widget(Paragraph::new(Line::styled(footer, theme::MUTED)), rows[2]);
    frame.render_widget(
        Paragraph::new(" [ CLOSE ] ").style(
            Style::new()
                .bg(theme::CYAN.into())
                .fg(theme::BG.into())
                .bold(),
        ),
        help_close_area(area),
    );
}

fn render_settings(frame: &mut Frame, area: Rect, app: &App) {
    let panel = help_panel_area(area);
    frame.render_widget(Clear, panel);
    let block = Block::new()
        .borders(Borders::ALL)
        .border_style(theme::CYAN)
        .title(Line::from(vec![
            Span::styled(
                " SETUP ",
                Style::new()
                    .bg(theme::CYAN.into())
                    .fg(theme::BG.into())
                    .bold(),
            ),
            Span::styled(" EFFECTIVE SETTINGS ", theme::CYAN),
        ]));
    let inner = block.inner(panel);
    frame.render_widget(block, panel);

    let rows = Layout::vertical([
        Constraint::Length(if app.settings_first_run() { 5 } else { 3 }),
        Constraint::Min(8),
        Constraint::Length(2),
    ])
    .split(inner);
    let intro = if app.settings_first_run() {
        vec![
            Line::styled(
                "WELCOME TO MARKET TERMINAL",
                Style::new().fg(theme::INK.into()).bold(),
            ),
            Line::styled(
                "This is a secret-free view of the providers and local inputs selected for this process.",
                theme::MUTED,
            ),
            Line::styled(
                "No-key Yahoo market data and live public feeds work immediately; configure an official provider when useful.",
                theme::MUTED,
            ),
        ]
    } else {
        vec![
            Line::styled(
                "EFFECTIVE STARTUP CONFIGURATION",
                Style::new().fg(theme::INK.into()).bold(),
            ),
            Line::styled(
                "Values are resolved at startup from exported variables and the ignored .env file.",
                theme::MUTED,
            ),
        ]
    };
    frame.render_widget(Paragraph::new(intro).wrap(Wrap { trim: true }), rows[0]);

    let columns =
        Layout::horizontal([Constraint::Percentage(54), Constraint::Percentage(46)]).split(rows[1]);
    let settings = app.runtime_settings();
    let credential_style = if settings.market_credentials == "MISSING" {
        theme::RED
    } else if settings.market_credentials == "CONFIGURED" {
        theme::GREEN
    } else {
        theme::AMBER
    };
    frame.render_widget(
        Paragraph::new(vec![
            setting_line("PRICE SOURCE", &settings.market_provider, theme::CYAN),
            setting_line(
                "CREDENTIALS",
                &settings.market_credentials,
                credential_style,
            ),
            setting_line(
                "QUOTE POLL",
                &format!("{} SECONDS", settings.quote_refresh_seconds),
                theme::INK,
            ),
            setting_line("WATCHLIST", &settings.watchlist, theme::INK),
            setting_line("MARKETS", &settings.market_symbols, theme::INK),
            setting_line("CHART SYMBOL", &settings.chart_symbol, theme::INK),
            setting_line("AI", &settings.ai_provider, theme::CYAN),
            setting_line("THEME", app.theme_name(), theme::CYAN),
            setting_line("KEYS", &settings.keybindings, theme::CYAN),
            setting_line("PORTFOLIO", &settings.portfolio_import, theme::INK),
            setting_line("NEWS", &settings.news_sources, theme::INK),
            setting_line("IRC", &settings.irc, theme::INK),
        ])
        .wrap(Wrap { trim: true })
        .block(components::terminal_block("CFG", "ACTIVE PROCESS")),
        columns[0],
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled("FAST START", theme::AMBER),
            Line::raw("1  cp .env.example .env"),
            Line::raw("2  Keep Yahoo, or choose Finnhub / Alpha Vantage / Alpaca"),
            Line::raw("3  Set credentials only for official API providers"),
            Line::raw("4  Run `codex login` for ChatGPT Pro"),
            Line::raw("5  Set watchlist / chart symbol / portfolio CSV paths"),
            Line::raw("6  Restart the terminal to apply provider changes"),
            Line::raw(""),
            Line::styled("USE NOW", theme::AMBER),
            Line::raw("PORT IMPORT \"~/Downloads/positions.csv\""),
            Line::raw("PORT IMPORT ACTIVITY \"~/Downloads/activity.csv\""),
            Line::raw(format!(
                "THEME NORD  ·  {} cycle themes",
                app.key_labels(&[ShellAction::NextTheme, ShellAction::PreviousTheme])
            )),
            Line::raw("KEYS  MARKET_TERMINAL_KEYBINDINGS in .env"),
            Line::raw(format!(
                "PRESETS  {}",
                theme::preset_names().collect::<Vec<_>>().join(" · ")
            )),
            Line::raw(format!(
                "HELP  ·  {} command and interaction guide",
                app.key_labels(&[ShellAction::Help])
            )),
            Line::raw(""),
            Line::styled(
                "Secrets are never displayed. CONFIGURED/MISSING is the only credential state shown.",
                theme::MUTED,
            ),
        ])
        .wrap(Wrap { trim: true })
        .block(components::terminal_block("GO", "CONFIGURE AND RESTART")),
        columns[1],
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!(
                    " ESC / {} ",
                    app.key_labels(&[ShellAction::Quit, ShellAction::Settings])
                ),
                Style::new()
                    .bg(theme::AMBER.into())
                    .fg(theme::BG.into())
                    .bold(),
            ),
            Span::styled(
                format!(
                    " CLOSE SETTINGS   ·   {} COMMAND   ·   {} HELP",
                    app.key_labels(&[ShellAction::OpenCommand]),
                    app.key_labels(&[ShellAction::Help])
                ),
                theme::MUTED,
            ),
        ])),
        rows[2],
    );
    frame.render_widget(
        Paragraph::new(" [ CLOSE ] ").style(
            Style::new()
                .bg(theme::CYAN.into())
                .fg(theme::BG.into())
                .bold(),
        ),
        settings_close_area(area),
    );
    frame.render_widget(
        Paragraph::new(" [ ◀ THEME ] ").style(
            Style::new()
                .bg(theme::AMBER.into())
                .fg(theme::BG.into())
                .bold(),
        ),
        settings_theme_previous_area(area),
    );
    frame.render_widget(
        Paragraph::new(" [ THEME ▶ ] ").style(
            Style::new()
                .bg(theme::AMBER.into())
                .fg(theme::BG.into())
                .bold(),
        ),
        settings_theme_next_area(area),
    );
}

fn setting_line<'a>(label: &'a str, value: &'a str, value_color: theme::ThemeColor) -> Line<'a> {
    Line::from(vec![
        Span::styled(format!("{label:<14}"), theme::AMBER),
        Span::styled(value, value_color),
    ])
}
