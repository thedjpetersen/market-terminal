use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, MouseEvent};
use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::Style,
    symbols,
    text::{Line, Span},
    widgets::{Axis, Block, Borders, Chart, Dataset, GraphType, Paragraph},
    Frame,
};

use crate::{
    app::{AppIntent, CommandInvocation, Workspace, WorkspaceDescriptor},
    ui::{
        components::{render_pairs, render_table, styled_row, terminal_block},
        is_primary_click,
        theme::{AMBER, CYAN, GREEN, MUTED},
    },
};

use super::{ResearchView, SecurityQuery, SecurityResearch, ID};

pub struct SecurityWorkspace {
    query: Arc<dyn SecurityQuery>,
    symbol: String,
    research_view: ResearchView,
    pending_intents: Vec<AppIntent>,
}

impl SecurityWorkspace {
    pub fn new(query: Arc<dyn SecurityQuery>) -> Self {
        Self {
            query,
            symbol: "AAPL US".into(),
            research_view: ResearchView::Financials,
            pending_intents: Vec::new(),
        }
    }

    fn select_view(&mut self, function: &str) -> bool {
        self.research_view = match function {
            "FA" | "FINANCIALS" => ResearchView::Financials,
            "EE" | "ESTIMATES" => ResearchView::Estimates,
            "OWN" | "OWNERSHIP" => ResearchView::Ownership,
            "FIL" | "FILINGS" => ResearchView::Filings,
            "RV" | "PEERS" => ResearchView::Peers,
            _ => return false,
        };
        true
    }

    fn ticker(&self) -> &str { self.symbol.split_whitespace().next().unwrap_or("AAPL") }
}

impl Workspace for SecurityWorkspace {
    fn descriptor(&self) -> WorkspaceDescriptor {
        WorkspaceDescriptor {
            id: ID,
            label: "SECURITY",
            hotkey: 's',
            commands: &["SEC", "AAPL", "EQUITY", "FA", "EE", "OWN", "FIL", "RV"],
        }
    }

