use std::{cell::Cell as StateCell, path::PathBuf, sync::Arc};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent};
use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Wrap},
    Frame,
};

use crate::{
    app::{
        AppIntent, CommandInvocation, ViewRestoreReport, ViewValue, Workspace, WorkspaceAction,
        WorkspaceDescriptor, WorkspaceViewState,
    },
    ui::{
        components::{render_table, terminal_block},
        is_primary_click, scroll_key, table_row_at,
        theme::{self, AMBER, BG, CYAN, GREEN, INK, MUTED, RED, YELLOW},
    },
};

use super::{
    format_money, PortfolioActivityLedger, PortfolioAttributionSnapshot,
    PortfolioContributionSnapshot, PortfolioPerformanceSnapshot, PortfolioRealizedGainSnapshot,
    PortfolioRepository, PortfolioSnapshot, PortfolioTaxLotSnapshot, PortfolioTradeLedger, ID,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PortfolioView {
    Positions,
    Activity,
    Performance,
    TaxLots,
    RealizedGains,
    Trades,
    Contribution,
    Attribution,
}

impl PortfolioView {
    const ALL: [Self; 8] = [
        Self::Positions,
        Self::Activity,
        Self::Performance,
        Self::TaxLots,
        Self::RealizedGains,
        Self::Trades,
        Self::Contribution,
        Self::Attribution,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::Positions => "POSITIONS",
            Self::Activity => "ACTIVITY",
            Self::Performance => "PERFORMANCE",
            Self::TaxLots => "LOTS",
            Self::RealizedGains => "REALIZED",
            Self::Trades => "TRADES",
            Self::Contribution => "CONTRIB",
            Self::Attribution => "ATTRIB",
        }
    }

    const fn action_id(self) -> &'static str {
        match self {
            Self::Positions => "view:positions",
            Self::Activity => "view:activity",
            Self::Performance => "view:performance",
            Self::TaxLots => "view:lots",
            Self::RealizedGains => "view:realized",
            Self::Trades => "view:trades",
            Self::Contribution => "view:contribution",
            Self::Attribution => "view:attribution",
        }
    }

    const fn action_key(self) -> &'static str {
        match self {
            Self::Positions => "positions",
            Self::Activity => "activity",
            Self::Performance => "performance",
            Self::TaxLots => "lots",
            Self::RealizedGains => "realized",
            Self::Trades => "trades",
            Self::Contribution => "contribution",
            Self::Attribution => "attribution",
        }
    }

    fn from_action_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|view| view.action_id() == id)
    }

    fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|view| view.action_key() == value)
    }

    fn offset(self, delta: isize) -> Self {
        let index = Self::ALL
            .iter()
            .position(|candidate| *candidate == self)
            .unwrap_or_default();
        let next = (index as isize + delta).rem_euclid(Self::ALL.len() as isize) as usize;
        Self::ALL[next]
    }
}

pub struct PortfolioWorkspace {
    query: Arc<dyn PortfolioRepository>,
    view: PortfolioView,
    selected: usize,
    viewport_top: StateCell<usize>,
    viewport_rows: StateCell<usize>,
    pending_intents: Vec<AppIntent>,
    status: String,
}

impl PortfolioWorkspace {
    pub fn new(query: Arc<dyn PortfolioRepository>) -> Self {
        let status = query.load_portfolio().source;
        Self {
            query,
            view: PortfolioView::Positions,
            selected: 0,
            viewport_top: StateCell::new(0),
            viewport_rows: StateCell::new(0),
            pending_intents: Vec::new(),
            status,
        }
    }

    fn select_view(&mut self, view: PortfolioView) {
        self.view = view;
        self.viewport_top.set(0);
        self.clamp_selection();
        self.status = match view {
            PortfolioView::Positions => self.query.load_portfolio().source,
            PortfolioView::Activity => self.query.load_activity().source,
            PortfolioView::Performance => self.query.load_performance().source,
            PortfolioView::TaxLots => self.query.load_tax_lots().source,
            PortfolioView::RealizedGains => self.query.load_realized_gains().source,
            PortfolioView::Trades => self.query.load_trades().source,
            PortfolioView::Contribution => self.query.load_contribution().source,
            PortfolioView::Attribution => self.query.load_attribution().source,
        };
    }

    fn selection_count(&self) -> usize {
        match self.view {
            PortfolioView::Positions => self.query.load_portfolio().positions.len(),
            PortfolioView::Activity => self.query.load_activity().entries.len(),
            PortfolioView::Performance => self.query.load_performance().series.len(),
            PortfolioView::TaxLots => self.query.load_tax_lots().lots.len(),
            PortfolioView::RealizedGains => self.query.load_realized_gains().lots.len(),
            PortfolioView::Trades => self.query.load_trades().executions.len(),
            PortfolioView::Contribution => self.query.load_contribution().rows.len(),
            PortfolioView::Attribution => self.query.load_attribution().rows.len(),
        }
    }

    fn clamp_selection(&mut self) {
        self.selected = self
            .selection_count()
            .checked_sub(1)
            .map_or(0, |last| self.selected.min(last));
        self.reveal_selection();
    }

    fn move_selection(&mut self, delta: isize) {
        let count = self.selection_count();
        self.selected = if count == 0 {
            0
        } else {
            self.selected.saturating_add_signed(delta).min(count - 1)
        };
        self.reveal_selection();
    }

    fn reveal_selection(&self) {
        let capacity = self.viewport_rows.get();
        if capacity == 0 {
            return;
        }
        let mut top = self.viewport_top.get();
        if self.selected < top {
            top = self.selected;
        } else if self.selected >= top.saturating_add(capacity) {
            top = self.selected.saturating_add(1).saturating_sub(capacity);
        }
        let maximum = self.selection_count().saturating_sub(capacity.max(1));
        self.viewport_top.set(top.min(maximum));
    }

    fn update_viewport(&self, area: Rect) -> (usize, usize) {
        let capacity = usize::from(area.height.saturating_sub(4));
        self.viewport_rows.set(capacity);
        self.reveal_selection();
        (self.viewport_top.get(), capacity)
    }

    fn row_identity(&self, index: usize) -> Option<String> {
        match self.view {
            PortfolioView::Positions => {
                self.query
                    .load_portfolio()
                    .positions
                    .get(index)
                    .map(|position| {
                        format!(
                            "position|{}|{}|{}",
                            position.account_id.as_str(),
                            position.instrument_id.as_str(),
                            position.currency
                        )
                    })
            }
            PortfolioView::Activity => self
                .query
                .load_activity()
                .entries
                .get(index)
                .map(|entry| format!("activity|{}", entry.activity_id)),
            PortfolioView::Performance => self
                .query
                .load_performance()
                .series
                .get(index)
                .map(|series| format!("performance|{}", series.currency)),
            PortfolioView::TaxLots => self
                .query
                .load_tax_lots()
                .lots
                .get(index)
                .map(|lot| format!("lot|{}", lot.lot_id)),
            PortfolioView::RealizedGains => self
                .query
                .load_realized_gains()
                .lots
                .get(index)
                .map(|lot| format!("realized|{}", lot.lot_id)),
            PortfolioView::Trades => self
                .query
                .load_trades()
                .executions
                .get(index)
                .map(|execution| format!("execution|{}", execution.execution_id)),
            PortfolioView::Contribution => {
                self.query.load_contribution().rows.get(index).map(|row| {
                    format!(
                        "contribution|{}|{}|{}",
                        row.account_id.as_str(),
                        row.instrument_id.as_str(),
                        row.currency
                    )
                })
            }
            PortfolioView::Attribution => {
                self.query.load_attribution().rows.get(index).map(|row| {
                    format!(
                        "attribution|{}|{}|{}",
                        row.account_id.as_str(),
                        row.instrument_id.as_str(),
                        row.currency
                    )
                })
            }
        }
    }

    fn row_index(&self, identity: &str) -> Option<usize> {
        match self.view {
            PortfolioView::Positions => {
                self.query
                    .load_portfolio()
                    .positions
                    .iter()
                    .position(|position| {
                        format!(
                            "position|{}|{}|{}",
                            position.account_id.as_str(),
                            position.instrument_id.as_str(),
                            position.currency
                        ) == identity
                    })
            }
            PortfolioView::Activity => self
                .query
                .load_activity()
                .entries
                .iter()
                .position(|entry| format!("activity|{}", entry.activity_id) == identity),
            PortfolioView::Performance => self
                .query
                .load_performance()
                .series
                .iter()
                .position(|series| format!("performance|{}", series.currency) == identity),
            PortfolioView::TaxLots => self
                .query
                .load_tax_lots()
                .lots
                .iter()
                .position(|lot| format!("lot|{}", lot.lot_id) == identity),
            PortfolioView::RealizedGains => self
                .query
                .load_realized_gains()
                .lots
                .iter()
                .position(|lot| format!("realized|{}", lot.lot_id) == identity),
            PortfolioView::Trades => self
                .query
                .load_trades()
                .executions
                .iter()
                .position(|execution| format!("execution|{}", execution.execution_id) == identity),
            PortfolioView::Contribution => {
                self.query.load_contribution().rows.iter().position(|row| {
                    format!(
                        "contribution|{}|{}|{}",
                        row.account_id.as_str(),
                        row.instrument_id.as_str(),
                        row.currency
                    ) == identity
                })
            }
            PortfolioView::Attribution => {
                self.query.load_attribution().rows.iter().position(|row| {
                    format!(
                        "attribution|{}|{}|{}",
                        row.account_id.as_str(),
                        row.instrument_id.as_str(),
                        row.currency
                    ) == identity
                })
            }
        }
    }

    fn symbol_at(&self, index: usize) -> Option<String> {
        let symbol = match self.view {
            PortfolioView::Positions => self
                .query
                .load_portfolio()
                .positions
                .get(index)
                .and_then(|position| (!position.cash).then(|| position.symbol.clone())),
            PortfolioView::Activity => self
                .query
                .load_activity()
                .entries
                .get(index)
                .and_then(|entry| entry.symbol.clone()),
            PortfolioView::Performance => None,
            PortfolioView::TaxLots => self
                .query
                .load_tax_lots()
                .lots
                .get(index)
                .map(|lot| lot.symbol.clone()),
            PortfolioView::RealizedGains => self
                .query
                .load_realized_gains()
                .lots
                .get(index)
                .map(|lot| lot.symbol.clone()),
            PortfolioView::Trades => self
                .query
                .load_trades()
                .executions
                .get(index)
                .map(|execution| execution.symbol.clone()),
            PortfolioView::Contribution => self
                .query
                .load_contribution()
                .rows
                .get(index)
                .map(|row| row.symbol.clone()),
            PortfolioView::Attribution => self
                .query
                .load_attribution()
                .rows
                .get(index)
                .map(|row| row.symbol.clone()),
        };
        symbol
    }

