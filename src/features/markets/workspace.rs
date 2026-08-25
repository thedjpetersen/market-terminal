use std::sync::Arc;

use ratatui::{
    layout::{Constraint, Layout, Rect},
    symbols,
    widgets::{Axis, Chart, Dataset, GraphType},
    Frame,
};

use crate::{
    app::{Workspace, WorkspaceDescriptor},
    ui::{
        components::{render_pairs, render_table, styled_row, terminal_block},
        theme::{AMBER, MUTED, YELLOW},
    },
};

use super::{MarketsQuery, ID};

pub struct MarketsWorkspace {
    query: Arc<dyn MarketsQuery>,
}

impl MarketsWorkspace {
    pub fn new(query: Arc<dyn MarketsQuery>) -> Self { Self { query } }
}

impl Workspace for MarketsWorkspace {
    fn descriptor(&self) -> WorkspaceDescriptor {
        WorkspaceDescriptor { id: ID, label: "MARKETS", hotkey: 'm', commands: &["MARKET", "WEI", "CURVE"] }
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let snapshot = self.query.load_markets();
        let columns = Layout::horizontal([Constraint::Percentage(66), Constraint::Percentage(34)]).split(area);
        let left = Layout::vertical([
            Constraint::Percentage(48), Constraint::Percentage(22), Constraint::Percentage(30),
        ]).split(columns[0]);
        let market_rows = snapshot.indices.iter().map(|index| {
            styled_row([index.name, index.symbol, index.last, index.net_change, index.percent_change])
        }).collect::<Vec<_>>();
        render_table(
            frame, left[0], "WEI", "WORLD EQUITY INDICES",
            ["INDEX", "SYMBOL", "LAST", "NET CHG", "% CHG"], market_rows,
            [Constraint::Percentage(27), Constraint::Percentage(14), Constraint::Percentage(23), Constraint::Percentage(19), Constraint::Percentage(17)],
        );
        render_pairs(frame, left[1], "XAM", "CROSS-ASSET MONITOR", &[
            ["US 10Y", "4.312  +3.2BP"], ["DXY", "104.72  −0.18%"],
            ["EUR/USD", "1.0837  +0.21%"], ["WTI", "78.42  +1.14%"],
            ["GOLD", "2,337.80  −0.36%"],
        ]);
        let curve = Chart::new(vec![Dataset::default()
            .marker(symbols::Marker::Braille)
            .graph_type(GraphType::Line)
            .style(YELLOW)
            .data(snapshot.treasury_curve)])
            .block(terminal_block("GC", "U.S. TREASURY CURVE"))
            .x_axis(Axis::default().bounds([0., 100.]).labels(["3M", "5Y", "10Y", "30Y"]).style(MUTED))
            .y_axis(Axis::default().bounds([4.2, 5.5]).labels(["4.2", "4.8", "5.5"]).style(AMBER));
        frame.render_widget(curve, left[2]);

        let right = Layout::vertical([
            Constraint::Percentage(45), Constraint::Percentage(27), Constraint::Percentage(28),
        ]).split(columns[1]);
        render_pairs(frame, right[0], "IMAP", "SECTOR PERFORMANCE", &[
            ["TECHNOLOGY", "+1.56%"], ["COMMUNICATION", "+1.11%"],
            ["CONS. DISC.", "+0.69%"], ["FINANCIALS", "+0.42%"],
            ["HEALTH CARE", "−0.15%"], ["UTILITIES", "−0.67%"], ["ENERGY", "−1.21%"],
        ]);
        render_pairs(frame, right[1], "MBR", "MARKET BREADTH", &[
            ["NYSE ADV / DEC", "2,181 / 812"], ["NEW HIGHS / LOWS", "224 / 31"],
            ["UP / DOWN VOLUME", "4.7X"], ["ABOVE 200 DMA", "62.8%"],
        ]);
        render_pairs(frame, right[2], "ECO", "ECONOMIC CALENDAR", &[
            ["08:30", "US INITIAL CLAIMS"], ["10:00", "US EXISTING HOMES"],
            ["14:00", "FED BEIGE BOOK"],
        ]);
    }
}