    fn handle_command(&mut self, invocation: &CommandInvocation) -> bool {
        let is_view_command = self.select_view(&invocation.function);
        for argument in &invocation.args {
            if let Some(view) = argument.strip_prefix("--view=") {
                self.select_view(&view.to_ascii_uppercase());
            }
        }

        let mut subject = if matches!(invocation.function.as_str(), "SEC" | "EQUITY") {
            invocation.args.iter().filter(|arg| !arg.starts_with("--")).cloned().collect()
        } else if is_view_command {
            invocation.args.iter().filter(|arg| !arg.starts_with("--")).cloned().collect()
        } else {
            let mut tokens = vec![invocation.function.clone()];
            tokens.extend(invocation.args.iter().filter(|arg| !arg.starts_with("--")).cloned());
            tokens
        };
        if subject.last().is_some_and(|token| token.eq_ignore_ascii_case("EQUITY")) {
            subject.pop();
        }
        if !subject.is_empty() {
            self.symbol = subject.join(" ");
        }
        true
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Tab => self.research_view = self.research_view.next(),
            KeyCode::Char('1') => self.research_view = ResearchView::Financials,
            KeyCode::Char('2') => self.research_view = ResearchView::Estimates,
            KeyCode::Char('3') => self.research_view = ResearchView::Ownership,
            KeyCode::Char('4') => self.research_view = ResearchView::Filings,
            KeyCode::Char('5') => self.research_view = ResearchView::Peers,
            KeyCode::Char('n') => self.pending_intents.push(AppIntent::DispatchCommand {
                command: format!("NEWS --symbol={}", self.ticker()),
                origin: ID,
            }),
            KeyCode::Char('c') => self.pending_intents.push(AppIntent::DispatchCommand {
                command: format!("CHART {}", self.symbol),
                origin: ID,
            }),
            _ => return false,
        }
        true
    }

    fn handle_mouse(&mut self, event: MouseEvent, area: Rect) -> bool {
        let rows = Layout::vertical([Constraint::Length(4), Constraint::Min(12)]).split(area);
        let grid = Layout::horizontal([
            Constraint::Percentage(62),
            Constraint::Percentage(19),
            Constraint::Percentage(19),
        ])
        .split(rows[1]);
        let left = Layout::vertical([Constraint::Percentage(58), Constraint::Percentage(42)])
            .split(grid[0]);
        if is_primary_click(event, left[0]) {
            self.pending_intents.push(AppIntent::DispatchCommand {
                command: format!("CHART {}", self.symbol),
                origin: ID,
            });
            return true;
        }
        let research = Layout::vertical([Constraint::Length(1), Constraint::Min(3)])
            .split(left[1]);
        if !is_primary_click(event, research[0]) {
            return false;
        }
        let mut x = research[0].x;
        for (index, view) in ResearchView::ALL.into_iter().enumerate() {
            let width = format!(" {} {} ", index + 1, view.label()).chars().count() as u16;
            if event.column >= x && event.column < x.saturating_add(width) {
                self.research_view = view;
                return true;
            }
            x = x.saturating_add(width);
        }
        true
    }

    fn poll_intents(&mut self) -> Vec<AppIntent> { std::mem::take(&mut self.pending_intents) }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let snapshot = self.query.load_security(&self.symbol);
        let research = self.query.load_research(&self.symbol);
        let rows = Layout::vertical([Constraint::Length(4), Constraint::Min(12)]).split(area);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(format!(" {} · {}  ", snapshot.symbol, snapshot.name), AMBER),
                Span::styled(format!("{}  ", snapshot.last), Style::new().fg(CYAN).bold()),
                Span::styled(format!("{}  {}  ", snapshot.absolute_change, snapshot.percent_change), GREEN),
                Span::styled(snapshot.session_summary, MUTED),
            ]))
            .block(Block::new().borders(Borders::ALL).border_style(AMBER))
            .alignment(Alignment::Center),
            rows[0],
        );

        let grid = Layout::horizontal([
            Constraint::Percentage(62), Constraint::Percentage(19), Constraint::Percentage(19),
        ]).split(rows[1]);
        let left = Layout::vertical([Constraint::Percentage(58), Constraint::Percentage(42)]).split(grid[0]);
        let chart = Chart::new(vec![Dataset::default()
            .name(format!("{} {}", snapshot.symbol, snapshot.last))
            .marker(symbols::Marker::Braille)
            .graph_type(GraphType::Line)
            .style(CYAN)
            .data(snapshot.price_series)])
            .block(terminal_block("GP", "INTRADAY PRICE"))
            .x_axis(Axis::default().bounds([0., 100.]).labels(["09:30", "12:30", "16:00"]).style(MUTED))
            .y_axis(Axis::default().bounds([195., 207.]).labels(["195", "201", "207"]).style(AMBER));
        frame.render_widget(chart, left[0]);
        let research_areas = Layout::vertical([Constraint::Length(1), Constraint::Min(3)])
            .split(left[1]);
        let research_tabs = ResearchView::ALL.into_iter().enumerate().map(|(index, view)| {
            let style = if view == self.research_view {
                Style::new().bg(CYAN).fg(crate::ui::theme::BG).bold()
            } else {
                Style::new().fg(MUTED)
            };
            Span::styled(format!(" {} {} ", index + 1, view.label()), style)
        }).collect::<Vec<_>>();
        frame.render_widget(Paragraph::new(Line::from(research_tabs)), research_areas[0]);
        render_research(frame, research_areas[1], self.research_view, &research);
        render_pairs(frame, grid[1], "DES", "KEY STATISTICS", &[
            ["MARKET CAP", "$3.15T"], ["P/E (TTM)", "31.92X"], ["P/E (FY1)", "29.44X"],
            ["DIV YIELD", "0.49%"], ["52W RANGE", "164—237"], ["BETA", "1.21"],
        ]);
        render_pairs(frame, grid[2], "ANR", "ANALYSTS", &[
            ["BUY", "32"], ["HOLD", "12"], ["SELL", "3"], ["CONSENSUS", "4.31 / 5"],
            ["TARGET", "$224.62"], ["UPSIDE", "+9.41%"],
        ]);
    }
}