    fn action_rows(&self, start: usize, len: usize) -> Vec<(usize, String, String)> {
        match self.view {
            PortfolioView::Positions => self
                .query
                .load_portfolio()
                .positions
                .into_iter()
                .enumerate()
                .skip(start)
                .take(len)
                .filter_map(|(index, position)| {
                    (!position.cash).then(|| {
                        let identity = format!(
                            "position|{}|{}|{}",
                            position.account_id.as_str(),
                            position.instrument_id.as_str(),
                            position.currency
                        );
                        (index, position.symbol, identity)
                    })
                })
                .collect(),
            PortfolioView::Activity => self
                .query
                .load_activity()
                .entries
                .into_iter()
                .enumerate()
                .skip(start)
                .take(len)
                .filter_map(|(index, entry)| {
                    entry.symbol.map(|symbol| {
                        let identity = format!("activity|{}", entry.activity_id);
                        (index, symbol, identity)
                    })
                })
                .collect(),
            PortfolioView::Performance => Vec::new(),
            PortfolioView::TaxLots => self
                .query
                .load_tax_lots()
                .lots
                .into_iter()
                .enumerate()
                .skip(start)
                .take(len)
                .map(|(index, lot)| {
                    let identity = format!("lot|{}", lot.lot_id);
                    (index, lot.symbol, identity)
                })
                .collect(),
            PortfolioView::RealizedGains => self
                .query
                .load_realized_gains()
                .lots
                .into_iter()
                .enumerate()
                .skip(start)
                .take(len)
                .map(|(index, lot)| {
                    let identity = format!("realized|{}", lot.lot_id);
                    (index, lot.symbol, identity)
                })
                .collect(),
            PortfolioView::Trades => self
                .query
                .load_trades()
                .executions
                .into_iter()
                .enumerate()
                .skip(start)
                .take(len)
                .map(|(index, execution)| {
                    let identity = format!("execution|{}", execution.execution_id);
                    (index, execution.symbol, identity)
                })
                .collect(),
            PortfolioView::Contribution => self
                .query
                .load_contribution()
                .rows
                .into_iter()
                .enumerate()
                .skip(start)
                .take(len)
                .map(|(index, row)| {
                    let identity = format!(
                        "contribution|{}|{}|{}",
                        row.account_id.as_str(),
                        row.instrument_id.as_str(),
                        row.currency
                    );
                    (index, row.symbol, identity)
                })
                .collect(),
            PortfolioView::Attribution => self
                .query
                .load_attribution()
                .rows
                .into_iter()
                .enumerate()
                .skip(start)
                .take(len)
                .map(|(index, row)| {
                    let identity = format!(
                        "attribution|{}|{}|{}",
                        row.account_id.as_str(),
                        row.instrument_id.as_str(),
                        row.currency
                    );
                    (index, row.symbol, identity)
                })
                .collect(),
        }
    }

    fn open_selected(&mut self) -> bool {
        let symbol = self.symbol_at(self.selected);
        let Some(symbol) = symbol else {
            self.status = "SELECTED ROW HAS NO SECURITY TO OPEN".to_owned();
            return self.selection_count() > 0;
        };
        self.pending_intents.push(AppIntent::DispatchCommand {
            command: format!("SEC {symbol} US"),
            origin: ID,
        });
        self.status = format!("OPENING {symbol} SECURITY RESEARCH");
        true
    }

    fn refresh_anchors(&self, view: PortfolioView) -> Option<(Option<String>, Option<String>)> {
        (self.view == view).then(|| {
            (
                self.row_identity(self.selected),
                self.row_identity(self.viewport_top.get()),
            )
        })
    }

    fn restore_refresh_anchors(&mut self, anchors: Option<(Option<String>, Option<String>)>) {
        if let Some((selected, top)) = anchors {
            self.selected = selected
                .as_deref()
                .and_then(|identity| self.row_index(identity))
                .unwrap_or_default();
            self.viewport_top.set(
                top.as_deref()
                    .and_then(|identity| self.row_index(identity))
                    .unwrap_or_default(),
            );
        }
        self.clamp_selection();
    }

    fn import_positions(&mut self, args: &[String]) {
        let raw_path = args.join(" ");
        if raw_path.is_empty() {
            self.status = "IMPORT REQUIRES A CSV PATH · PORT IMPORT <FILE.CSV>".to_owned();
            return;
        }
        self.status = match self.query.import_csv(&expand_home(&raw_path)) {
            Ok(snapshot) => format!(
                "IMPORTED {} POSITIONS · {}",
                snapshot.positions.len(),
                snapshot.source
            ),
            Err(error) => format!("IMPORT ERROR · {error}"),
        };
        self.view = PortfolioView::Positions;
        self.viewport_top.set(0);
        self.clamp_selection();
    }

    fn import_activity(&mut self, args: &[String]) {
        let raw_path = args.join(" ");
        if raw_path.is_empty() {
            self.status =
                "ACTIVITY IMPORT REQUIRES A CSV PATH · PORT IMPORT ACTIVITY <FILE.CSV>".to_owned();
            return;
        }
        self.status = match self.query.import_activity_csv(&expand_home(&raw_path)) {
            Ok(activity) => format!(
                "IMPORTED {} ACTIVITY ROWS · {}",
                activity.entries.len(),
                activity.source
            ),
            Err(error) => format!("ACTIVITY IMPORT ERROR · {error}"),
        };
        self.view = PortfolioView::Activity;
        self.viewport_top.set(0);
        self.clamp_selection();
    }

    fn import_performance(&mut self, args: &[String]) {
        let raw_path = args.join(" ");
        if raw_path.is_empty() {
            self.status =
                "PERFORMANCE IMPORT REQUIRES A CSV PATH · PORT IMPORT PERFORMANCE <FILE.CSV>"
                    .to_owned();
            return;
        }
        self.status = match self.query.import_performance_csv(&expand_home(&raw_path)) {
            Ok(performance) => format!(
                "IMPORTED {} VALUATION POINTS · {}",
                performance.point_count(),
                performance.source
            ),
            Err(error) => format!("PERFORMANCE IMPORT ERROR · {error}"),
        };
        self.view = PortfolioView::Performance;
        self.viewport_top.set(0);
        self.clamp_selection();
    }

    fn import_tax_lots(&mut self, args: &[String]) {
        let raw_path = args.join(" ");
        if raw_path.is_empty() {
            self.status =
                "TAX-LOT IMPORT REQUIRES A CSV PATH · PORT IMPORT LOTS <FILE.CSV>".to_owned();
            return;
        }
        self.status = match self.query.import_tax_lots_csv(&expand_home(&raw_path)) {
            Ok(snapshot) => format!(
                "IMPORTED {} OPEN TAX LOTS · {}",
                snapshot.lots.len(),
                snapshot.source
            ),
            Err(error) => format!("TAX-LOT IMPORT ERROR · {error}"),
        };
        self.view = PortfolioView::TaxLots;
        self.viewport_top.set(0);
        self.clamp_selection();
    }

    fn import_realized_gains(&mut self, args: &[String]) {
        let raw_path = args.join(" ");
        if raw_path.is_empty() {
            self.status = "CLOSED-LOT IMPORT REQUIRES A CSV PATH · PORT IMPORT REALIZED <FILE.CSV>"
                .to_owned();
            return;
        }
        self.status = match self
            .query
            .import_realized_gains_csv(&expand_home(&raw_path))
        {
            Ok(snapshot) => format!(
                "IMPORTED {} CLOSED LOTS · {}",
                snapshot.lots.len(),
                snapshot.source
            ),
            Err(error) => format!("CLOSED-LOT IMPORT ERROR · {error}"),
        };
        self.view = PortfolioView::RealizedGains;
        self.viewport_top.set(0);
        self.clamp_selection();
    }

    fn import_trades(&mut self, args: &[String]) {
        let raw_path = args.join(" ");
        if raw_path.is_empty() {
            self.status =
                "TRADE IMPORT REQUIRES A CSV PATH · PORT IMPORT TRADES <FILE.CSV>".to_owned();
            return;
        }
        self.status = match self.query.import_trades_csv(&expand_home(&raw_path)) {
            Ok(ledger) => format!(
                "IMPORTED {} EXECUTIONS · {}",
                ledger.executions.len(),
                ledger.source
            ),
            Err(error) => format!("TRADE IMPORT ERROR · {error}"),
        };
        self.view = PortfolioView::Trades;
        self.viewport_top.set(0);
        self.clamp_selection();
    }

    fn import_contribution(&mut self, args: &[String]) {
        let raw_path = args.join(" ");
        if raw_path.is_empty() {
            self.status =
                "CONTRIBUTION IMPORT REQUIRES A CSV PATH · PORT IMPORT CONTRIBUTION <FILE.CSV>"
                    .to_owned();
            return;
        }
        self.status = match self.query.import_contribution_csv(&expand_home(&raw_path)) {
            Ok(snapshot) => format!(
                "IMPORTED {} CONTRIBUTION ROWS · {}",
                snapshot.rows.len(),
                snapshot.source
            ),
            Err(error) => format!("CONTRIBUTION IMPORT ERROR · {error}"),
        };
        self.view = PortfolioView::Contribution;
        self.viewport_top.set(0);
        self.clamp_selection();
    }

    fn import_attribution(&mut self, args: &[String]) {
        let raw_path = args.join(" ");
        if raw_path.is_empty() {
            self.status =
                "ATTRIBUTION IMPORT REQUIRES A CSV PATH · PORT IMPORT ATTRIBUTION <FILE.CSV>"
                    .to_owned();
            return;
        }
        self.status = match self.query.import_attribution_csv(&expand_home(&raw_path)) {
            Ok(snapshot) => format!(
                "IMPORTED {} LINKED ATTRIBUTION ROWS · {}",
                snapshot.rows.len(),
                snapshot.source
            ),
            Err(error) => format!("ATTRIBUTION IMPORT ERROR · {error}"),
        };
        self.view = PortfolioView::Attribution;
        self.viewport_top.set(0);
        self.clamp_selection();
    }

    fn reload_positions(&mut self) {
        let anchors = self.refresh_anchors(PortfolioView::Positions);
        self.status = match self.query.reload() {
            Ok(snapshot) => format!(
                "RELOADED {} POSITIONS · {}",
                snapshot.positions.len(),
                snapshot.source
            ),
            Err(error) => format!("RELOAD ERROR · {error}"),
        };
        self.restore_refresh_anchors(anchors);
    }

    fn reload_activity(&mut self) {
        let anchors = self.refresh_anchors(PortfolioView::Activity);
        self.status = match self.query.reload_activity() {
            Ok(activity) => format!(
                "RELOADED {} ACTIVITY ROWS · {}",
                activity.entries.len(),
                activity.source
            ),
            Err(error) => format!("ACTIVITY RELOAD ERROR · {error}"),
        };
        self.restore_refresh_anchors(anchors);
    }

    fn reload_performance(&mut self) {
        let anchors = self.refresh_anchors(PortfolioView::Performance);
        self.status = match self.query.reload_performance() {
            Ok(performance) => format!(
                "RELOADED {} VALUATION POINTS · {}",
                performance.point_count(),
                performance.source
            ),
            Err(error) => format!("PERFORMANCE RELOAD ERROR · {error}"),
        };
        self.restore_refresh_anchors(anchors);
    }

    fn reload_tax_lots(&mut self) {
        let anchors = self.refresh_anchors(PortfolioView::TaxLots);
        self.status = match self.query.reload_tax_lots() {
            Ok(snapshot) => format!(
                "RELOADED {} OPEN TAX LOTS · {}",
                snapshot.lots.len(),
                snapshot.source
            ),
            Err(error) => format!("TAX-LOT RELOAD ERROR · {error}"),
        };
        self.restore_refresh_anchors(anchors);
    }

