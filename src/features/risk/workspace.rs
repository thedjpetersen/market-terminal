use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, MouseEvent};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Cell, Paragraph, Row, Table, Wrap},
    Frame,
};

use crate::{
    app::{AppIntent, CommandInvocation, Workspace, WorkspaceDescriptor},
    ui::{
        is_primary_click, scroll_key, table_row_at,
        theme::{AMBER, BG, CYAN, GREEN, INK, MUTED, RED, YELLOW},
    },
};

use super::{domain, RiskQuery, RiskSnapshot, ID};

pub struct RiskWorkspace {
    query: Arc<dyn RiskQuery>,
    selected: usize,
    status: String,
    pending_intents: Vec<AppIntent>,
}

impl RiskWorkspace {
    pub fn new(query: Arc<dyn RiskQuery>) -> Self {
        Self {
            query,
            selected: 0,
            status: "VERSIONED PORTFOLIO RISK".to_owned(),
            pending_intents: Vec::new(),
        }
    }

    fn move_selection(&mut self, delta: isize) {
        let count = self
            .query
            .load_risk()
            .map(|snapshot| snapshot.positions.len())
            .unwrap_or_default();
        if count == 0 {
            self.selected = 0;
        } else {
            self.selected = self.selected.saturating_add_signed(delta).min(count - 1);
        }
    }

    fn open_selected(&mut self) -> bool {
        let Ok(snapshot) = self.query.load_risk() else {
            return false;
        };
        let Some(position) = snapshot.positions.get(self.selected) else {
            return false;
        };
        if position.cash {
            self.status = format!("{} IS CASH · NO SECURITY RESEARCH", position.currency);
            return true;
        }
        self.pending_intents.push(AppIntent::DispatchCommand {
            command: format!("SEC {}", position.symbol),
            origin: ID,
        });
        self.status = format!("OPENING {} SECURITY RESEARCH", position.symbol);
        true
    }

    fn refresh_status(&mut self) {
        self.status = match self.query.load_risk() {
            Ok(snapshot) => format!(
                "RECOMPUTED {} POSITIONS · {}",
                snapshot.positions.len(),
                snapshot.input_version
            ),
            Err(error) => error.to_string(),
        };
    }
}

impl Workspace for RiskWorkspace {
    fn descriptor(&self) -> WorkspaceDescriptor {
        WorkspaceDescriptor {
            id: ID,
            label: "RISK",
            hotkey: '\0',
            commands: &["RISK"],
        }
    }

    fn is_favorite(&self) -> bool {
        true
    }

    fn handle_command(&mut self, _invocation: &CommandInvocation) -> bool {
        self.refresh_status();
        true
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_selection(-1);
                true
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_selection(1);
                true
            }
            KeyCode::Enter | KeyCode::Char('s') => self.open_selected(),
            KeyCode::Char('r') => {
                self.refresh_status();
                true
            }
            _ => false,
        }
    }

    fn handle_mouse(&mut self, event: MouseEvent, area: Rect) -> bool {
        let areas = risk_layout(area);
        if is_primary_click(event, areas.header) {
            self.refresh_status();
            return true;
        }
        if let Ok(snapshot) = self.query.load_risk() {
            if let Some(index) = table_row_at(event, areas.table, snapshot.positions.len()) {
                self.selected = index;
                return self.open_selected();
            }
        }
        if is_primary_click(event, areas.side) {
            self.status = "RISK RESULTS ARE PER-CURRENCY · NO INVENTED FX CONVERSION".to_owned();
            return true;
        }
        if is_primary_click(event, areas.footer) {
            let controls = [
                (" ↑↓/JK SELECT  ", None),
                ("ENTER/S SECURITY  ", Some(KeyCode::Enter)),
                ("R RECOMPUTE  ", Some(KeyCode::Char('r'))),
            ];
            let mut x = areas.footer.x;
            for (label, key) in controls {
                let width = label.chars().count() as u16;
                if event.column >= x && event.column < x.saturating_add(width) {
                    return key.is_none_or(|key| {
                        self.handle_key(KeyEvent::new(key, crossterm::event::KeyModifiers::NONE))
                    });
                }
                x = x.saturating_add(width);
            }
            return true;
        }
        if let Some(key) = scroll_key(event, areas.table) {
            return self.handle_key(key);
        }
        false
    }

    fn poll_intents(&mut self) -> Vec<AppIntent> {
        std::mem::take(&mut self.pending_intents)
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        match self.query.load_risk() {
            Ok(snapshot) => self.render_snapshot(frame, area, &snapshot),
            Err(error) => frame.render_widget(
                Paragraph::new(vec![
                    Line::styled("RISK UNAVAILABLE", RED),
                    Line::raw(""),
                    Line::styled(error.to_string(), YELLOW),
                    Line::raw(""),
                    Line::styled("IMPORT A PORTFOLIO WITH PORT IMPORT <FILE.CSV>", CYAN),
                ])
                .wrap(Wrap { trim: true })
                .block(crate::ui::components::terminal_block(
                    "RISK",
                    "VERSIONED PORTFOLIO INPUT REQUIRED",
                )),
                area,
            ),
        }
    }
}