fn render_research(frame: &mut Frame, area: Rect, view: ResearchView, research: &SecurityResearch) {
    match view {
        ResearchView::Financials => render_table(
            frame, area, "FA", "FINANCIALS · 1-5/TAB SWITCH · N NEWS · C CHART",
            ["USD BN", "FY24", "FY25E", "FY26E"],
            vec![
                styled_row(["REVENUE", "391.0", "414.8", "438.1"]),
                styled_row(["EBITDA", "131.4", "140.7", "151.2"]),
                styled_row(["EPS", "6.57", "7.24", "7.93"]),
            ],
            [Constraint::Percentage(34), Constraint::Percentage(22), Constraint::Percentage(22), Constraint::Percentage(22)],
        ),
        ResearchView::Estimates => render_table(
            frame, area, "EE", "CONSENSUS ESTIMATES · RANGE",
            ["PERIOD", "REVENUE", "EPS", "HIGH", "LOW"],
            research.estimates.iter().map(|value| styled_row([
                value.period, value.revenue, value.eps, value.eps_high, value.eps_low,
            ])).collect(),
            [Constraint::Percentage(18), Constraint::Percentage(24), Constraint::Percentage(18), Constraint::Percentage(20), Constraint::Percentage(20)],
        ),
        ResearchView::Ownership => render_table(
            frame, area, "OWN", "TOP INSTITUTIONAL HOLDERS",
            ["MANAGER", "SHARES", "VALUE", "Q/Q"],
            research.owners.iter().map(|value| styled_row([
                value.manager, value.shares, value.value, value.quarterly_change,
            ])).collect(),
            [Constraint::Percentage(46), Constraint::Percentage(18), Constraint::Percentage(20), Constraint::Percentage(16)],
        ),
        ResearchView::Filings => render_table(
            frame, area, "FIL", "REGULATORY FILINGS",
            ["FILED", "FORM", "PERIOD", "DESCRIPTION", "ACCESSION"],
            research.filings.iter().map(|value| styled_row([
                value.filed, value.form, value.period, value.description, value.accession,
            ])).collect(),
            [Constraint::Percentage(15), Constraint::Percentage(9), Constraint::Percentage(15), Constraint::Percentage(27), Constraint::Percentage(34)],
        ),
        ResearchView::Peers => render_table(
            frame, area, "RV", "RELATIVE VALUE · CANONICAL INSTRUMENT LINKED",
            ["SYMBOL", "COMPANY", "P/E", "EV/EBITDA", "REV GR", "GM"],
            research.peers.iter().map(|value| styled_row([
                value.symbol, value.name, value.price_to_earnings, value.ev_to_ebitda,
                value.revenue_growth, value.gross_margin,
            ])).collect(),
            [Constraint::Percentage(13), Constraint::Percentage(25), Constraint::Percentage(13), Constraint::Percentage(18), Constraint::Percentage(16), Constraint::Percentage(15)],
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyModifiers, MouseButton, MouseEventKind};
    use crate::features::security::SecuritySnapshot;

    struct StubQuery;

    fn click(column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    impl SecurityQuery for StubQuery {
        fn load_security(&self, _symbol: &str) -> SecuritySnapshot {
            SecuritySnapshot {
                symbol: "AAPL US EQUITY", name: "APPLE", last: "205.30",
                absolute_change: "+1.72", percent_change: "+0.84%", session_summary: "OPEN",
                price_series: &[],
            }
        }
    }

    #[test]
    fn research_commands_change_view_without_replacing_symbol() {
        let mut workspace = SecurityWorkspace::new(Arc::new(StubQuery));
        workspace.handle_command(&CommandInvocation { function: "FIL".into(), args: vec![] });
        assert_eq!(workspace.research_view, ResearchView::Filings);
        assert_eq!(workspace.symbol, "AAPL US");
    }

    #[test]
    fn news_shortcut_emits_instrument_scoped_command() {
        let mut workspace = SecurityWorkspace::new(Arc::new(StubQuery));
        assert!(workspace.handle_key(KeyEvent::new(
            KeyCode::Char('n'),
            crossterm::event::KeyModifiers::NONE,
        )));
        assert_eq!(workspace.poll_intents(), vec![AppIntent::DispatchCommand {
            command: "NEWS --symbol=AAPL".into(), origin: ID,
        }]);
    }

    #[test]
    fn clicking_research_tabs_changes_the_active_view() {
        let mut workspace = SecurityWorkspace::new(Arc::new(StubQuery));
        let area = Rect::new(0, 0, 160, 40);
        let rows = Layout::vertical([Constraint::Length(4), Constraint::Min(12)]).split(area);
        let grid = Layout::horizontal([
            Constraint::Percentage(62),
            Constraint::Percentage(19),
            Constraint::Percentage(19),
        ])
        .split(rows[1]);
        let left = Layout::vertical([Constraint::Percentage(58), Constraint::Percentage(42)])
            .split(grid[0]);
        let tabs = Layout::vertical([Constraint::Length(1), Constraint::Min(3)])
            .split(left[1])[0];
        let second_tab = tabs.x.saturating_add(" 1 FA FINANCIALS ".chars().count() as u16);

        assert!(workspace.handle_mouse(click(second_tab, tabs.y), area));

        assert_eq!(workspace.research_view, ResearchView::Estimates);
    }
}