    fn reload_realized_gains(&mut self) {
        let anchors = self.refresh_anchors(PortfolioView::RealizedGains);
        self.status = match self.query.reload_realized_gains() {
            Ok(snapshot) => format!(
                "RELOADED {} CLOSED LOTS · {}",
                snapshot.lots.len(),
                snapshot.source
            ),
            Err(error) => format!("CLOSED-LOT RELOAD ERROR · {error}"),
        };
        self.restore_refresh_anchors(anchors);
    }

    fn reload_trades(&mut self) {
        let anchors = self.refresh_anchors(PortfolioView::Trades);
        self.status = match self.query.reload_trades() {
            Ok(ledger) => format!(
                "RELOADED {} EXECUTIONS · {}",
                ledger.executions.len(),
                ledger.source
            ),
            Err(error) => format!("TRADE RELOAD ERROR · {error}"),
        };
        self.restore_refresh_anchors(anchors);
    }

    fn reload_contribution(&mut self) {
        let anchors = self.refresh_anchors(PortfolioView::Contribution);
        self.status = match self.query.reload_contribution() {
            Ok(snapshot) => format!(
                "RELOADED {} CONTRIBUTION ROWS · {}",
                snapshot.rows.len(),
                snapshot.source
            ),
            Err(error) => format!("CONTRIBUTION RELOAD ERROR · {error}"),
        };
        self.restore_refresh_anchors(anchors);
    }

    fn reload_attribution(&mut self) {
        let anchors = self.refresh_anchors(PortfolioView::Attribution);
        self.status = match self.query.reload_attribution() {
            Ok(snapshot) => format!(
                "RELOADED {} LINKED ATTRIBUTION ROWS · {}",
                snapshot.rows.len(),
                snapshot.source
            ),
            Err(error) => format!("ATTRIBUTION RELOAD ERROR · {error}"),
        };
        self.restore_refresh_anchors(anchors);
    }

    fn reload_current(&mut self) {
        match self.view {
            PortfolioView::Positions => self.reload_positions(),
            PortfolioView::Activity => self.reload_activity(),
            PortfolioView::Performance => self.reload_performance(),
            PortfolioView::TaxLots => self.reload_tax_lots(),
            PortfolioView::RealizedGains => self.reload_realized_gains(),
            PortfolioView::Trades => self.reload_trades(),
            PortfolioView::Contribution => self.reload_contribution(),
            PortfolioView::Attribution => self.reload_attribution(),
        }
    }
}

impl Workspace for PortfolioWorkspace {
    fn descriptor(&self) -> WorkspaceDescriptor {
        WorkspaceDescriptor {
            id: ID,
            label: "PORTFOLIO",
            hotkey: 'p',
            commands: &[
                "PORT",
                "PORTFOLIO",
                "POSITIONS",
                "ACTIVITY",
                "TRANSACTIONS",
                "PERFORMANCE",
                "LOTS",
                "TAXLOTS",
                "REALIZED",
                "CLOSEDLOTS",
                "TRADES",
                "FILLS",
                "CONTRIBUTION",
                "ATTRIBUTION",
            ],
        }
    }