impl RiskWorkspace {
    fn render_snapshot(&self, frame: &mut Frame, area: Rect, snapshot: &RiskSnapshot) {
        let areas = risk_layout(area);
        render_header(frame, areas.header, snapshot, &self.status);
        render_positions(frame, areas.table, snapshot, self.selected);
        render_provenance(frame, areas.side, snapshot);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" ↑↓/JK ", AMBER),
                Span::styled("SELECT  ", MUTED),
                Span::styled("ENTER/S ", AMBER),
                Span::styled("SECURITY  ", MUTED),
                Span::styled("R ", AMBER),
                Span::styled("RECOMPUTE  ", MUTED),
                Span::styled(
                    "POINT-IN-TIME · PER-CURRENCY · MISSING PRICES EXPLICIT",
                    YELLOW,
                ),
            ])),
            areas.footer,
        );
    }
}

#[derive(Debug, Clone, Copy)]
struct RiskLayout {
    header: Rect,
    table: Rect,
    side: Rect,
    footer: Rect,
}

fn risk_layout(area: Rect) -> RiskLayout {
    let rows = Layout::vertical([
        Constraint::Length(4),
        Constraint::Min(10),
        Constraint::Length(2),
    ])
    .split(area);
    let body =
        Layout::horizontal([Constraint::Percentage(74), Constraint::Percentage(26)]).split(rows[1]);
    RiskLayout {
        header: rows[0],
        table: body[0],
        side: body[1],
        footer: rows[2],
    }
}

fn render_header(frame: &mut Frame, area: Rect, snapshot: &RiskSnapshot, status: &str) {
    let kpis = Layout::horizontal([Constraint::Ratio(1, 4); 4]).split(area);
    let values = [
        (
            "PRICED / UNPRICED",
            format!(
                "{} / {}",
                snapshot.priced_position_count(),
                snapshot.unpriced_position_count()
            ),
        ),
        ("CURRENCIES", snapshot.currencies.len().to_string()),
        ("LARGEST NON-CASH", snapshot.largest_position_label()),
        ("NON-CASH -10%", snapshot.scenario_label()),
    ];
    for (index, (label, value)) in values.into_iter().enumerate() {
        frame.render_widget(
            Paragraph::new(vec![
                Line::styled(label, MUTED),
                Line::styled(value, if index == 3 { RED } else { INK }),
                Line::styled(status, YELLOW),
            ])
            .block(crate::ui::components::terminal_block("RISK", label)),
            kpis[index],
        );
    }
}

fn render_positions(frame: &mut Frame, area: Rect, snapshot: &RiskSnapshot, selected: usize) {
    let header = Row::new([
        "ACCOUNT · SYMBOL · CCY",
        "MARKET VALUE",
        "CCY WEIGHT",
        "-10% CHANGE",
        "QUALITY",
    ])
    .style(Style::new().fg(AMBER.into()).bold())
    .bottom_margin(1);
    let rows = snapshot
        .positions
        .iter()
        .enumerate()
        .map(|(index, position)| {
            let values = [
                format!(
                    "{} · {} · {}",
                    position.account, position.symbol, position.currency
                ),
                position
                    .market_value
                    .map(domain::format_money)
                    .unwrap_or_else(|| "UNPRICED".to_owned()),
                position
                    .currency_weight_bps
                    .map(domain::format_bps)
                    .unwrap_or_else(|| "—".to_owned()),
                position
                    .scenario_change
                    .map(domain::format_money)
                    .unwrap_or_else(|| "—".to_owned()),
                if position.market_value.is_some() {
                    "PRICED"
                } else {
                    "MISSING"
                }
                .to_owned(),
            ];
            Row::new(values.into_iter().map(|value| {
                let style = crate::ui::theme::value(&value);
                Cell::from(value).style(style)
            }))
            .style(if index == selected {
                Style::new().bg(CYAN.into()).fg(BG.into()).bold()
            } else {
                Style::new()
            })
        });
    frame.render_widget(
        Table::new(
            rows,
            [
                Constraint::Percentage(31),
                Constraint::Percentage(21),
                Constraint::Percentage(16),
                Constraint::Percentage(19),
                Constraint::Percentage(13),
            ],
        )
        .header(header)
        .column_spacing(1)
        .block(crate::ui::components::terminal_block(
            "EXPOSURE",
            "POSITION CONCENTRATION",
        )),
        area,
    );
}

