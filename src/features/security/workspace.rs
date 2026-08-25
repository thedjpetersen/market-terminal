use std::sync::Arc;

use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Style, Stylize},
    symbols,
    text::{Line, Span},
    widgets::{Axis, Block, Borders, Chart, Dataset, GraphType, Paragraph},
    Frame,
};

use crate::{
    app::{Workspace, WorkspaceDescriptor},
    ui::{
        components::{render_pairs, render_table, styled_row, terminal_block},
        theme::{AMBER, CYAN, GREEN, MUTED},
    },
};

use super::{SecurityQuery, ID};

pub struct SecurityWorkspace {
    query: Arc<dyn SecurityQuery>,
    symbol: String,
}

impl SecurityWorkspace {
    pub fn new(query: Arc<dyn SecurityQuery>) -> Self {
        Self { query, symbol: "AAPL US".into() }
    }
}

impl Workspace for SecurityWorkspace {
    fn descriptor(&self) -> WorkspaceDescriptor {
        WorkspaceDescriptor { id: ID, label: "SECURITY", hotkey: 's', commands: &["SEC", "AAPL", "EQUITY"] }
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let snapshot = self.query.load_security(&self.symbol);
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
        render_table(
            frame, left[1], "FA", "FINANCIAL SNAPSHOT",
            ["USD BN", "FY24", "FY25E", "FY26E"],
            vec![
                styled_row(["REVENUE", "391.0", "414.8", "438.1"]),
                styled_row(["EBITDA", "131.4", "140.7", "151.2"]),
                styled_row(["EPS", "6.57", "7.24", "7.93"]),
            ],
            [Constraint::Percentage(34), Constraint::Percentage(22), Constraint::Percentage(22), Constraint::Percentage(22)],
        );
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