    fn handle_command(&mut self, invocation: &CommandInvocation) -> bool {
        match invocation.function.as_str() {
            "POSITIONS" => self.select_view(PortfolioView::Positions),
            "ACTIVITY" | "TRANSACTIONS" => self.select_view(PortfolioView::Activity),
            "PERFORMANCE" => self.select_view(PortfolioView::Performance),
            "LOTS" | "TAXLOTS" => self.select_view(PortfolioView::TaxLots),
            "REALIZED" | "CLOSEDLOTS" => self.select_view(PortfolioView::RealizedGains),
            "TRADES" | "FILLS" => self.select_view(PortfolioView::Trades),
            "CONTRIBUTION" => self.select_view(PortfolioView::Contribution),
            "ATTRIBUTION" => self.select_view(PortfolioView::Attribution),
            _ => {}
        }
        let Some(operation) = invocation
            .args
            .first()
            .map(|value| value.to_ascii_uppercase())
        else {
            return true;
        };
        match operation.as_str() {
            "POSITIONS" => self.select_view(PortfolioView::Positions),
            "ACTIVITY" | "TRANSACTIONS" | "LEDGER" => self.select_view(PortfolioView::Activity),
            "PERFORMANCE" | "PERF" => self.select_view(PortfolioView::Performance),
            "LOTS" | "TAXLOTS" | "TAX-LOTS" => self.select_view(PortfolioView::TaxLots),
            "REALIZED" | "GAINS" | "CLOSEDLOTS" | "CLOSED-LOTS" => {
                self.select_view(PortfolioView::RealizedGains)
            }
            "TRADES" | "FILLS" | "EXECUTIONS" => self.select_view(PortfolioView::Trades),
            "CONTRIBUTION" | "CONTRIB" => self.select_view(PortfolioView::Contribution),
            "ATTRIBUTION" | "ATTRIB" | "LINKED" => self.select_view(PortfolioView::Attribution),
            "IMPORT" => {
                let target = invocation
                    .args
                    .get(1)
                    .map(|value| value.to_ascii_uppercase());
                if target
                    .as_deref()
                    .is_some_and(|value| matches!(value, "ACTIVITY" | "TRANSACTIONS" | "LEDGER"))
                {
                    self.import_activity(invocation.args.get(2..).unwrap_or_default());
                } else if target
                    .as_deref()
                    .is_some_and(|value| matches!(value, "PERFORMANCE" | "PERF" | "VALUATIONS"))
                {
                    self.import_performance(invocation.args.get(2..).unwrap_or_default());
                } else if target
                    .as_deref()
                    .is_some_and(|value| matches!(value, "LOTS" | "TAXLOTS" | "TAX-LOTS"))
                {
                    self.import_tax_lots(invocation.args.get(2..).unwrap_or_default());
                } else if target.as_deref().is_some_and(|value| {
                    matches!(value, "REALIZED" | "GAINS" | "CLOSEDLOTS" | "CLOSED-LOTS")
                }) {
                    self.import_realized_gains(invocation.args.get(2..).unwrap_or_default());
                } else if target
                    .as_deref()
                    .is_some_and(|value| matches!(value, "TRADES" | "FILLS" | "EXECUTIONS"))
                {
                    self.import_trades(invocation.args.get(2..).unwrap_or_default());
                } else if target
                    .as_deref()
                    .is_some_and(|value| matches!(value, "ATTRIBUTION" | "ATTRIB" | "LINKED"))
                {
                    self.import_attribution(invocation.args.get(2..).unwrap_or_default());
                } else if target
                    .as_deref()
                    .is_some_and(|value| matches!(value, "CONTRIBUTION" | "CONTRIB"))
                {
                    self.import_contribution(invocation.args.get(2..).unwrap_or_default());
                } else {
                    self.import_positions(invocation.args.get(1..).unwrap_or_default());
                }
            }
            "RELOAD" | "REFRESH" => {
                let target = invocation
                    .args
                    .get(1)
                    .map(|value| value.to_ascii_uppercase());
                if target
                    .as_deref()
                    .is_some_and(|value| matches!(value, "ACTIVITY" | "TRANSACTIONS" | "LEDGER"))
                {
                    self.reload_activity();
                } else if target
                    .as_deref()
                    .is_some_and(|value| matches!(value, "PERFORMANCE" | "PERF" | "VALUATIONS"))
                {
                    self.reload_performance();
                } else if target
                    .as_deref()
                    .is_some_and(|value| matches!(value, "LOTS" | "TAXLOTS" | "TAX-LOTS"))
                {
                    self.reload_tax_lots();
                } else if target.as_deref().is_some_and(|value| {
                    matches!(value, "REALIZED" | "GAINS" | "CLOSEDLOTS" | "CLOSED-LOTS")
                }) {
                    self.reload_realized_gains();
                } else if target
                    .as_deref()
                    .is_some_and(|value| matches!(value, "TRADES" | "FILLS" | "EXECUTIONS"))
                {
                    self.reload_trades();
                } else if target
                    .as_deref()
                    .is_some_and(|value| matches!(value, "ATTRIBUTION" | "ATTRIB" | "LINKED"))
                {
                    self.reload_attribution();
                } else if target
                    .as_deref()
                    .is_some_and(|value| matches!(value, "CONTRIBUTION" | "CONTRIB"))
                {
                    self.reload_contribution();
                } else {
                    self.reload_positions();
                }
            }
            _ => {}
        }
        true
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => {
                self.select_view(self.view.offset(1));
                true
            }
            KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => {
                self.select_view(self.view.offset(-1));
                true
            }
            KeyCode::Char('1') => {
                self.select_view(PortfolioView::Positions);
                true
            }
            KeyCode::Char('2') => {
                self.select_view(PortfolioView::Activity);
                true
            }
            KeyCode::Char('3') => {
                self.select_view(PortfolioView::Performance);
                true
            }
            KeyCode::Char('4') => {
                self.select_view(PortfolioView::TaxLots);
                true
            }
            KeyCode::Char('5') => {
                self.select_view(PortfolioView::RealizedGains);
                true
            }
            KeyCode::Char('6') => {
                self.select_view(PortfolioView::Trades);
                true
            }
            KeyCode::Char('7') => {
                self.select_view(PortfolioView::Contribution);
                true
            }
            KeyCode::Char('8') => {
                self.select_view(PortfolioView::Attribution);
                true
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_selection(-1);
                true
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_selection(1);
                true
            }
            KeyCode::Enter | KeyCode::Char('o') => self.open_selected(),
            KeyCode::Char('r') => {
                self.reload_current();
                true
            }
            _ => false,
        }
    }

    fn actions(&self, area: Rect) -> Vec<WorkspaceAction> {
        let areas = portfolio_layout(area);
        let mut actions = Vec::new();
        let (viewport_top, viewport_rows) = self.update_viewport(areas.main);
        let visible_rows = viewport_rows.min(self.selection_count().saturating_sub(viewport_top));
        let action_rows = self.action_rows(viewport_top, visible_rows);
        let selected_row_is_actionable = action_rows
            .iter()
            .any(|(index, _, _)| *index == self.selected);
        let mut x = areas.tabs.x;
        for (index, view) in PortfolioView::ALL.into_iter().enumerate() {
            let width = format!(" {} {} ", index + 1, view.label()).chars().count() as u16;
            if x >= areas.tabs.right() {
                break;
            }
            let mut action = WorkspaceAction::new(
                view.action_id(),
                format!("Open {} portfolio view", view.label()),
                Rect::new(
                    x,
                    areas.tabs.y,
                    width.min(areas.tabs.right().saturating_sub(x)),
                    1,
                ),
            );
            if view == self.view && !selected_row_is_actionable {
                action = action.preferred();
            }
            actions.push(action);
            x = x.saturating_add(width);
        }

        for (index, symbol, identity) in action_rows {
            let ordinal = index.saturating_sub(viewport_top);
            let mut action = WorkspaceAction::new(
                format!(
                    "row:{}:{index}:{:016x}",
                    self.view.action_key(),
                    portfolio_identity_hash(&identity)
                ),
                format!("Open {symbol} security research"),
                Rect::new(
                    areas.main.x.saturating_add(1),
                    areas
                        .main
                        .y
                        .saturating_add(3 + u16::try_from(ordinal).unwrap_or(u16::MAX)),
                    areas.main.width.saturating_sub(2),
                    1,
                ),
            );
            if index == self.selected {
                action = action.preferred();
            }
            actions.push(action);
        }

        actions.push(WorkspaceAction::new(
            "reload",
            format!("Reload {} portfolio data", self.view.label()),
            Rect::new(
                areas.header.x,
                areas.header.y,
                areas.header.width.min(12),
                1,
            ),
        ));
        actions
    }

    fn activate_action(&mut self, id: &str) -> bool {
        if let Some(view) = PortfolioView::from_action_id(id) {
            self.select_view(view);
            return true;
        }
        if id == "reload" {
            self.reload_current();
            return true;
        }
        let mut parts = id.splitn(4, ':');
        let (Some("row"), Some(view), Some(index), Some(expected_identity)) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return false;
        };
        let Some(index) = index.parse::<usize>().ok() else {
            return false;
        };
        if view != self.view.action_key()
            || !self.row_identity(index).is_some_and(|identity| {
                format!("{:016x}", portfolio_identity_hash(&identity)) == expected_identity
            })
        {
            return false;
        }
        self.selected = index;
        self.open_selected()
    }

    fn handle_mouse(&mut self, event: MouseEvent, area: Rect) -> bool {
        let areas = portfolio_layout(area);
        let (viewport_top, viewport_rows) = self.update_viewport(areas.main);
        if is_primary_click(event, areas.header) {
            self.reload_current();
            return true;
        }
        if is_primary_click(event, areas.tabs) {
            let mut x = areas.tabs.x;
            for (index, view) in PortfolioView::ALL.into_iter().enumerate() {
                let width = format!(" {} {} ", index + 1, view.label()).chars().count() as u16;
                if event.column >= x && event.column < x.saturating_add(width) {
                    self.select_view(view);
                    return true;
                }
                x = x.saturating_add(width);
            }
            return true;
        }
        if let Some(index) = table_row_at(
            event,
            areas.main,
            viewport_rows.min(self.selection_count().saturating_sub(viewport_top)),
        ) {
            let index = viewport_top.saturating_add(index);
            self.selected = index;
            return self.open_selected();
        }
        if is_primary_click(event, areas.main) {
            if self.view == PortfolioView::Performance {
                self.status = "TWR IS FLOW-ADJUSTED PER CURRENCY · ATTRIBUTION REQUIRES POSITION-LEVEL RETURN HISTORY".to_owned();
            }
            return true;
        }
        if is_primary_click(event, areas.side) {
            self.status = match self.view {
                PortfolioView::Positions => {
                    "POSITION TOTALS RECONCILE BY CURRENCY · UNPRICED ROWS STAY VISIBLE".to_owned()
                }
                PortfolioView::Activity => {
                    "CASH SIGNS ARE PROVIDER-REPORTED · NO FX OR RETURN INFERENCE".to_owned()
                }
                PortfolioView::Performance => {
                    "END-OF-PERIOD FLOWS ARE REMOVED BEFORE LINKING SUB-PERIOD RETURNS".to_owned()
                }
                PortfolioView::TaxLots => {
                    "OPEN LOT BASIS RECONCILES BY CURRENCY · CLOSED TRADES REMAIN SEPARATE"
                        .to_owned()
                }
                PortfolioView::RealizedGains => {
                    "CLOSED-LOT PROCEEDS LESS BASIS RECONCILES BY CURRENCY · NO TAX INFERENCE"
                        .to_owned()
                }
                PortfolioView::Trades => {
                    "EXECUTION GROSS, FEES, AND NET CASH RECONCILE BY CURRENCY · READ ONLY"
                        .to_owned()
                }
                PortfolioView::Contribution => {
                    "GAIN/LOSS AND ADDITIVE CONTRIBUTION RECONCILE BY CURRENCY · NO FX".to_owned()
                }
                PortfolioView::Attribution => {
                    "ORDERED CONTRIBUTIONS LINK TO GEOMETRIC RETURN BY CURRENCY · NO FX".to_owned()
                }
            };
            return true;
        }
        if is_primary_click(event, areas.footer) {
            let controls = [
                (" 1/2/3/4/5/6/7/8/TAB VIEW  ", Some(KeyCode::Tab)),
                ("↑↓/JK SELECT  ", None),
                ("ENTER/O SECURITY  ", Some(KeyCode::Enter)),
                ("R RELOAD  ", Some(KeyCode::Char('r'))),
            ];
            let mut x = areas.footer.x;
            for (label, key) in controls {
                let width = label.chars().count() as u16;
                if event.column >= x && event.column < x.saturating_add(width) {
                    return key
                        .is_none_or(|key| self.handle_key(KeyEvent::new(key, KeyModifiers::NONE)));
                }
                x = x.saturating_add(width);
            }
            return true;
        }
        if let Some(key) = scroll_key(event, areas.main) {
            return self.handle_key(key);
        }
        false
    }

    fn poll_intents(&mut self) -> Vec<AppIntent> {
        std::mem::take(&mut self.pending_intents)
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let positions = self.query.load_portfolio();
        let activity = self.query.load_activity();
        let performance = self.query.load_performance();
        let tax_lots = self.query.load_tax_lots();
        let realized_gains = self.query.load_realized_gains();
        let trades = self.query.load_trades();
        let contribution = self.query.load_contribution();
        let attribution = self.query.load_attribution();
        let areas = portfolio_layout(area);
        let (viewport_top, viewport_rows) = self.update_viewport(areas.main);
        render_header(
            frame,
            areas.header,
            self.view,
            &PortfolioHeaderData {
                positions: &positions,
                activity: &activity,
                performance: &performance,
                tax_lots: &tax_lots,
                realized_gains: &realized_gains,
                trades: &trades,
                contribution: &contribution,
                attribution: &attribution,
            },
        );
        render_tabs(frame, areas.tabs, self.view);
        match self.view {
            PortfolioView::Positions => {
                render_positions(
                    frame,
                    areas.main,
                    &positions,
                    self.selected,
                    viewport_top,
                    viewport_rows,
                );
                render_position_source(frame, areas.side, &positions, &self.status);
            }
            PortfolioView::Activity => {
                render_activity(
                    frame,
                    areas.main,
                    &activity,
                    self.selected,
                    viewport_top,
                    viewport_rows,
                );
                render_activity_source(frame, areas.side, &activity, &self.status);
            }
            PortfolioView::Performance => {
                render_performance(
                    frame,
                    areas.main,
                    &performance,
                    self.selected,
                    viewport_top,
                    viewport_rows,
                );
                render_performance_inputs(frame, areas.side, &performance, &self.status);
            }
            PortfolioView::TaxLots => {
                render_tax_lots(
                    frame,
                    areas.main,
                    &tax_lots,
                    self.selected,
                    viewport_top,
                    viewport_rows,
                );
                render_tax_lot_source(frame, areas.side, &tax_lots, &self.status);
            }
            PortfolioView::RealizedGains => {
                render_realized_gains(
                    frame,
                    areas.main,
                    &realized_gains,
                    self.selected,
                    viewport_top,
                    viewport_rows,
                );
                render_realized_gain_source(frame, areas.side, &realized_gains, &self.status);
            }
            PortfolioView::Trades => {
                render_trades(
                    frame,
                    areas.main,
                    &trades,
                    self.selected,
                    viewport_top,
                    viewport_rows,
                );
                render_trade_source(frame, areas.side, &trades, &self.status);
            }
            PortfolioView::Contribution => {
                render_contribution(
                    frame,
                    areas.main,
                    &contribution,
                    self.selected,
                    viewport_top,
                    viewport_rows,
                );
                render_contribution_source(frame, areas.side, &contribution, &self.status);
            }
            PortfolioView::Attribution => {
                render_attribution(
                    frame,
                    areas.main,
                    &attribution,
                    self.selected,
                    viewport_top,
                    viewport_rows,
                );
                render_attribution_source(frame, areas.side, &attribution, &self.status);
            }
        }
        render_footer(frame, areas.footer, self.view);
    }

    fn capture_view(&self) -> WorkspaceViewState {
        let mut state = WorkspaceViewState::new(ID.as_str())
            .with_field("view", ViewValue::Text(self.view.action_key().to_owned()));
        if let Some(identity) = self.row_identity(self.selected) {
            state = state.with_field("selected_row_id", ViewValue::Text(identity));
        }
        if let Some(identity) = self.row_identity(self.viewport_top.get()) {
            state = state.with_field("top_row_id", ViewValue::Text(identity));
        }
        state
    }

    fn restore_view(&mut self, state: &WorkspaceViewState) -> ViewRestoreReport {
        if !state.workspace.eq_ignore_ascii_case(ID.as_str()) {
            return ViewRestoreReport::warning(format!(
                "saved state belongs to {}, not portfolio",
                state.workspace
            ));
        }

        let mut report = ViewRestoreReport::default();
        if let Some(value) = state.fields.get("view") {
            match value.as_text().and_then(PortfolioView::parse) {
                Some(view) => {
                    self.select_view(view);
                    report.restored_fields += 1;
                }
                None => {
                    report.skipped_fields += 1;
                    report
                        .warnings
                        .push("saved portfolio view is unavailable".to_owned());
                }
            }
        }

        self.selected = 0;
        self.viewport_top.set(0);
        restore_portfolio_row(
            self,
            state,
            "selected_row_id",
            "selected row",
            true,
            &mut report,
        );
        restore_portfolio_row(
            self,
            state,
            "top_row_id",
            "viewport anchor",
            false,
            &mut report,
        );
        self.clamp_selection();

        const KNOWN_FIELDS: [&str; 3] = ["view", "selected_row_id", "top_row_id"];
        let unknown = state
            .fields
            .keys()
            .filter(|field| !KNOWN_FIELDS.contains(&field.as_str()))
            .count();
        if unknown > 0 {
            report.skipped_fields += unknown;
            report
                .warnings
                .push(format!("ignored {unknown} future portfolio field(s)"));
        }
        if !state.children.is_empty() {
            report.skipped_fields += state.children.len();
            report.warnings.push(format!(
                "ignored {} future portfolio child state(s)",
                state.children.len()
            ));
        }
        report
    }
}

fn restore_portfolio_row(
    workspace: &mut PortfolioWorkspace,
    state: &WorkspaceViewState,
    field: &str,
    label: &str,
    selected: bool,
    report: &mut ViewRestoreReport,
) {
    let Some(value) = state.fields.get(field) else {
        return;
    };
    match value
        .as_text()
        .filter(|value| valid_portfolio_row_id(value))
    {
        Some(identity) => match workspace.row_index(identity) {
            Some(index) => {
                if selected {
                    workspace.selected = index;
                } else {
                    workspace.viewport_top.set(index);
                }
                report.restored_fields += 1;
            }
            None => {
                report.skipped_fields += 1;
                report
                    .warnings
                    .push(format!("saved portfolio {label} is no longer available"));
            }
        },
        None => {
            report.skipped_fields += 1;
            report
                .warnings
                .push(format!("saved portfolio {label} identity is invalid"));
        }
    }
}

fn valid_portfolio_row_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 512
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

fn portfolio_identity_hash(value: &str) -> u64 {
    value
        .as_bytes()
        .iter()
        .fold(0xcbf29ce484222325, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        })
}

#[derive(Debug, Clone, Copy)]
struct PortfolioLayout {
    header: Rect,
    tabs: Rect,
    main: Rect,
    side: Rect,
    footer: Rect,
}

