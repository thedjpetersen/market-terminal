use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Style,
    symbols,
    text::{Line, Span},
    widgets::{Axis, Chart, Dataset, GraphType, LegendPosition, Paragraph},
    Frame,
};

use crate::{
    app::{Workspace, WorkspaceDescriptor},
    ui::{
        components::{render_pairs, render_table, styled_row, terminal_block},
        theme::{AMBER, BG, CYAN, GREEN, MUTED, YELLOW},
    },
};

use super::{OverviewQuery, ID};

pub struct OverviewWorkspace {
    query: Arc<dyn OverviewQuery>,
    selected_period: usize,
}

impl OverviewWorkspace {
    pub fn new(query: Arc<dyn OverviewQuery>) -> Self {
        Self { query, selected_period: 3 }
    }
}

impl Workspace for OverviewWorkspace {
    fn descriptor(&self) -> WorkspaceDescriptor {
        WorkspaceDescriptor {
            id: ID,
            label: "OVERVIEW",
            hotkey: 'g',
            commands: &["OVERVIEW", "HOME", "PERF"],
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        if let KeyCode::Char(character @ '1'..='8') = key.code {
            self.selected_period = character as usize - '1' as usize;
            true
        } else {
            false
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let snapshot = self.query.load_overview();
        let rows = Layout::vertical([
            Constraint::Length(2),
            Constraint::Percentage(52),
            Constraint::Length(7),
            Constraint::Min(8),
        ])
        .split(area);

        let mut periods = vec![Span::raw(" ")];
        for (index, period) in snapshot.periods.iter().enumerate() {
            let style = if index == self.selected_period {
                Style::new().bg(CYAN).fg(BG)
            } else {
                Style::new().fg(CYAN)
            };
            periods.push(Span::styled(format!(" {} {} ", index + 1, period), style));
        }
        periods.push(Span::styled("   ● MARKET OPEN  ", GREEN));
        periods.push(Span::styled("REGULAR SESSION", MUTED));
        frame.render_widget(Paragraph::new(Line::from(periods)), rows[0]);

        let datasets = vec![
            Dataset::default()
                .name("001 +17.1%")
                .marker(symbols::Marker::Braille)
                .graph_type(GraphType::Line)
                .style(YELLOW)
                .data(snapshot.primary_returns),
            Dataset::default()
                .name("002 +14.3%")
                .marker(symbols::Marker::Braille)
                .graph_type(GraphType::Line)
                .style(CYAN)
                .data(snapshot.comparison_returns),
        ];
        let chart = Chart::new(datasets)
            .block(terminal_block("PERF", "RETURNS — YTD (%)"))
            .x_axis(Axis::default().bounds([0., 100.]).labels(["02 JAN", "30 MAR", "25 JUN"]).style(MUTED))
            .y_axis(Axis::default().bounds([-3., 18.]).labels(["−3.0", "7.7", "17.1"]).style(AMBER))
            .legend_position(Some(LegendPosition::TopLeft));
        frame.render_widget(chart, rows[1]);

        let middle = Layout::horizontal([
            Constraint::Percentage(50),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
        ])
        .split(rows[2]);
        render_table(
            frame,
            middle[0],
            "RISK",
            "RISK & RETURN",
            ["PORTFOLIO", "RETURN", "MAX DD", "SHARPE"],
            vec![
                styled_row(["001", "+17.02%", "−6.3%", "2.79"]),
                styled_row(["002", "+13.87%", "−6.6%", "2.28"]),
            ],
            [
                Constraint::Percentage(30),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(20),
            ],
        );
        render_pairs(frame, middle[1], "ASST", "ASSET RETURNS", &[
            ["SPYY", "+13.97%"], ["IS3R", "+30.31%"], ["AVWS", "+22.05%"],
        ]);
        render_pairs(frame, middle[2], "WATC", "WATCHLIST", &[
            ["AVWC", "+16.72%"], ["DEGC", "+13.15%"], ["DEGT", "+12.75%"],
        ]);

        let bottom = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).split(rows[3]);
        render_pairs(frame, bottom[0], "PORT", "TOP HOLDINGS", &[
            ["NVIDIA CORPORATION", "4.8%"], ["APPLE INC.", "4.3%"],
            ["MICROSOFT CORPORATION", "2.6%"], ["AMAZON.COM INC.", "2.3%"],
            ["ALPHABET CLASS A", "2.0%"],
        ]);
        render_pairs(frame, bottom[1], "TOP", "NEWS & MOVERS", &[
            ["ADVANTEST CORP.", "+15.06%"], ["KIOXIA HOLDINGS", "+12.27%"],
            ["MS&AD INSURANCE", "−4.45%"], ["KOMATSU LTD.", "−3.56%"],
        ]);
    }
}
