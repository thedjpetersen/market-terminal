use std::{path::PathBuf, sync::Arc};

use crossterm::event::MouseEvent;
use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    text::Line,
    widgets::{Block, Borders, Cell, Paragraph, Row},
    Frame,
};

use crate::{
    app::{AppIntent, CommandInvocation, Workspace, WorkspaceDescriptor},
    ui::{
        components::{render_table, terminal_block},
        table_row_at,
        theme::{self, AMBER, CYAN, GREEN, INK, MUTED, YELLOW},
    },
};

use super::{PortfolioRepository, ID};

pub struct PortfolioWorkspace {
    query: Arc<dyn PortfolioRepository>,
    pending_intents: Vec<AppIntent>,
    status: String,
}

impl PortfolioWorkspace {
    pub fn new(query: Arc<dyn PortfolioRepository>) -> Self {
        let status = query.load_portfolio().source;
        Self { query, pending_intents: Vec::new(), status }
    }
}

impl Workspace for PortfolioWorkspace {
    fn descriptor(&self) -> WorkspaceDescriptor {
        WorkspaceDescriptor { id: ID, label: "PORTFOLIO", hotkey: 'p', commands: &["PORT", "PORTFOLIO", "POSITIONS"] }
    }

    fn handle_command(&mut self, invocation: &CommandInvocation) -> bool {
        let Some(operation) = invocation.args.first().map(|value| value.to_ascii_uppercase()) else {
            return true;
        };
        match operation.as_str() {
            "IMPORT" => {
                let raw_path = invocation.args.get(1..).unwrap_or_default().join(" ");
                if raw_path.is_empty() {
                    self.status = "IMPORT REQUIRES A CSV PATH · PORT IMPORT <FILE.CSV>".to_owned();
                    return true;
                }
                let path = expand_home(&raw_path);
                self.status = match self.query.import_csv(&path) {
                    Ok(snapshot) => format!(
                        "IMPORTED {} POSITIONS · {}",
                        snapshot.positions.len(), snapshot.source
                    ),
                    Err(error) => format!("IMPORT ERROR · {error}"),
                };
            }
            "RELOAD" | "REFRESH" => {
                self.status = match self.query.reload() {
                    Ok(snapshot) => format!(
                        "RELOADED {} POSITIONS · {}",
                        snapshot.positions.len(), snapshot.source
                    ),
                    Err(error) => format!("RELOAD ERROR · {error}"),
                };
            }
            _ => {}
        }
        true
    }

    fn handle_mouse(&mut self, event: MouseEvent, area: Rect) -> bool {
        let snapshot = self.query.load_portfolio();
        let rows = Layout::vertical([Constraint::Length(4), Constraint::Min(10)]).split(area);
        let columns = Layout::horizontal([Constraint::Percentage(76), Constraint::Percentage(24)])
        .split(rows[1]);
        let Some(index) = table_row_at(event, columns[0], snapshot.positions.len()) else {
            return false;
        };
        let symbol = &snapshot.positions[index].symbol;
        self.pending_intents.push(AppIntent::DispatchCommand {
            command: format!("SEC {symbol} US"),
            origin: ID,
        });
        true
    }

    fn poll_intents(&mut self) -> Vec<AppIntent> { std::mem::take(&mut self.pending_intents) }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let snapshot = self.query.load_portfolio();
        let rows = Layout::vertical([Constraint::Length(4), Constraint::Min(10)]).split(area);
        let kpis = Layout::horizontal([Constraint::Ratio(1, 4); 4]).split(rows[0]);
        for (index, (label, value)) in [
            ("NET ASSET VALUE", snapshot.net_asset_value.as_str()),
            ("YTD RETURN", snapshot.ytd_return.as_str()),
            ("AVAILABLE CASH", snapshot.available_cash.as_str()),
            ("SHARPE", snapshot.sharpe.as_str()),
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

        let columns = Layout::horizontal([Constraint::Percentage(76), Constraint::Percentage(24)])
            .split(rows[1]);
        let position_rows = snapshot.positions.iter().map(|position| {
            Row::new([
                position.symbol.clone(),
                position.quantity.clone(),
                position.average_cost.clone(),
                position.market_value.clone(),
                position.pnl.clone(),
                position.weight.clone(),
            ].into_iter().map(|value| {
                let style = theme::value(&value);
                Cell::from(value).style(style)
            }))
        }).collect::<Vec<_>>();
        render_table(
            frame, columns[0], "PORT", "POSITIONS",
            ["SYMBOL", "QTY", "AVG COST", "MKT VALUE", "P&L", "WEIGHT"],
            position_rows,
            [Constraint::Percentage(13), Constraint::Percentage(13), Constraint::Percentage(18), Constraint::Percentage(22), Constraint::Percentage(18), Constraint::Percentage(16)],
        );
        frame.render_widget(
            Paragraph::new(vec![
                Line::styled("SOURCE", AMBER),
                Line::styled(snapshot.source, INK),
                Line::raw(""),
                Line::styled("AS OF", AMBER),
                Line::styled(snapshot.as_of, MUTED),
                Line::raw(""),
                Line::styled(&self.status, YELLOW),
                Line::raw(""),
                Line::styled("PORT IMPORT <FILE.CSV>", CYAN),
                Line::styled("PORT RELOAD", CYAN),
                Line::raw(""),
                Line::styled("CLICK A POSITION TO OPEN SECURITY", MUTED),
            ])
            .block(terminal_block("SRC", "IMPORTED PORTFOLIO")),
            columns[1],
        );
    }
}

fn expand_home(path: &str) -> PathBuf {
    if path == "~" {
        return std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(path));
    }
    if let Some(relative) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(relative);
        }
    }
    PathBuf::from(path)
}