fn portfolio_layout(area: Rect) -> PortfolioLayout {
    let rows = Layout::vertical([
        Constraint::Length(4),
        Constraint::Length(1),
        Constraint::Min(9),
        Constraint::Length(2),
    ])
    .split(area);
    let body =
        Layout::horizontal([Constraint::Percentage(74), Constraint::Percentage(26)]).split(rows[2]);
    PortfolioLayout {
        header: rows[0],
        tabs: rows[1],
        main: body[0],
        side: body[1],
        footer: rows[3],
    }
}

struct PortfolioHeaderData<'a> {
    positions: &'a PortfolioSnapshot,
    activity: &'a PortfolioActivityLedger,
    performance: &'a PortfolioPerformanceSnapshot,
    tax_lots: &'a PortfolioTaxLotSnapshot,
    realized_gains: &'a PortfolioRealizedGainSnapshot,
    trades: &'a PortfolioTradeLedger,
    contribution: &'a PortfolioContributionSnapshot,
    attribution: &'a PortfolioAttributionSnapshot,
}

fn render_header(
    frame: &mut Frame,
    area: Rect,
    view: PortfolioView,
    data: &PortfolioHeaderData<'_>,
) {
    let kpis = Layout::horizontal([Constraint::Ratio(1, 4); 4]).split(area);
    let values = match view {
        PortfolioView::Positions => [
            ("NET ASSET VALUE", data.positions.net_asset_value_label()),
            ("YTD RETURN", data.positions.ytd_return_label()),
            ("AVAILABLE CASH", data.positions.available_cash_label()),
            ("SHARPE", data.positions.sharpe_label()),
        ],
        PortfolioView::Activity => [
            ("ACTIVITY ROWS", data.activity.entries.len().to_string()),
            ("PERIOD", data.activity.period_label().to_owned()),
            ("NET CASH EFFECT", data.activity.net_cash_effect_label()),
            (
                "CURRENCIES",
                data.activity.currency_totals.len().to_string(),
            ),
        ],
        PortfolioView::Performance => [
            (
                "VALUATION POINTS",
                data.performance.point_count().to_string(),
            ),
            (
                "TIME-WEIGHTED RETURN",
                data.performance.time_weighted_return_label(),
            ),
            ("BENCHMARK", data.performance.benchmark_return_label()),
            ("ACTIVE RETURN", data.performance.active_return_label()),
        ],
        PortfolioView::TaxLots => [
            ("OPEN LOTS", data.tax_lots.lots.len().to_string()),
            ("COST BASIS", data.tax_lots.cost_basis_label()),
            ("CURRENT VALUE", data.tax_lots.current_value_label()),
            ("UNREALIZED GAIN", data.tax_lots.unrealized_gain_label()),
        ],
        PortfolioView::RealizedGains => [
            ("CLOSED LOTS", data.realized_gains.lots.len().to_string()),
            ("PROCEEDS", data.realized_gains.proceeds_label()),
            ("COST BASIS", data.realized_gains.cost_basis_label()),
            ("REALIZED GAIN", data.realized_gains.realized_gain_label()),
        ],
        PortfolioView::Trades => [
            ("FILLS", data.trades.executions.len().to_string()),
            ("BUYS", data.trades.buy_fill_count().to_string()),
            ("SELLS", data.trades.sell_fill_count().to_string()),
            ("NET CASH", data.trades.net_cash_effect_label()),
        ],
        PortfolioView::Contribution => [
            ("POSITIONS", data.contribution.rows.len().to_string()),
            ("PERIOD", data.contribution.period.clone()),
            (
                "PORTFOLIO RETURN",
                data.contribution.portfolio_return_label(),
            ),
            ("ACTIVE RETURN", data.contribution.active_return_label()),
        ],
        PortfolioView::Attribution => [
            ("SECURITIES", data.attribution.rows.len().to_string()),
            ("PERIOD", data.attribution.period.clone()),
            ("LINKED RETURN", data.attribution.linked_return_label()),
            (
                "LINKED ACTIVE",
                data.attribution.linked_active_return_label(),
            ),
        ],
    };
    for (index, (label, value)) in values.into_iter().enumerate() {
        frame.render_widget(
            Paragraph::new(vec![
                Line::styled(label, MUTED),
                Line::styled(value.clone(), if value == "N/A" { RED } else { CYAN }),
            ])
            .block(Block::new().borders(Borders::ALL).border_style(AMBER))
            .alignment(Alignment::Center),
            kpis[index],
        );
    }
}