fn render_provenance(frame: &mut Frame, area: Rect, snapshot: &RiskSnapshot) {
    let mut lines = vec![
        Line::styled("INPUT", AMBER),
        Line::styled(snapshot.source.clone(), INK),
        Line::styled(snapshot.as_of.clone(), MUTED),
        Line::styled(snapshot.input_version.clone(), CYAN),
        Line::raw(""),
        Line::styled("CURRENCY SCENARIOS", AMBER),
    ];
    for currency in &snapshot.currencies {
        lines.push(Line::styled(
            format!(
                "{} NAV {} · CASH {}",
                currency.currency,
                domain::format_money(currency.priced_nav),
                domain::format_money(currency.available_cash)
            ),
            INK,
        ));
        lines.push(Line::styled(
            format!(
                "  -10% {} · MAX {} · {} UNPRICED",
                domain::format_money(currency.scenario_change),
                currency
                    .largest_non_cash_weight_bps
                    .map(domain::format_bps)
                    .unwrap_or_else(|| "—".to_owned()),
                currency.unpriced_positions
            ),
            if currency.scenario_change.minor_units() < 0 {
                RED
            } else {
                GREEN
            },
        ));
    }
    lines.extend([
        Line::raw(""),
        Line::styled("METHODOLOGY", AMBER),
        Line::styled(snapshot.methodology.clone(), MUTED),
        Line::raw(""),
        Line::styled("DISCLOSURES", AMBER),
    ]);
    for disclosure in snapshot.disclosures.iter().take(6) {
        lines.push(Line::styled(format!("• {disclosure}"), MUTED));
    }
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: true }).block(
            crate::ui::components::terminal_block("METHOD", "VERSIONED INPUT"),
        ),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::risk::{
        calculate_risk, RiskCurrencyInput, RiskError, RiskInput, RiskPositionInput,
        SCENARIO_SHOCK_BPS,
    };
    use crate::foundation::{Currency, InstrumentId, Money};
    use crate::{bootstrap, runtime};
    use crossterm::event::{KeyModifiers, MouseButton, MouseEventKind};
    use ratatui::{backend::TestBackend, Terminal};
    use std::sync::Arc;

    struct TestRisk(RiskSnapshot);

    impl RiskQuery for TestRisk {
        fn load_risk(&self) -> Result<RiskSnapshot, RiskError> {
            Ok(self.0.clone())
        }
    }

    fn query() -> Arc<dyn RiskQuery> {
        let usd = Currency::new("USD").unwrap();
        let snapshot = calculate_risk(RiskInput {
            positions: vec![RiskPositionInput {
                instrument_id: InstrumentId::new("us:xnas:aapl"),
                account: "ACCOUNT 1".to_owned(),
                symbol: "AAPL".to_owned(),
                currency: usd,
                market_value: Some(Money::from_minor_units(100_000, usd)),
                cash: false,
            }],
            currencies: vec![RiskCurrencyInput {
                currency: usd,
                priced_nav: Money::from_minor_units(100_000, usd),
                available_cash: Money::from_minor_units(0, usd),
                priced_positions: 1,
                unpriced_positions: 0,
            }],
            source: "TEST".to_owned(),
            as_of: "2026-08-27T20:00:00Z".to_owned(),
            input_version: "INPUT-1".to_owned(),
            disclosures: Vec::new(),
        })
        .unwrap();
        Arc::new(TestRisk(snapshot))
    }

    #[test]
    fn risk_command_is_exact_and_selected_security_opens() {
        let mut workspace = RiskWorkspace::new(query());

        assert_eq!(workspace.descriptor().commands, &["RISK"]);
        assert!(workspace.is_favorite());
        assert!(workspace.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));
        assert_eq!(
            workspace.poll_intents(),
            vec![AppIntent::DispatchCommand {
                command: "SEC AAPL".to_owned(),
                origin: ID,
            }]
        );
    }

    #[test]
    fn table_header_sidebar_and_footer_are_clickable() {
        let mut workspace = RiskWorkspace::new(query());
        let area = Rect::new(0, 0, 120, 36);
        let layout = risk_layout(area);
        let click = |x, y| MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        };

        assert!(workspace.handle_mouse(click(layout.header.x + 1, layout.header.y + 1), area));
        assert!(workspace.handle_mouse(click(layout.table.x + 2, layout.table.y + 3), area));
        assert!(!workspace.poll_intents().is_empty());
        assert!(workspace.handle_mouse(click(layout.side.x + 1, layout.side.y + 1), area));
        assert!(workspace.status.contains("PER-CURRENCY"));
        assert!(workspace.handle_mouse(click(layout.footer.x + 45, layout.footer.y), area));
    }

    #[test]
    fn scenario_constant_is_explicitly_negative_ten_percent() {
        assert_eq!(SCENARIO_SHOCK_BPS, -1_000);
    }

    #[test]
    fn application_command_opens_risk_with_standard_clickable_chrome() {
        let mut app = bootstrap::demo_app();
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for character in "RISK".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(app.active_workspace(), ID);
        assert_eq!(
            app.workspaces.shell_chrome(ID),
            crate::app::ShellChrome::Standard
        );
        let mut terminal = Terminal::new(TestBackend::new(160, 48)).unwrap();
        terminal.draw(|frame| runtime::render(frame, &app)).unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("MARKET TERMINAL"));
        assert!(rendered.contains("POSITION CONCENTRATION"));
    }
}
