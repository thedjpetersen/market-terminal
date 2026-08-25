use std::sync::Arc;

use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    text::Line,
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::{
    app::{Workspace, WorkspaceDescriptor},
    ui::{
        components::{render_pairs, render_table, styled_row},
        theme::{AMBER, CYAN, GREEN, MUTED},
    },
};

use super::{PortfolioQuery, ID};

pub struct PortfolioWorkspace {
    query: Arc<dyn PortfolioQuery>,
}

impl PortfolioWorkspace {
    pub fn new(query: Arc<dyn PortfolioQuery>) -> Self { Self { query } }
}

impl Workspace for PortfolioWorkspace {
    fn descriptor(&self) -> WorkspaceDescriptor {
        WorkspaceDescriptor { id: ID, label: "PORTFOLIO", hotkey: 'p', commands: &["PORT", "PORTFOLIO", "POSITIONS"] }
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let snapshot = self.query.load_portfolio();
        let rows = Layout::vertical([Constraint::Length(4), Constraint::Min(10)]).split(area);
        let kpis = Layout::horizontal([Constraint::Ratio(1, 4); 4]).split(rows[0]);
        for (index, (label, value)) in [
            ("NET ASSET VALUE", snapshot.net_asset_value),
            ("YTD RETURN", snapshot.ytd_return),
            ("AVAILABLE CASH", snapshot.available_cash),
            ("SHARPE", snapshot.sharpe),
        ].iter().enumerate() {
            frame.render_widget(
                Paragraph::new(vec![
                    Line::styled(*label, MUTED),
                    Line::styled(*value, if index == 1 { GREEN } else { CYAN }),
                ])
                .block(Block::new().borders(Borders::ALL).border_style(AMBER))
                .alignment(Alignment::Center),
                kpis[index],
            );
        }

        let columns = Layout::horizontal([
            Constraint::Percentage(62), Constraint::Percentage(20), Constraint::Percentage(18),
        ]).split(rows[1]);
        let position_rows = snapshot.positions.iter().map(|position| {
            styled_row([
                position.symbol, position.quantity, position.average_cost,
                position.market_value, position.pnl, position.weight,
            ])
        }).collect::<Vec<_>>();
        render_table(
            frame, columns[0], "PORT", "POSITIONS",
            ["SYMBOL", "QTY", "AVG COST", "MKT VALUE", "P&L", "WEIGHT"],
            position_rows,
            [Constraint::Percentage(13), Constraint::Percentage(13), Constraint::Percentage(18), Constraint::Percentage(22), Constraint::Percentage(18), Constraint::Percentage(16)],
        );
        render_pairs(frame, columns[1], "PMAP", "ALLOCATION", &[
            ["TECHNOLOGY", "59.2%"], ["BROAD MARKET", "10.6%"],
            ["CASH", "12.2%"], ["OTHER", "18.0%"],
        ]);
        let right = Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]).split(columns[2]);
        render_pairs(frame, right[0], "ATTR", "ATTRIBUTION", &[
            ["NVDA", "+5.48%"], ["META", "+2.14%"], ["MSFT", "+1.83%"], ["AMZN", "+1.62%"],
        ]);
        render_pairs(frame, right[1], "MARS", "RISK SCENARIOS", &[
            ["SPX −10%", "−$83,441"], ["NASDAQ −20%", "−$194,702"],
            ["RATES +100BP", "−$18,821"],
        ]);
    }
}