fn render_tabs(frame: &mut Frame, area: Rect, active: PortfolioView) {
    let spans = PortfolioView::ALL
        .into_iter()
        .enumerate()
        .map(|(index, view)| {
            let style = if view == active {
                Style::new().bg(AMBER.into()).fg(BG.into()).bold()
            } else {
                Style::new().fg(MUTED.into())
            };
            Span::styled(format!(" {} {} ", index + 1, view.label()), style)
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_positions(
    frame: &mut Frame,
    area: Rect,
    snapshot: &PortfolioSnapshot,
    selected: usize,
    viewport_top: usize,
    viewport_rows: usize,
) {
    let rows = snapshot
        .positions
        .iter()
        .enumerate()
        .skip(viewport_top)
        .take(viewport_rows)
        .map(|(index, position)| {
            styled_data_row(
                [
                    format!(
                        "{} · {} · {}",
                        position.account_id.as_str(),
                        position.symbol,
                        position.currency
                    ),
                    position.quantity_label(),
                    position.average_cost_label(),
                    position.market_value_label(),
                    position.pnl_label(),
                    position.weight_label(),
                ],
                index,
                selected,
            )
        })
        .collect::<Vec<_>>();
    render_table(
        frame,
        area,
        "PORT",
        "POSITIONS",
        [
            "ACCOUNT · SYMBOL · CCY",
            "QTY",
            "AVG COST",
            "MKT VALUE",
            "P&L",
            "WEIGHT",
        ],
        rows,
        [
            Constraint::Percentage(24),
            Constraint::Percentage(11),
            Constraint::Percentage(16),
            Constraint::Percentage(20),
            Constraint::Percentage(14),
            Constraint::Percentage(15),
        ],
    );
}

fn render_activity(
    frame: &mut Frame,
    area: Rect,
    activity: &PortfolioActivityLedger,
    selected: usize,
    viewport_top: usize,
    viewport_rows: usize,
) {
    let rows = activity
        .entries
        .iter()
        .enumerate()
        .skip(viewport_top)
        .take(viewport_rows)
        .map(|(index, entry)| {
            styled_data_row(
                [
                    entry.date.clone(),
                    entry.account_id.as_str().to_owned(),
                    entry.kind.label().to_owned(),
                    format!("{} · {}", entry.symbol_label(), entry.description),
                    entry.quantity_label(),
                    entry.cash_effect_label(),
                    entry.fees_label(),
                ],
                index,
                selected,
            )
        })
        .collect::<Vec<_>>();
    render_table(
        frame,
        area,
        "ACT",
        "VERSIONED CASH + BROKER ACTIVITY",
        [
            "DATE",
            "ACCOUNT",
            "TYPE",
            "SYMBOL · DESCRIPTION",
            "QTY",
            "CASH",
            "FEES",
        ],
        rows,
        [
            Constraint::Percentage(12),
            Constraint::Percentage(12),
            Constraint::Percentage(11),
            Constraint::Percentage(27),
            Constraint::Percentage(11),
            Constraint::Percentage(17),
            Constraint::Percentage(10),
        ],
    );
}

fn styled_data_row<const N: usize>(
    values: [String; N],
    index: usize,
    selected: usize,
) -> Row<'static> {
    Row::new(values.into_iter().map(|value| {
        let style = theme::value(&value);
        Cell::from(value).style(style)
    }))
    .style(if index == selected {
        Style::new().bg(CYAN.into()).fg(BG.into()).bold()
    } else {
        Style::new()
    })
}

fn render_position_source(
    frame: &mut Frame,
    area: Rect,
    snapshot: &PortfolioSnapshot,
    status: &str,
) {
    let mut lines = vec![
        Line::styled("SOURCE", AMBER),
        Line::styled(snapshot.source.clone(), INK),
        Line::styled(snapshot.as_of.clone(), MUTED),
        Line::styled(snapshot.input_version.clone(), CYAN),
        Line::raw(""),
        Line::styled("CURRENCY TOTALS", AMBER),
    ];
    for total in &snapshot.currency_totals {
        lines.push(Line::styled(
            format!(
                "{} NAV {} · CASH {} · {} UNPRICED",
                total.currency,
                format_money(total.net_asset_value),
                format_money(total.available_cash),
                total.unpriced_positions
            ),
            INK,
        ));
    }
    lines.extend([
        Line::raw(""),
        Line::styled("METHODOLOGY", AMBER),
        Line::styled(snapshot.methodology.clone(), MUTED),
        Line::raw(""),
        Line::styled(status, YELLOW),
        Line::styled("PORT IMPORT <FILE.CSV>", CYAN),
    ]);
    for disclosure in snapshot.disclosures.iter().take(5) {
        lines.push(Line::styled(format!("• {disclosure}"), MUTED));
    }
    render_side(frame, area, "SRC", "POSITION SNAPSHOT", lines);
}

fn render_activity_source(
    frame: &mut Frame,
    area: Rect,
    activity: &PortfolioActivityLedger,
    status: &str,
) {
    let mut lines = vec![
        Line::styled("SOURCE / PERIOD", AMBER),
        Line::styled(activity.source.clone(), INK),
        Line::styled(activity.period.clone(), MUTED),
        Line::styled(activity.input_version.clone(), CYAN),
        Line::raw(""),
        Line::styled("CASH RECONCILIATION", AMBER),
    ];
    for total in &activity.currency_totals {
        lines.extend([
            Line::styled(
                format!(
                    "{} IN {} · OUT {}",
                    total.currency,
                    format_money(total.inflows),
                    format_money(total.outflows)
                ),
                INK,
            ),
            Line::styled(
                format!(
                    "  NET {} · DIV {} · INT {} · FEES {}",
                    format_money(total.net_cash_effect),
                    format_money(total.dividends),
                    format_money(total.interest),
                    format_money(total.fees)
                ),
                GREEN,
            ),
        ]);
    }
    lines.extend([
        Line::raw(""),
        Line::styled("METHODOLOGY", AMBER),
        Line::styled(activity.methodology.clone(), MUTED),
        Line::raw(""),
        Line::styled(status, YELLOW),
        Line::styled("PORT IMPORT ACTIVITY <CSV>", CYAN),
    ]);
    for disclosure in activity.disclosures.iter().take(6) {
        lines.push(Line::styled(format!("• {disclosure}"), MUTED));
    }
    render_side(frame, area, "LEDGER", "EXACT CASH BY CURRENCY", lines);
}

fn render_tax_lots(
    frame: &mut Frame,
    area: Rect,
    snapshot: &PortfolioTaxLotSnapshot,
    selected: usize,
    viewport_top: usize,
    viewport_rows: usize,
) {
    let rows = snapshot
        .lots
        .iter()
        .enumerate()
        .skip(viewport_top)
        .take(viewport_rows)
        .map(|(index, lot)| {
            styled_data_row(
                [
                    lot.acquired_date.clone(),
                    lot.account_id.as_str().to_owned(),
                    format!("{} · {}", lot.symbol, lot.currency),
                    lot.holding_period.label().to_owned(),
                    lot.quantity_label(),
                    format_money(lot.cost_basis),
                    lot.current_value_label(),
                    lot.unrealized_gain_label(),
                    lot.unrealized_return_label(),
                ],
                index,
                selected,
            )
        })
        .collect::<Vec<_>>();
    render_table(
        frame,
        area,
        "LOTS",
        "OPEN TAX LOT BASIS",
        [
            "ACQUIRED",
            "ACCOUNT",
            "SYMBOL · CCY",
            "TERM",
            "QTY",
            "BASIS",
            "VALUE",
            "GAIN",
            "RETURN",
        ],
        rows,
        [
            Constraint::Percentage(11),
            Constraint::Percentage(10),
            Constraint::Percentage(13),
            Constraint::Percentage(8),
            Constraint::Percentage(10),
            Constraint::Percentage(13),
            Constraint::Percentage(13),
            Constraint::Percentage(12),
            Constraint::Percentage(10),
        ],
    );
}

fn render_tax_lot_source(
    frame: &mut Frame,
    area: Rect,
    snapshot: &PortfolioTaxLotSnapshot,
    status: &str,
) {
    let mut lines = vec![
        Line::styled("SOURCE / AS OF", AMBER),
        Line::styled(snapshot.source.clone(), INK),
        Line::styled(snapshot.as_of.clone(), MUTED),
        Line::styled(snapshot.input_version.clone(), CYAN),
        Line::raw(""),
        Line::styled("LOT RECONCILIATION", AMBER),
    ];
    for total in &snapshot.currency_totals {
        lines.extend([
            Line::styled(
                format!(
                    "{} {} LOTS · BASIS {}",
                    total.currency,
                    total.lots,
                    format_money(total.cost_basis)
                ),
                INK,
            ),
            Line::styled(
                format!(
                    "  VALUE {} · GAIN {} · {} UNPRICED",
                    format_money(total.current_value),
                    format_money(total.unrealized_gain),
                    total.unpriced_lots
                ),
                GREEN,
            ),
        ]);
    }
    lines.extend([
        Line::raw(""),
        Line::styled("METHODOLOGY", AMBER),
        Line::styled(snapshot.methodology.clone(), MUTED),
        Line::raw(""),
        Line::styled(status, YELLOW),
        Line::styled("PORT IMPORT LOTS <CSV>", CYAN),
    ]);
    for disclosure in snapshot.disclosures.iter().take(7) {
        lines.push(Line::styled(format!("• {disclosure}"), MUTED));
    }
    render_side(frame, area, "BASIS", "OPEN LOT RECONCILIATION", lines);
}

fn render_realized_gains(
    frame: &mut Frame,
    area: Rect,
    snapshot: &PortfolioRealizedGainSnapshot,
    selected: usize,
    viewport_top: usize,
    viewport_rows: usize,
) {
    let rows = snapshot
        .lots
        .iter()
        .enumerate()
        .skip(viewport_top)
        .take(viewport_rows)
        .map(|(index, lot)| {
            styled_data_row(
                [
                    lot.disposed_date.clone(),
                    lot.acquired_date.clone(),
                    lot.account_id.as_str().to_owned(),
                    format!("{} · {}", lot.symbol, lot.currency),
                    lot.holding_period.label().to_owned(),
                    lot.quantity_label(),
                    format_money(lot.proceeds),
                    format_money(lot.cost_basis),
                    format_money(lot.realized_gain),
                    lot.realized_return_label(),
                ],
                index,
                selected,
            )
        })
        .collect::<Vec<_>>();
    render_table(
        frame,
        area,
        "REAL",
        "BROKER CLOSED LOTS + REALIZED GAINS",
        [
            "SOLD",
            "ACQUIRED",
            "ACCOUNT",
            "SYMBOL · CCY",
            "TERM",
            "QTY",
            "PROCEEDS",
            "BASIS",
            "GAIN",
            "RETURN",
        ],
        rows,
        [
            Constraint::Percentage(10),
            Constraint::Percentage(10),
            Constraint::Percentage(9),
            Constraint::Percentage(12),
            Constraint::Percentage(7),
            Constraint::Percentage(9),
            Constraint::Percentage(11),
            Constraint::Percentage(11),
            Constraint::Percentage(11),
            Constraint::Percentage(10),
        ],
    );
}

fn render_realized_gain_source(
    frame: &mut Frame,
    area: Rect,
    snapshot: &PortfolioRealizedGainSnapshot,
    status: &str,
) {
    let mut lines = vec![
        Line::styled("SOURCE / PERIOD", AMBER),
        Line::styled(snapshot.source.clone(), INK),
        Line::styled(snapshot.period.clone(), MUTED),
        Line::styled(snapshot.input_version.clone(), CYAN),
        Line::raw(""),
        Line::styled("REALIZED RECONCILIATION", AMBER),
    ];
    for total in &snapshot.currency_totals {
        lines.extend([
            Line::styled(
                format!(
                    "{} {} LOTS · PROCEEDS {}",
                    total.currency,
                    total.lots,
                    format_money(total.proceeds)
                ),
                INK,
            ),
            Line::styled(
                format!(
                    "  BASIS {} · GAIN {}",
                    format_money(total.cost_basis),
                    format_money(total.realized_gain)
                ),
                GREEN,
            ),
            Line::styled(
                format!(
                    "  SHORT {} · LONG {} · UNKNOWN {}",
                    format_money(total.short_term_gain),
                    format_money(total.long_term_gain),
                    format_money(total.unknown_term_gain)
                ),
                MUTED,
            ),
        ]);
    }
    lines.extend([
        Line::raw(""),
        Line::styled("METHODOLOGY", AMBER),
        Line::styled(snapshot.methodology.clone(), MUTED),
        Line::raw(""),
        Line::styled(status, YELLOW),
        Line::styled("PORT IMPORT REALIZED <CSV>", CYAN),
    ]);
    for disclosure in snapshot.disclosures.iter().take(7) {
        lines.push(Line::styled(format!("• {disclosure}"), MUTED));
    }
    render_side(frame, area, "GAIN", "CLOSED-LOT RECONCILIATION", lines);
}

fn render_trades(
    frame: &mut Frame,
    area: Rect,
    ledger: &PortfolioTradeLedger,
    selected: usize,
    viewport_top: usize,
    viewport_rows: usize,
) {
    let rows = ledger
        .executions
        .iter()
        .enumerate()
        .skip(viewport_top)
        .take(viewport_rows)
        .map(|(index, execution)| {
            styled_data_row(
                [
                    execution.executed_at.clone(),
                    execution.order_id.clone(),
                    execution.account_id.as_str().to_owned(),
                    execution.side.label().to_owned(),
                    format!("{} · {}", execution.symbol, execution.currency),
                    execution.quantity_label(),
                    execution.fill_price_label(),
                    format_money(execution.gross_amount),
                    format_money(execution.fees),
                    format_money(execution.net_cash_effect),
                ],
                index,
                selected,
            )
        })
        .collect::<Vec<_>>();
    render_table(
        frame,
        area,
        "FILL",
        "VERIFIED BROKER ORDER/FILL HISTORY",
        [
            "EXECUTED",
            "ORDER",
            "ACCOUNT",
            "SIDE",
            "SYMBOL · CCY",
            "QTY",
            "PRICE",
            "GROSS",
            "FEES",
            "NET CASH",
        ],
        rows,
        [
            Constraint::Percentage(15),
            Constraint::Percentage(8),
            Constraint::Percentage(8),
            Constraint::Percentage(6),
            Constraint::Percentage(11),
            Constraint::Percentage(9),
            Constraint::Percentage(10),
            Constraint::Percentage(10),
            Constraint::Percentage(9),
            Constraint::Percentage(14),
        ],
    );
}

fn render_trade_source(frame: &mut Frame, area: Rect, ledger: &PortfolioTradeLedger, status: &str) {
    let mut lines = vec![
        Line::styled("SOURCE / PERIOD", AMBER),
        Line::styled(ledger.source.clone(), INK),
        Line::styled(ledger.period.clone(), MUTED),
        Line::styled(ledger.input_version.clone(), CYAN),
        Line::raw(""),
        Line::styled("EXECUTION RECONCILIATION", AMBER),
    ];
    for total in &ledger.currency_totals {
        lines.extend([
            Line::styled(
                format!(
                    "{} {} FILLS · BUY {} · SELL {}",
                    total.currency, total.fills, total.buy_fills, total.sell_fills
                ),
                INK,
            ),
            Line::styled(
                format!(
                    "  BUY {} · SELL {}",
                    format_money(total.buy_gross),
                    format_money(total.sell_gross)
                ),
                MUTED,
            ),
            Line::styled(
                format!(
                    "  FEES {} · NET {}",
                    format_money(total.fees),
                    format_money(total.net_cash_effect)
                ),
                GREEN,
            ),
        ]);
    }
    lines.extend([
        Line::raw(""),
        Line::styled("METHODOLOGY", AMBER),
        Line::styled(ledger.methodology.clone(), MUTED),
        Line::raw(""),
        Line::styled(status, YELLOW),
        Line::styled("PORT IMPORT TRADES <CSV>", CYAN),
    ]);
    for disclosure in ledger.disclosures.iter().take(8) {
        lines.push(Line::styled(format!("• {disclosure}"), MUTED));
    }
    render_side(frame, area, "EXEC", "READ-ONLY ORDER/FILL LEDGER", lines);
}

fn render_contribution(
    frame: &mut Frame,
    area: Rect,
    snapshot: &PortfolioContributionSnapshot,
    selected: usize,
    viewport_top: usize,
    viewport_rows: usize,
) {
    let rows = snapshot
        .rows
        .iter()
        .enumerate()
        .skip(viewport_top)
        .take(viewport_rows)
        .map(|(index, row)| {
            styled_data_row(
                [
                    row.account_id.as_str().to_owned(),
                    format!("{} · {}", row.symbol, row.currency),
                    row.beginning_value_label(),
                    row.external_flow_label(),
                    row.ending_value_label(),
                    row.gain_loss_label(),
                    row.contribution_label(),
                    row.benchmark_contribution_label(),
                    row.active_contribution_label(),
                ],
                index,
                selected,
            )
        })
        .collect::<Vec<_>>();
    render_table(
        frame,
        area,
        "ATTR",
        "SINGLE-PERIOD SECURITY CONTRIBUTION",
        [
            "ACCOUNT",
            "SYMBOL · CCY",
            "BEGIN",
            "FLOW",
            "END",
            "GAIN/LOSS",
            "CONTRIB",
            "BENCH",
            "ACTIVE",
        ],
        rows,
        [
            Constraint::Percentage(9),
            Constraint::Percentage(13),
            Constraint::Percentage(12),
            Constraint::Percentage(10),
            Constraint::Percentage(12),
            Constraint::Percentage(12),
            Constraint::Percentage(11),
            Constraint::Percentage(10),
            Constraint::Percentage(11),
        ],
    );
}

fn render_contribution_source(
    frame: &mut Frame,
    area: Rect,
    snapshot: &PortfolioContributionSnapshot,
    status: &str,
) {
    let mut lines = vec![
        Line::styled("SOURCE / PERIOD", AMBER),
        Line::styled(snapshot.source.clone(), INK),
        Line::styled(snapshot.period.clone(), MUTED),
        Line::styled(snapshot.input_version.clone(), CYAN),
        Line::raw(""),
        Line::styled("CONTRIBUTION RECONCILIATION", AMBER),
    ];
    for total in &snapshot.currency_totals {
        lines.extend([
            Line::styled(
                format!(
                    "{} {} POSITIONS · BEGIN {}",
                    total.currency,
                    total.positions,
                    format_money(total.beginning_value)
                ),
                INK,
            ),
            Line::styled(
                format!(
                    "  FLOW {} · END {} · GAIN {}",
                    format_money(total.external_flow),
                    format_money(total.ending_value),
                    format_money(total.gain_loss)
                ),
                MUTED,
            ),
            Line::styled(
                format!(
                    "  RETURN {} · BENCH {} · ACTIVE {}",
                    total.portfolio_return_label(),
                    total.benchmark_return_label(),
                    total.active_return_label()
                ),
                GREEN,
            ),
            Line::styled(
                format!(
                    "  ROUNDING RESIDUAL {} cbp{}",
                    total.contribution_rounding_residual_centibps,
                    total
                        .active_rounding_residual_centibps
                        .map(|value| format!(" · ACTIVE {value} cbp"))
                        .unwrap_or_default()
                ),
                MUTED,
            ),
        ]);
    }
    lines.extend([
        Line::raw(""),
        Line::styled("METHODOLOGY", AMBER),
        Line::styled(snapshot.methodology.clone(), MUTED),
        Line::raw(""),
        Line::styled(status, YELLOW),
        Line::styled("PORT IMPORT CONTRIBUTION <CSV>", CYAN),
    ]);
    for disclosure in snapshot.disclosures.iter().take(8) {
        lines.push(Line::styled(format!("• {disclosure}"), MUTED));
    }
    render_side(frame, area, "ATTR", "ADDITIVE CONTRIBUTION", lines);
}

fn render_attribution(
    frame: &mut Frame,
    area: Rect,
    snapshot: &PortfolioAttributionSnapshot,
    selected: usize,
    viewport_top: usize,
    viewport_rows: usize,
) {
    let rows = snapshot
        .rows
        .iter()
        .enumerate()
        .skip(viewport_top)
        .take(viewport_rows)
        .map(|(index, row)| {
            styled_data_row(
                [
                    row.account_id.as_str().to_owned(),
                    format!("{} · {}", row.symbol, row.currency),
                    row.periods_present.to_string(),
                    row.linked_contribution_label(),
                    row.linked_benchmark_contribution_label(),
                    row.linked_active_contribution_label(),
                ],
                index,
                selected,
            )
        })
        .collect::<Vec<_>>();
    render_table(
        frame,
        area,
        "LINK",
        "MULTI-PERIOD SECURITY ATTRIBUTION",
        [
            "ACCOUNT",
            "SYMBOL · CCY",
            "PERIODS",
            "LINKED CONTRIB",
            "LINKED BENCH",
            "LINKED ACTIVE",
        ],
        rows,
        [
            Constraint::Percentage(15),
            Constraint::Percentage(22),
            Constraint::Percentage(10),
            Constraint::Percentage(18),
            Constraint::Percentage(17),
            Constraint::Percentage(18),
        ],
    );
}

fn render_attribution_source(
    frame: &mut Frame,
    area: Rect,
    snapshot: &PortfolioAttributionSnapshot,
    status: &str,
) {
    let mut lines = vec![
        Line::styled("SOURCE / LINKED PERIOD", AMBER),
        Line::styled(snapshot.source.clone(), INK),
        Line::styled(snapshot.period.clone(), MUTED),
        Line::styled(snapshot.input_version.clone(), CYAN),
        Line::raw(""),
        Line::styled("LINKED RECONCILIATION", AMBER),
    ];
    for total in &snapshot.currency_totals {
        lines.extend([
            Line::styled(
                format!(
                    "{} {} PERIODS · {} SECURITIES",
                    total.currency, total.periods, total.securities
                ),
                INK,
            ),
            Line::styled(
                format!(
                    "  RETURN {} · BENCH {}",
                    total.linked_return_label(),
                    total.linked_benchmark_return_label()
                ),
                GREEN,
            ),
            Line::styled(
                format!("  ACTIVE {}", total.linked_active_return_label()),
                GREEN,
            ),
            Line::styled(
                format!(
                    "  RESIDUAL {} cbp{}",
                    total.contribution_rounding_residual_centibps,
                    total
                        .active_rounding_residual_centibps
                        .map(|value| format!(" · ACTIVE {value} cbp"))
                        .unwrap_or_default()
                ),
                MUTED,
            ),
        ]);
    }
    lines.extend([
        Line::raw(""),
        Line::styled("METHODOLOGY", AMBER),
        Line::styled(snapshot.methodology.clone(), MUTED),
        Line::raw(""),
        Line::styled(status, YELLOW),
        Line::styled("PORT IMPORT ATTRIBUTION <CSV>", CYAN),
    ]);
    for disclosure in snapshot.disclosures.iter().take(8) {
        lines.push(Line::styled(format!("• {disclosure}"), MUTED));
    }
    render_side(frame, area, "LINK", "ORDERED FRONGELLO LINKING", lines);
}

fn render_side(
    frame: &mut Frame,
    area: Rect,
    code: &'static str,
    title: &'static str,
    lines: Vec<Line<'_>>,
) {
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(terminal_block(code, title)),
        area,
    );
}

fn render_performance(
    frame: &mut Frame,
    area: Rect,
    performance: &PortfolioPerformanceSnapshot,
    selected: usize,
    viewport_top: usize,
    viewport_rows: usize,
) {
    let rows = performance
        .series
        .iter()
        .enumerate()
        .skip(viewport_top)
        .take(viewport_rows)
        .map(|(index, series)| {
            let opening = series
                .points
                .first()
                .map(|point| format_money(point.ending_value))
                .unwrap_or_else(|| "—".to_owned());
            let ending = series
                .points
                .last()
                .map(|point| format_money(point.ending_value))
                .unwrap_or_else(|| "—".to_owned());
            let net_flow = series
                .points
                .iter()
                .try_fold(0_i128, |total, point| {
                    total.checked_add(point.external_flow.minor_units())
                })
                .map(|minor| {
                    format_money(crate::foundation::Money::from_minor_units(
                        minor,
                        series.currency,
                    ))
                })
                .unwrap_or_else(|| "OVERFLOW".to_owned());
            styled_data_row(
                [
                    series.currency.to_string(),
                    series.period_label(),
                    series.points.len().to_string(),
                    opening,
                    ending,
                    net_flow,
                    series.time_weighted_return_label(),
                    series.benchmark_return_label(),
                    series.active_return_label(),
                ],
                index,
                selected,
            )
        })
        .collect::<Vec<_>>();
    render_table(
        frame,
        area,
        "PERF",
        "FLOW-ADJUSTED TIME-WEIGHTED RETURN",
        [
            "CCY", "PERIOD", "POINTS", "OPEN", "END", "NET FLOW", "TWR", "BENCH", "ACTIVE",
        ],
        rows,
        [
            Constraint::Percentage(6),
            Constraint::Percentage(20),
            Constraint::Percentage(7),
            Constraint::Percentage(12),
            Constraint::Percentage(12),
            Constraint::Percentage(12),
            Constraint::Percentage(10),
            Constraint::Percentage(10),
            Constraint::Percentage(11),
        ],
    );
}

fn render_performance_inputs(
    frame: &mut Frame,
    area: Rect,
    performance: &PortfolioPerformanceSnapshot,
    status: &str,
) {
    let mut lines = vec![
        Line::styled("SOURCE / PERIOD", AMBER),
        Line::styled(performance.source.clone(), INK),
        Line::styled(performance.period.clone(), MUTED),
        Line::styled(performance.input_version.clone(), CYAN),
        Line::raw(""),
        Line::styled("METHODOLOGY", AMBER),
        Line::styled(performance.methodology.clone(), MUTED),
        Line::raw(""),
        Line::styled(status, YELLOW),
        Line::styled("PORT IMPORT PERFORMANCE <CSV>", CYAN),
    ];
    for disclosure in performance.disclosures.iter().take(7) {
        lines.push(Line::styled(format!("• {disclosure}"), MUTED));
    }
    render_side(frame, area, "INPUT", "DATED VALUATIONS", lines);
}

fn render_footer(frame: &mut Frame, area: Rect, view: PortfolioView) {
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" 1/2/3/4/5/6/7/8/TAB ", AMBER),
            Span::styled("VIEW  ", MUTED),
            Span::styled("↑↓/JK ", AMBER),
            Span::styled("SELECT  ", MUTED),
            Span::styled("ENTER/O ", AMBER),
            Span::styled("SECURITY  ", MUTED),
            Span::styled("R ", AMBER),
            Span::styled("RELOAD  ", MUTED),
            Span::styled(format!("{} · CLICKABLE", view.label()), YELLOW),
        ])),
        area,
    );
}

fn expand_home(path: &str) -> PathBuf {
    if path == "~" {
        return std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(path));
    }
    if let Some(relative) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(relative);
        }
    }
    PathBuf::from(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{bootstrap, runtime};
    use crossterm::event::{MouseButton, MouseEventKind};
    use ratatui::{backend::TestBackend, Terminal};
    use std::sync::Mutex;

    fn click(x: u16, y: u16) -> MouseEvent {
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        }
    }

    #[test]
    fn keyboard_and_commands_switch_all_views() {
        let mut workspace = PortfolioWorkspace::new(Arc::new(crate::infrastructure::DemoData));

        assert!(workspace.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE)));
        assert_eq!(workspace.view, PortfolioView::Activity);
        assert!(workspace.handle_command(&CommandInvocation {
            function: "PORT".to_owned(),
            args: vec!["PERFORMANCE".to_owned()],
        }));
        assert_eq!(workspace.view, PortfolioView::Performance);
        assert!(workspace.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));
        assert_eq!(workspace.view, PortfolioView::TaxLots);
        assert!(workspace.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));
        assert_eq!(workspace.view, PortfolioView::RealizedGains);
        assert!(workspace.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));
        assert_eq!(workspace.view, PortfolioView::Trades);
        assert!(workspace.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));
        assert_eq!(workspace.view, PortfolioView::Contribution);
        assert!(workspace.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));
        assert_eq!(workspace.view, PortfolioView::Attribution);
        assert!(workspace.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));
        assert_eq!(workspace.view, PortfolioView::Positions);
    }

    #[test]
    fn all_portfolio_regions_respond_to_primary_clicks() {
        let mut workspace = PortfolioWorkspace::new(Arc::new(crate::infrastructure::DemoData));
        let area = Rect::new(0, 0, 120, 36);
        let layout = portfolio_layout(area);

        assert!(workspace.handle_mouse(click(layout.tabs.x + 15, layout.tabs.y), area));
        assert_eq!(workspace.view, PortfolioView::Activity);
        assert!(workspace.handle_mouse(click(layout.main.x + 2, layout.main.y + 2), area));
        assert!(workspace.handle_mouse(click(layout.side.x + 1, layout.side.y + 1), area));
        assert!(workspace.status.contains("PROVIDER-REPORTED"));
        assert!(workspace.handle_mouse(click(layout.footer.x + 1, layout.footer.y), area));
        assert!(workspace.handle_mouse(click(layout.header.x + 1, layout.header.y + 1), area));
    }

    #[test]
    fn visible_actions_route_tabs_rows_and_reload_without_shell_domain_knowledge() {
        let mut workspace = PortfolioWorkspace::new(Arc::new(crate::infrastructure::DemoData));
        let area = Rect::new(0, 0, 120, 36);
        let actions = workspace.actions(area);

        assert!(actions.iter().any(|action| action.id == "view:attribution"));
        let row_action = actions
            .iter()
            .find(|action| action.id.starts_with("row:positions:0:"))
            .unwrap()
            .id
            .clone();
        assert!(actions.iter().any(|action| action.id == "reload"));
        assert!(actions.iter().all(|action| {
            action.area.x >= area.x
                && action.area.y >= area.y
                && action.area.right() <= area.right()
                && action.area.bottom() <= area.bottom()
        }));

        assert!(workspace.activate_action("view:attribution"));
        assert_eq!(workspace.view, PortfolioView::Attribution);
        let attribution_row = workspace
            .actions(area)
            .into_iter()
            .find(|action| action.id.starts_with("row:attribution:0:"))
            .unwrap()
            .id;
        assert!(workspace.activate_action(&attribution_row));
        assert!(matches!(
            workspace.poll_intents().as_slice(),
            [AppIntent::DispatchCommand { command, origin }]
                if command.starts_with("SEC ") && *origin == ID
        ));
        assert!(!workspace.activate_action(&row_action));
        assert!(!workspace.activate_action("row:attribution:999999:AAPL"));
        assert!(!workspace.activate_action("unknown"));
    }

    #[test]
    fn narrow_portfolio_actions_include_only_rendered_tabs_and_rows() {
        let workspace = PortfolioWorkspace::new(Arc::new(crate::infrastructure::DemoData));
        let actions = workspace.actions(Rect::new(0, 0, 60, 24));

        assert!(actions.iter().any(|action| action.id == "view:positions"));
        assert!(!actions.iter().any(|action| action.id == "view:attribution"));
        assert!(
            actions
                .iter()
                .filter(|action| action.id.starts_with("row:"))
                .count()
                <= 11
        );
    }

    #[test]
    fn typed_view_round_trips_every_subview_and_stable_row_anchor() {
        for view in PortfolioView::ALL {
            let mut source = PortfolioWorkspace::new(Arc::new(crate::infrastructure::DemoData));
            source.select_view(view);
            source.selected = source.selection_count().saturating_sub(1);
            source.viewport_top.set(source.selected.saturating_sub(1));
            let state = source.capture_view();

            let mut restored = PortfolioWorkspace::new(Arc::new(crate::infrastructure::DemoData));
            let report = restored.restore_view(&state);

            assert_eq!(report.restored_fields, state.fields.len(), "{view:?}");
            assert_eq!(report.skipped_fields, 0, "{view:?}");
            assert!(
                report.warnings.is_empty(),
                "{view:?}: {:?}",
                report.warnings
            );
            assert_eq!(restored.capture_view(), state, "{view:?}");
        }
    }

    struct ReversedPortfolio;

    impl PortfolioRepository for ReversedPortfolio {
        fn load_portfolio(&self) -> PortfolioSnapshot {
            let mut snapshot = crate::infrastructure::DemoData.load_portfolio();
            snapshot.positions.reverse();
            snapshot
        }
    }

    #[test]
    fn typed_view_follows_position_identity_after_provider_reordering() {
        let mut source = PortfolioWorkspace::new(Arc::new(crate::infrastructure::DemoData));
        source.selected = 1;
        source.viewport_top.set(1);
        let state = source.capture_view();
        let selected_identity = state.fields.get("selected_row_id").cloned();

        let mut restored = PortfolioWorkspace::new(Arc::new(ReversedPortfolio));
        let report = restored.restore_view(&state);

        assert_eq!(report.restored_fields, 3);
        assert_eq!(report.skipped_fields, 0);
        assert_eq!(restored.selected, restored.selection_count() - 2);
        assert_eq!(
            restored.capture_view().fields.get("selected_row_id"),
            selected_identity.as_ref()
        );
    }

    struct RefreshingPortfolio {
        snapshot: Mutex<PortfolioSnapshot>,
    }

    impl PortfolioRepository for RefreshingPortfolio {
        fn load_portfolio(&self) -> PortfolioSnapshot {
            self.snapshot.lock().expect("portfolio snapshot").clone()
        }

        fn reload(&self) -> Result<PortfolioSnapshot, super::super::PortfolioError> {
            let mut snapshot = self.snapshot.lock().expect("portfolio snapshot");
            snapshot.positions.reverse();
            Ok(snapshot.clone())
        }
    }

    #[test]
    fn live_reload_preserves_selected_and_top_position_identities() {
        let query = Arc::new(RefreshingPortfolio {
            snapshot: Mutex::new(crate::infrastructure::DemoData.load_portfolio()),
        });
        let mut workspace = PortfolioWorkspace::new(query);
        workspace.selected = 1;
        workspace.viewport_top.set(0);
        let selected = workspace.row_identity(1).unwrap();
        let top = workspace.row_identity(0).unwrap();

        workspace.reload_positions();

        assert_eq!(workspace.row_identity(workspace.selected), Some(selected));
        assert_eq!(
            workspace.row_identity(workspace.viewport_top.get()),
            Some(top)
        );
    }

    #[test]
    fn viewport_scrolls_long_tables_and_actions_track_rendered_rows() {
        let mut workspace = PortfolioWorkspace::new(Arc::new(crate::infrastructure::DemoData));
        workspace.selected = workspace.selection_count() - 1;
        let area = Rect::new(0, 0, 80, 16);
        let actions = workspace.actions(area);
        let top = workspace.viewport_top.get();

        assert!(top > 0);
        assert!(workspace.selected >= top);
        assert!(workspace.selected < top + workspace.viewport_rows.get());
        assert!(actions.iter().any(|action| {
            action
                .id
                .starts_with(&format!("row:positions:{}:", workspace.selected))
        }));
        assert_eq!(
            workspace.capture_view().fields.get("top_row_id"),
            workspace.row_identity(top).map(ViewValue::Text).as_ref()
        );
    }

    #[test]
    fn typed_view_degrades_invalid_missing_and_future_state_independently() {
        let state = WorkspaceViewState::new(ID.as_str())
            .with_field("view", ViewValue::Text("future".to_owned()))
            .with_field(
                "selected_row_id",
                ViewValue::Text("position|missing|instrument|USD".to_owned()),
            )
            .with_field(
                "top_row_id",
                ViewValue::Text("position|bad\nidentity|USD".to_owned()),
            )
            .with_field("future_field", ViewValue::Boolean(true))
            .with_child(WorkspaceViewState::new("future-portfolio-child"));
        let mut restored = PortfolioWorkspace::new(Arc::new(crate::infrastructure::DemoData));

        let report = restored.restore_view(&state);

        assert_eq!(report.restored_fields, 0);
        assert_eq!(report.skipped_fields, 5);
        assert_eq!(report.warnings.len(), 5);
        assert_eq!(restored.view, PortfolioView::Positions);
        assert_eq!(restored.selected, 0);
        assert_eq!(restored.viewport_top.get(), 0);
    }

    #[test]
    fn performance_panel_renders_versioned_flow_adjusted_returns() {
        let mut app = bootstrap::demo_app();
        for character in "/PERFORMANCE\n".chars() {
            let code = match character {
                '\n' => KeyCode::Enter,
                character => KeyCode::Char(character),
            };
            app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
        }
        let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
        terminal.draw(|frame| runtime::render(frame, &app)).unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();

        assert!(rendered.contains("FLOW-ADJUSTED TIME-WEIGHTED RETURN"));
        assert!(rendered.contains("+17.78%"));
        assert!(rendered.contains("+12.40%"));
        assert!(rendered.contains("DEMO-PERFORMANCE-V1"));
    }

    #[test]
    fn tax_lot_panel_renders_open_basis_and_security_rows() {
        let mut app = bootstrap::demo_app();
        for character in "/LOTS\n".chars() {
            let code = match character {
                '\n' => KeyCode::Enter,
                character => KeyCode::Char(character),
            };
            app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
        }
        let mut terminal = Terminal::new(TestBackend::new(160, 48)).unwrap();
        terminal.draw(|frame| runtime::render(frame, &app)).unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();

        assert!(rendered.contains("OPEN TAX LOT BASIS"));
        assert!(rendered.contains("DEMO-TAX-LOTS-V1"));
        assert!(rendered.contains("META"));
        assert!(rendered.contains("$30,000.00"));
    }

    #[test]
    fn realized_gain_panel_renders_closed_lot_reconciliation() {
        let mut app = bootstrap::demo_app();
        for character in "/REALIZED\n".chars() {
            let code = match character {
                '\n' => KeyCode::Enter,
                character => KeyCode::Char(character),
            };
            app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
        }
        let mut terminal = Terminal::new(TestBackend::new(160, 48)).unwrap();
        terminal.draw(|frame| runtime::render(frame, &app)).unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();

        assert!(rendered.contains("BROKER CLOSED LOTS + REALIZED GAINS"));
        assert!(rendered.contains("DEMO-REALIZED-GAINS-V1"));
        assert!(rendered.contains("NVDA"));
        assert!(rendered.contains("$7,500.00"));
    }

    #[test]
    fn trade_panel_renders_verified_fill_reconciliation() {
        let mut app = bootstrap::demo_app();
        for character in "/TRADES\n".chars() {
            let code = match character {
                '\n' => KeyCode::Enter,
                character => KeyCode::Char(character),
            };
            app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
        }
        let mut terminal = Terminal::new(TestBackend::new(160, 48)).unwrap();
        terminal.draw(|frame| runtime::render(frame, &app)).unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();

        assert!(rendered.contains("VERIFIED BROKER ORDER/FILL HISTORY"));
        assert!(rendered.contains("DEMO-TRADES-V1"));
        assert!(rendered.contains("META"));
        assert!(rendered.contains("$2,217.00"));
    }

    #[test]
    fn contribution_panel_renders_additive_active_attribution() {
        let mut app = bootstrap::demo_app();
        for character in "/CONTRIBUTION\n".chars() {
            let code = match character {
                '\n' => KeyCode::Enter,
                character => KeyCode::Char(character),
            };
            app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
        }
        let mut terminal = Terminal::new(TestBackend::new(160, 48)).unwrap();
        terminal.draw(|frame| runtime::render(frame, &app)).unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();

        assert!(rendered.contains("SINGLE-PERIOD SECURITY CONTRIBUTION"));
        assert!(rendered.contains("DEMO-CONTRIBUTION-V1"));
        assert!(rendered.contains("META"));
        assert!(rendered.contains("+7.0000%"));
        assert!(rendered.contains("+2.0000%"));
    }

    #[test]
    fn attribution_panel_renders_linked_multi_period_contribution() {
        let mut app = bootstrap::demo_app();
        for character in "/ATTRIBUTION\n".chars() {
            let code = match character {
                '\n' => KeyCode::Enter,
                character => KeyCode::Char(character),
            };
            app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
        }
        let mut terminal = Terminal::new(TestBackend::new(160, 48)).unwrap();
        terminal.draw(|frame| runtime::render(frame, &app)).unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();

        assert!(rendered.contains("MULTI-PERIOD SECURITY ATTRIBUTION"));
        assert!(rendered.contains("DEMO-ATTRIBUTION-V1"));
        assert!(rendered.contains("META"));
        assert!(rendered.contains("+7.8000%"));
        assert!(rendered.contains("+3.8500%"));
    }
}
