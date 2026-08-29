use std::sync::{
    mpsc::{sync_channel, Receiver, SyncSender, TrySendError},
    Arc,
};

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
    app::{
        AppIntent, CommandInvocation, ViewRestoreReport, ViewValue, Workspace, WorkspaceAction,
        WorkspaceDescriptor, WorkspaceViewState,
    },
    ui::{
        components::{render_pairs, render_table, styled_row, terminal_block},
        is_primary_click, table_row_at,
        theme::{AMBER, BG, CYAN, GREEN, MUTED, RED},
    },
};

use super::{
    ResearchView, SecurityDocumentOpener, SecurityError, SecurityPage, SecurityQuery,
    SecurityResearch, ID,
};

struct SecurityRefresh {
    generation: u64,
    symbol: String,
}

struct SecurityRefreshResult {
    generation: u64,
    result: Result<SecurityPage, SecurityError>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SecurityAreas {
    header: Rect,
    chart: Rect,
    tabs: Rect,
    research: Rect,
    statistics: Rect,
    sources: Rect,
}

fn security_areas(area: Rect) -> SecurityAreas {
    let rows = Layout::vertical([Constraint::Length(4), Constraint::Min(12)]).split(area);
    let grid = Layout::horizontal([
        Constraint::Percentage(62),
        Constraint::Percentage(19),
        Constraint::Percentage(19),
    ])
    .split(rows[1]);
    let left =
        Layout::vertical([Constraint::Percentage(58), Constraint::Percentage(42)]).split(grid[0]);
    let research = Layout::vertical([Constraint::Length(1), Constraint::Min(3)]).split(left[1]);
    SecurityAreas {
        header: rows[0],
        chart: left[0],
        tabs: research[0],
        research: research[1],
        statistics: grid[1],
        sources: grid[2],
    }
}

fn research_action_key(view: ResearchView) -> &'static str {
    match view {
        ResearchView::Financials => "financials",
        ResearchView::Estimates => "estimates",
        ResearchView::Ownership => "ownership",
        ResearchView::Filings => "filings",
        ResearchView::Peers => "peers",
    }
}

fn research_view_from_action(id: &str) -> Option<ResearchView> {
    match id.strip_prefix("view:")? {
        "financials" => Some(ResearchView::Financials),
        "estimates" => Some(ResearchView::Estimates),
        "ownership" => Some(ResearchView::Ownership),
        "filings" => Some(ResearchView::Filings),
        "peers" => Some(ResearchView::Peers),
        _ => None,
    }
}

fn research_tab_areas(area: Rect) -> Vec<(ResearchView, Rect)> {
    let mut x = area.x;
    let mut tabs = Vec::new();
    for (index, view) in ResearchView::ALL.into_iter().enumerate() {
        if x >= area.right() {
            break;
        }
        let width = format!(" {} {} ", index + 1, view.label()).chars().count() as u16;
        tabs.push((
            view,
            Rect::new(x, area.y, width.min(area.right().saturating_sub(x)), 1),
        ));
        x = x.saturating_add(width);
    }
    tabs
}

fn research_row_area(area: Rect, index: usize) -> Option<Rect> {
    let y = area.y.saturating_add(3 + u16::try_from(index).ok()?);
    (y < area.bottom())
        .then(|| Rect::new(area.x.saturating_add(1), y, area.width.saturating_sub(2), 1))
}

pub struct SecurityWorkspace {
    query: Arc<dyn SecurityQuery>,
    symbol: String,
    instrument_id: String,
    research_view: ResearchView,
    selected_insider: usize,
    selected_insider_accession: Option<String>,
    document_opener: Option<Arc<dyn SecurityDocumentOpener>>,
    document_status: String,
    pending_intents: Vec<AppIntent>,
    refresh_sender: SyncSender<SecurityRefresh>,
    refresh_receiver: Receiver<SecurityRefreshResult>,
    pending_refresh: Option<SecurityRefresh>,
    desired_generation: u64,
    page: Option<SecurityPage>,
    error: Option<SecurityError>,
}

impl SecurityWorkspace {
    pub fn new(query: Arc<dyn SecurityQuery>) -> Self {
        Self::with_symbol(query, "AAPL US")
    }

    pub fn with_symbol(query: Arc<dyn SecurityQuery>, symbol: impl Into<String>) -> Self {
        Self::build(query, symbol.into(), None)
    }

    pub fn with_symbol_and_document_opener(
        query: Arc<dyn SecurityQuery>,
        symbol: impl Into<String>,
        document_opener: Arc<dyn SecurityDocumentOpener>,
    ) -> Self {
        Self::build(query, symbol.into(), Some(document_opener))
    }

    fn build(
        query: Arc<dyn SecurityQuery>,
        symbol: String,
        document_opener: Option<Arc<dyn SecurityDocumentOpener>>,
    ) -> Self {
        let identity = super::SecurityIdentity::from_terminal_symbol(&symbol);
        let instrument_id = identity.instrument_id.to_string();
        let symbol = identity.terminal_symbol;
        let (refresh_sender, worker_receiver) = sync_channel::<SecurityRefresh>(1);
        let (worker_sender, refresh_receiver) = sync_channel::<SecurityRefreshResult>(1);
        let worker_query = query.clone();
        std::thread::Builder::new()
            .name("security-research".to_owned())
            .spawn(move || {
                while let Ok(mut refresh) = worker_receiver.recv() {
                    while let Ok(newer) = worker_receiver.try_recv() {
                        refresh = newer;
                    }
                    let result = worker_query.load_security(&refresh.symbol);
                    if worker_sender
                        .send(SecurityRefreshResult {
                            generation: refresh.generation,
                            result,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .expect("security research worker should start");
        let mut workspace = Self {
            query,
            symbol,
            instrument_id,
            research_view: ResearchView::Financials,
            selected_insider: 0,
            selected_insider_accession: None,
            document_opener,
            document_status: "O/ENTER OPENS SELECTED SEC FILING".to_owned(),
            pending_intents: Vec::new(),
            refresh_sender,
            refresh_receiver,
            pending_refresh: None,
            desired_generation: 0,
            page: None,
            error: None,
        };
        workspace.queue_refresh();
        workspace
    }

    fn select_view(&mut self, function: &str) -> bool {
        self.research_view = match function {
            "FA" | "FINANCIALS" => ResearchView::Financials,
            "EE" | "ESTIMATES" => ResearchView::Estimates,
            "OWN" | "OWNERSHIP" | "INSIDER" | "FORM4" => ResearchView::Ownership,
            "FIL" | "FILINGS" => ResearchView::Filings,
            "RV" | "PEERS" => ResearchView::Peers,
            _ => return false,
        };
        true
    }

    fn move_insider_selection(&mut self, delta: isize) {
        let count = self
            .page
            .as_ref()
            .map_or(0, |page| page.research.insider_transactions.len());
        if count == 0 {
            self.selected_insider = 0;
            self.selected_insider_accession = None;
            return;
        }
        self.selected_insider = self
            .selected_insider
            .saturating_add_signed(delta)
            .min(count - 1);
        self.remember_selected_insider();
    }

    fn remember_selected_insider(&mut self) {
        self.selected_insider_accession = self.page.as_ref().and_then(|page| {
            page.research
                .insider_transactions
                .get(self.selected_insider)
                .map(|transaction| transaction.accession.clone())
        });
    }

    fn resolve_selected_insider(&mut self, page: &SecurityPage) {
        let requested = self.selected_insider_accession.as_deref();
        let resolved = requested.and_then(|accession| {
            page.research
                .insider_transactions
                .iter()
                .position(|transaction| transaction.accession == accession)
        });
        self.selected_insider = resolved
            .unwrap_or(0)
            .min(page.research.insider_transactions.len().saturating_sub(1));
        if requested.is_some() && resolved.is_none() {
            self.document_status =
                "SAVED FORM 4 SELECTION UNAVAILABLE · USING FIRST VISIBLE ROW".to_owned();
        }
        self.selected_insider_accession = page
            .research
            .insider_transactions
            .get(self.selected_insider)
            .map(|transaction| transaction.accession.clone());
    }

    fn open_selected_insider_filing(&mut self) {
        let Some((url, accession)) = self.page.as_ref().and_then(|page| {
            let transaction = page
                .research
                .insider_transactions
                .get(self.selected_insider)?;
            Some((
                transaction.document_url.clone(),
                transaction.accession.clone(),
            ))
        }) else {
            self.document_status = "NO FORM 4 TRANSACTION SELECTED".to_owned();
            return;
        };
        let Some(url) = url else {
            self.document_status = "SELECTED FORM 4 HAS NO PUBLISHER LINK".to_owned();
            return;
        };
        self.open_document(&url, &accession);
    }

    fn open_document(&mut self, url: &str, label: &str) -> bool {
        let Some(opener) = self.document_opener.clone() else {
            self.document_status = "DOCUMENT OPENER UNAVAILABLE".to_owned();
            return true;
        };
        self.document_status = match opener.open(url) {
            Ok(()) => format!("OPENED {label}"),
            Err(error) => error.to_string(),
        };
        true
    }

    fn open_insider_action(&mut self, index: usize, expected_accession: &str) -> bool {
        if self.research_view != ResearchView::Ownership {
            return false;
        }
        let Some((url, accession)) = self.page.as_ref().and_then(|page| {
            let transaction = page.research.insider_transactions.get(index)?;
            (transaction.accession == expected_accession).then(|| {
                (
                    transaction.document_url.clone(),
                    transaction.accession.clone(),
                )
            })
        }) else {
            return false;
        };
        self.selected_insider = index;
        self.remember_selected_insider();
        let Some(url) = url else {
            self.document_status = "SELECTED FORM 4 HAS NO PUBLISHER LINK".to_owned();
            return true;
        };
        self.open_document(&url, &accession)
    }

    fn open_filing_action(&mut self, index: usize, expected_accession: &str) -> bool {
        if self.research_view != ResearchView::Filings {
            return false;
        }
        let Some((url, accession)) = self.page.as_ref().and_then(|page| {
            let filing = page.research.filings.get(index)?;
            (filing.accession == expected_accession)
                .then(|| (filing.document_url.clone(), filing.accession.clone()))
        }) else {
            return false;
        };
        let Some(url) = url else {
            self.document_status = "SELECTED FILING HAS NO PUBLISHER LINK".to_owned();
            return true;
        };
        self.open_document(&url, &accession)
    }

    fn open_peer_action(&mut self, index: usize, expected_symbol: &str) -> bool {
        if self.research_view != ResearchView::Peers {
            return false;
        }
        let Some(symbol) = self.page.as_ref().and_then(|page| {
            let peer = page.research.peers.get(index)?;
            (peer.symbol == expected_symbol).then(|| peer.symbol.clone())
        }) else {
            return false;
        };
        self.pending_intents.push(AppIntent::DispatchCommand {
            command: format!("SEC {symbol}"),
            origin: ID,
        });
        true
    }

    fn open_chart(&mut self) {
        self.pending_intents.push(AppIntent::DispatchCommand {
            command: format!("CHART {}", self.symbol),
            origin: ID,
        });
    }

    fn ticker(&self) -> &str {
        self.symbol.split_whitespace().next().unwrap_or("AAPL")
    }

    fn queue_refresh(&mut self) {
        self.desired_generation = self.desired_generation.wrapping_add(1);
        self.pending_refresh = Some(SecurityRefresh {
            generation: self.desired_generation,
            symbol: self.symbol.clone(),
        });
        self.page = None;
        self.error = None;
        self.dispatch_pending_refresh();
    }

    fn refresh_live(&mut self) {
        self.query.request_refresh(&self.symbol);
        self.queue_refresh();
    }

    fn dispatch_pending_refresh(&mut self) {
        let Some(refresh) = self.pending_refresh.take() else {
            return;
        };
        match self.refresh_sender.try_send(refresh) {
            Ok(()) => {}
            Err(TrySendError::Full(refresh)) => self.pending_refresh = Some(refresh),
            Err(TrySendError::Disconnected(_)) => {
                self.error = Some(SecurityError::Unavailable(
                    "security research worker stopped".to_owned(),
                ));
            }
        }
    }

    fn poll_refresh(&mut self) {
        while let Ok(refresh) = self.refresh_receiver.try_recv() {
            if refresh.generation != self.desired_generation {
                continue;
            }
            match refresh.result {
                Ok(page) => {
                    self.instrument_id = page.research.identity.instrument_id.to_string();
                    self.resolve_selected_insider(&page);
                    self.page = Some(page);
                    self.error = None;
                }
                Err(error) => {
                    self.page = None;
                    self.error = Some(error);
                }
            }
        }
        self.dispatch_pending_refresh();
    }
}

impl Workspace for SecurityWorkspace {
    fn descriptor(&self) -> WorkspaceDescriptor {
        WorkspaceDescriptor {
            id: ID,
            label: "SECURITY",
            hotkey: 's',
            commands: &[
                "SEC", "AAPL", "EQUITY", "FA", "EE", "OWN", "INSIDER", "FORM4", "FIL", "RV",
            ],
        }
    }

    fn handle_command(&mut self, invocation: &CommandInvocation) -> bool {
        let previous_symbol = self.symbol.clone();
        let is_view_command = self.select_view(&invocation.function);
        for argument in &invocation.args {
            if let Some(view) = argument.strip_prefix("--view=") {
                self.select_view(&view.to_ascii_uppercase());
            }
        }

        let mut subject = if matches!(invocation.function.as_str(), "SEC" | "EQUITY") {
            invocation
                .args
                .iter()
                .filter(|arg| !arg.starts_with("--"))
                .cloned()
                .collect()
        } else if is_view_command {
            invocation
                .args
                .iter()
                .filter(|arg| !arg.starts_with("--"))
                .cloned()
                .collect()
        } else {
            let mut tokens = vec![invocation.function.clone()];
            tokens.extend(
                invocation
                    .args
                    .iter()
                    .filter(|arg| !arg.starts_with("--"))
                    .cloned(),
            );
            tokens
        };
        if subject
            .last()
            .is_some_and(|token| token.eq_ignore_ascii_case("EQUITY"))
        {
            subject.pop();
        }
        if !subject.is_empty() {
            self.symbol =
                super::SecurityIdentity::from_terminal_symbol(&subject.join(" ")).terminal_symbol;
        }
        if self.symbol != previous_symbol {
            self.instrument_id = super::SecurityIdentity::from_terminal_symbol(&self.symbol)
                .instrument_id
                .to_string();
            self.selected_insider = 0;
            self.selected_insider_accession = None;
            self.queue_refresh();
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
            KeyCode::Up | KeyCode::Char('k') if self.research_view == ResearchView::Ownership => {
                self.move_insider_selection(-1)
            }
            KeyCode::Down | KeyCode::Char('j') if self.research_view == ResearchView::Ownership => {
                self.move_insider_selection(1)
            }
            KeyCode::Enter | KeyCode::Char('o')
                if self.research_view == ResearchView::Ownership =>
            {
                self.open_selected_insider_filing()
            }
            KeyCode::Char('n') => self.pending_intents.push(AppIntent::DispatchCommand {
                command: format!("NEWS --symbol={}", self.ticker()),
                origin: ID,
            }),
            KeyCode::Char('c') => self.open_chart(),
            KeyCode::Char('a') => self.pending_intents.push(AppIntent::DispatchCommand {
                command: format!("SHEET INSERT {}", self.symbol),
                origin: ID,
            }),
            KeyCode::F(9) => self.refresh_live(),
            _ => return false,
        }
        true
    }

    fn handle_mouse(&mut self, event: MouseEvent, area: Rect) -> bool {
        let areas = security_areas(area);
        if is_primary_click(event, areas.header) {
            return self.handle_key(KeyEvent::new(
                KeyCode::F(9),
                crossterm::event::KeyModifiers::NONE,
            ));
        }
        if is_primary_click(event, areas.chart) {
            if self.research_view == ResearchView::Ownership {
                if let Some(index) = self.page.as_ref().and_then(|page| {
                    super::insider_chart::selected_at_column(
                        areas.chart,
                        &page.research.insider_transactions,
                        event.column,
                    )
                }) {
                    self.selected_insider = index;
                    self.remember_selected_insider();
                }
                return true;
            }
            self.open_chart();
            return true;
        }
        if is_primary_click(event, areas.tabs) {
            for (view, tab_area) in research_tab_areas(areas.tabs) {
                if crate::ui::contains(tab_area, event.column, event.row) {
                    self.research_view = view;
                    return true;
                }
            }
            return true;
        }
        if self.research_view == ResearchView::Ownership {
            if let Some(index) = table_row_at(
                event,
                areas.research,
                self.page
                    .as_ref()
                    .map_or(0, |page| page.research.insider_transactions.len()),
            ) {
                self.selected_insider = index;
                self.remember_selected_insider();
                return true;
            }
        }
        if self.research_view == ResearchView::Filings {
            if let Some(index) = table_row_at(
                event,
                areas.research,
                self.page
                    .as_ref()
                    .map_or(0, |page| page.research.filings.len()),
            ) {
                let Some(accession) = self.page.as_ref().and_then(|page| {
                    page.research
                        .filings
                        .get(index)
                        .map(|filing| filing.accession.clone())
                }) else {
                    return false;
                };
                return self.open_filing_action(index, &accession);
            }
        }
        if self.research_view == ResearchView::Peers {
            if let Some(index) = table_row_at(
                event,
                areas.research,
                self.page
                    .as_ref()
                    .map_or(0, |page| page.research.peers.len()),
            ) {
                let Some(symbol) = self.page.as_ref().and_then(|page| {
                    page.research
                        .peers
                        .get(index)
                        .map(|peer| peer.symbol.clone())
                }) else {
                    return false;
                };
                return self.open_peer_action(index, &symbol);
            }
        }
        false
    }

    fn actions(&self, area: Rect) -> Vec<WorkspaceAction> {
        if self.page.is_none() {
            return vec![WorkspaceAction::new(
                "refresh",
                format!("Retry {} security research", self.symbol),
                area,
            )
            .preferred()];
        }

        let areas = security_areas(area);
        let page = self.page.as_ref().expect("page was checked above");
        let has_preferred_content = match self.research_view {
            ResearchView::Financials | ResearchView::Estimates => true,
            ResearchView::Ownership => page
                .research
                .insider_transactions
                .get(self.selected_insider)
                .is_some_and(|transaction| transaction.document_url.is_some()),
            ResearchView::Filings => page
                .research
                .filings
                .iter()
                .take(usize::from(areas.research.height.saturating_sub(3)))
                .any(|filing| filing.document_url.is_some()),
            ResearchView::Peers => !page.research.peers.is_empty(),
        };
        let mut actions = research_tab_areas(areas.tabs)
            .into_iter()
            .map(|(view, tab_area)| {
                let action = WorkspaceAction::new(
                    format!("view:{}", research_action_key(view)),
                    format!("Open {} research", view.label()),
                    tab_area,
                );
                if view == self.research_view && !has_preferred_content {
                    action.preferred()
                } else {
                    action
                }
            })
            .collect::<Vec<_>>();

        match self.research_view {
            ResearchView::Financials | ResearchView::Estimates => {
                actions.push(
                    WorkspaceAction::new(
                        format!("chart:{}", self.symbol),
                        format!("Open {} chart", self.symbol),
                        areas.chart,
                    )
                    .preferred(),
                );
            }
            ResearchView::Ownership => {
                actions.extend(
                    page.research
                        .insider_transactions
                        .iter()
                        .enumerate()
                        .filter_map(|(index, transaction)| {
                            let row_area = research_row_area(areas.research, index)?;
                            transaction.document_url.as_ref()?;
                            let action = WorkspaceAction::new(
                                format!("ownership:{index}:{}", transaction.accession),
                                format!(
                                    "Open {} Form 4 filing from {}",
                                    transaction.owner, transaction.transaction_date
                                ),
                                row_area,
                            );
                            Some(if index == self.selected_insider {
                                action.preferred()
                            } else {
                                action
                            })
                        }),
                );
            }
            ResearchView::Filings => {
                let mut preferred = false;
                actions.extend(page.research.filings.iter().enumerate().filter_map(
                    |(index, filing)| {
                        let row_area = research_row_area(areas.research, index)?;
                        filing.document_url.as_ref()?;
                        let action = WorkspaceAction::new(
                            format!("filing:{index}:{}", filing.accession),
                            format!("Open {} {} filing", filing.form, filing.filed),
                            row_area,
                        );
                        if preferred {
                            Some(action)
                        } else {
                            preferred = true;
                            Some(action.preferred())
                        }
                    },
                ));
            }
            ResearchView::Peers => {
                actions.extend(page.research.peers.iter().enumerate().filter_map(
                    |(index, peer)| {
                        let row_area = research_row_area(areas.research, index)?;
                        let action = WorkspaceAction::new(
                            format!("peer:{index}:{}", peer.symbol),
                            format!("Open {} security research", peer.symbol),
                            row_area,
                        );
                        Some(if index == 0 {
                            action.preferred()
                        } else {
                            action
                        })
                    },
                ));
            }
        }

        actions.push(WorkspaceAction::new(
            "refresh",
            format!("Refresh {} security research", self.symbol),
            areas.header,
        ));
        actions
    }

    fn activate_action(&mut self, id: &str) -> bool {
        if id == "refresh" {
            self.refresh_live();
            return true;
        }
        if let Some(view) = research_view_from_action(id) {
            self.research_view = view;
            return true;
        }
        if let Some(expected_symbol) = id.strip_prefix("chart:") {
            if self.research_view == ResearchView::Ownership || self.symbol != expected_symbol {
                return false;
            }
            self.open_chart();
            return true;
        }
        let mut parts = id.splitn(3, ':');
        let (Some(kind), Some(index), Some(identity)) = (parts.next(), parts.next(), parts.next())
        else {
            return false;
        };
        let Ok(index) = index.parse::<usize>() else {
            return false;
        };
        match kind {
            "ownership" => self.open_insider_action(index, identity),
            "filing" => self.open_filing_action(index, identity),
            "peer" => self.open_peer_action(index, identity),
            _ => false,
        }
    }

    fn poll_intents(&mut self) -> Vec<AppIntent> {
        self.poll_refresh();
        std::mem::take(&mut self.pending_intents)
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let areas = security_areas(area);
        let Some(page) = &self.page else {
            let (title, detail, style) = match &self.error {
                Some(error) => ("SECURITY DATA FAILED", error.to_string(), RED),
                None => (
                    "LOADING LIVE SECURITY DATA…",
                    format!("{} · SEC EDGAR + MARKET DATA", self.symbol),
                    AMBER,
                ),
            };
            frame.render_widget(
                Paragraph::new(vec![
                    Line::from(Span::styled(title, style)),
                    Line::from(""),
                    Line::from(Span::styled(detail, MUTED)),
                    Line::from(""),
                    Line::from(Span::styled(
                        "Press F9 or click this panel header to retry.",
                        MUTED,
                    )),
                ])
                .block(terminal_block("SEC", "LIVE RESEARCH")),
                area,
            );
            return;
        };
        let snapshot = &page.snapshot;
        let research = &page.research;
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(format!(" {} · {}  ", snapshot.symbol, snapshot.name), AMBER),
                Span::styled(
                    format!("{}  ", snapshot.last),
                    Style::new().fg(CYAN.into()).bold(),
                ),
                Span::styled(
                    format!(
                        "{}  {}  ",
                        snapshot.absolute_change, snapshot.percent_change
                    ),
                    GREEN,
                ),
                Span::styled(snapshot.session_summary.as_str(), MUTED),
            ]))
            .block(Block::new().borders(Borders::ALL).border_style(AMBER))
            .alignment(Alignment::Center),
            areas.header,
        );

        if self.research_view == ResearchView::Ownership {
            super::insider_chart::render(
                frame,
                areas.chart,
                &research.insider_transactions,
                self.selected_insider,
            );
        } else {
            let y_bounds = price_bounds(&snapshot.price_series);
            let x_max = snapshot
                .price_series
                .last()
                .map(|point| point.0)
                .unwrap_or(100.0)
                .max(1.0);
            let y_middle = (y_bounds[0] + y_bounds[1]) / 2.0;
            let chart = Chart::new(vec![Dataset::default()
                .name(format!("{} {}", snapshot.symbol, snapshot.last))
                .marker(symbols::Marker::Braille)
                .graph_type(GraphType::Line)
                .style(CYAN)
                .data(&snapshot.price_series)])
            .block(terminal_block("GP", "RECENT DAILY PRICE"))
            .x_axis(
                Axis::default()
                    .bounds([0.0, x_max])
                    .labels(["START", "RECENT", "LATEST"])
                    .style(MUTED),
            )
            .y_axis(
                Axis::default()
                    .bounds(y_bounds)
                    .labels([
                        format!("{:.2}", y_bounds[0]),
                        format!("{y_middle:.2}"),
                        format!("{:.2}", y_bounds[1]),
                    ])
                    .style(AMBER),
            );
            frame.render_widget(chart, areas.chart);
        }
        let research_tabs = ResearchView::ALL
            .into_iter()
            .enumerate()
            .map(|(index, view)| {
                let style = if view == self.research_view {
                    Style::new()
                        .bg(CYAN.into())
                        .fg(crate::ui::theme::BG.into())
                        .bold()
                } else {
                    Style::new().fg(MUTED.into())
                };
                Span::styled(format!(" {} {} ", index + 1, view.label()), style)
            })
            .collect::<Vec<_>>();
        frame.render_widget(Paragraph::new(Line::from(research_tabs)), areas.tabs);
        render_research(
            frame,
            areas.research,
            self.research_view,
            research,
            self.selected_insider,
        );
        let statistics = snapshot
            .statistics
            .iter()
            .take(8)
            .map(|(label, value)| [label.clone(), value.clone()])
            .collect::<Vec<_>>();
        render_pairs(
            frame,
            areas.statistics,
            "DES",
            "REFERENCE DATA",
            &statistics,
        );
        let source_status = vec![
            ["MARKET".to_owned(), snapshot.source.clone()],
            ["RESEARCH".to_owned(), research.source.clone()],
            [
                "ESTIMATES".to_owned(),
                availability(research.estimates.len(), "SEC DOES NOT PROVIDE").to_owned(),
            ],
            ["FORM 4".to_owned(), research.insider_status.clone()],
            ["OPEN".to_owned(), self.document_status.clone()],
            [
                "PEERS".to_owned(),
                availability(research.peers.len(), "NOT PROVIDED BY SEC").to_owned(),
            ],
            ["REFRESH".to_owned(), "F9 / CLICK HEADER".to_owned()],
        ];
        render_pairs(frame, areas.sources, "SRC", "SOURCE STATUS", &source_status);
    }

    fn capture_view(&self) -> WorkspaceViewState {
        let mut state = WorkspaceViewState::new(ID.as_str())
            .with_field("instrument_id", ViewValue::Text(self.instrument_id.clone()))
            .with_field("symbol", ViewValue::Text(self.symbol.clone()))
            .with_field(
                "research_view",
                ViewValue::Text(research_action_key(self.research_view).to_owned()),
            );
        if let Some(accession) = self.selected_insider_accession.as_ref().or_else(|| {
            self.page.as_ref().and_then(|page| {
                page.research
                    .insider_transactions
                    .get(self.selected_insider)
                    .map(|transaction| &transaction.accession)
            })
        }) {
            state = state.with_field(
                "selected_insider_accession",
                ViewValue::Text(accession.clone()),
            );
        }
        state
    }

    fn restore_view(&mut self, state: &WorkspaceViewState) -> ViewRestoreReport {
        if !state.workspace.eq_ignore_ascii_case(ID.as_str()) {
            return ViewRestoreReport::warning(format!(
                "saved state belongs to {}, not security",
                state.workspace
            ));
        }
        let mut report = ViewRestoreReport::default();
        let mut identity_changed = false;
        let instrument_id = state.fields.get("instrument_id");
        let symbol = state.fields.get("symbol");
        if instrument_id.is_some() || symbol.is_some() {
            match (
                instrument_id.and_then(ViewValue::as_text),
                symbol.and_then(ViewValue::as_text),
            ) {
                (Some(instrument_id), Some(symbol))
                    if valid_security_identity(instrument_id, symbol) =>
                {
                    identity_changed = self.symbol != symbol || self.instrument_id != instrument_id;
                    self.symbol = symbol.to_owned();
                    self.instrument_id = instrument_id.to_owned();
                    if identity_changed {
                        self.selected_insider = 0;
                        self.selected_insider_accession = None;
                        self.queue_refresh();
                    }
                    report.restored_fields += 2;
                }
                _ => {
                    report.skipped_fields +=
                        usize::from(instrument_id.is_some()) + usize::from(symbol.is_some());
                    report
                        .warnings
                        .push("security instrument identity is incomplete or invalid".to_owned());
                }
            }
        }
        if let Some(value) = state.fields.get("research_view") {
            match value.as_text().and_then(research_view_from_saved_key) {
                Some(view) => {
                    self.research_view = view;
                    report.restored_fields += 1;
                }
                None => {
                    report.skipped_fields += 1;
                    report
                        .warnings
                        .push("security research view is unavailable".to_owned());
                }
            }
        }
        if let Some(value) = state.fields.get("selected_insider_accession") {
            match value.as_text().filter(|value| valid_accession(value)) {
                Some(accession) => {
                    self.selected_insider_accession = Some(accession.to_owned());
                    if !identity_changed {
                        if let Some(page) = self.page.take() {
                            self.resolve_selected_insider(&page);
                            self.page = Some(page);
                        }
                    }
                    report.restored_fields += 1;
                }
                None => {
                    report.skipped_fields += 1;
                    report
                        .warnings
                        .push("security Form 4 selection is invalid".to_owned());
                }
            }
        }

        const KNOWN_FIELDS: [&str; 4] = [
            "instrument_id",
            "symbol",
            "research_view",
            "selected_insider_accession",
        ];
        let unknown = state
            .fields
            .keys()
            .filter(|field| !KNOWN_FIELDS.contains(&field.as_str()))
            .count();
        if unknown > 0 {
            report.skipped_fields += unknown;
            report
                .warnings
                .push(format!("ignored {unknown} future security field(s)"));
        }
        report
    }
}

fn research_view_from_saved_key(value: &str) -> Option<ResearchView> {
    ResearchView::ALL
        .into_iter()
        .find(|view| research_action_key(*view) == value)
}

fn valid_security_identity(instrument_id: &str, symbol: &str) -> bool {
    if instrument_id.trim().is_empty() || symbol.trim().is_empty() {
        return false;
    }
    let normalized = super::SecurityIdentity::from_terminal_symbol(symbol);
    normalized.terminal_symbol == symbol && symbol.split_whitespace().count() == 2
}

fn valid_accession(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn availability(count: usize, unavailable: &'static str) -> String {
    if count == 0 {
        unavailable.to_owned()
    } else {
        format!("{count} RECORDS")
    }
}

fn compact_number(value: f64) -> String {
    let absolute = value.abs();
    if absolute >= 1_000_000_000.0 {
        format!("{:.1}B", value / 1_000_000_000.0)
    } else if absolute >= 1_000_000.0 {
        format!("{:.1}M", value / 1_000_000.0)
    } else if absolute >= 1_000.0 {
        format!("{:.1}K", value / 1_000.0)
    } else {
        format!("{value:.0}")
    }
}

fn compact_usd(value: f64) -> String {
    format!("${}", compact_number(value))
}

fn price_bounds(series: &[(f64, f64)]) -> [f64; 2] {
    let (minimum, maximum) = series
        .iter()
        .map(|point| point.1)
        .filter(|value| value.is_finite())
        .fold(
            (f64::INFINITY, f64::NEG_INFINITY),
            |(minimum, maximum), value| (minimum.min(value), maximum.max(value)),
        );
    if !minimum.is_finite() || !maximum.is_finite() {
        return [0.0, 1.0];
    }
    let padding = ((maximum - minimum) * 0.08)
        .max(maximum.abs() * 0.01)
        .max(0.01);
    [minimum - padding, maximum + padding]
}

fn render_research(
    frame: &mut Frame,
    area: Rect,
    view: ResearchView,
    research: &SecurityResearch,
    selected_insider: usize,
) {
    match view {
        ResearchView::Financials if research.financials.is_empty() => render_unavailable(
            frame,
            area,
            "FA",
            "SEC COMPANY FACTS",
            "No comparable annual US-GAAP facts were returned.",
        ),
        ResearchView::Financials => {
            let financials = research.financials.iter().rev().take(3).collect::<Vec<_>>();
            let header = [
                "USD BN / SHARE".to_owned(),
                financials
                    .first()
                    .map_or("—", |value| value.period.as_str())
                    .to_owned(),
                financials
                    .get(1)
                    .map_or("—", |value| value.period.as_str())
                    .to_owned(),
                financials
                    .get(2)
                    .map_or("—", |value| value.period.as_str())
                    .to_owned(),
            ];
            let row = |label: &str, value: fn(&super::FinancialPeriod) -> &str| {
                styled_row([
                    label.to_owned(),
                    financials
                        .first()
                        .map_or("—", |period| value(period))
                        .to_owned(),
                    financials
                        .get(1)
                        .map_or("—", |period| value(period))
                        .to_owned(),
                    financials
                        .get(2)
                        .map_or("—", |period| value(period))
                        .to_owned(),
                ])
            };
            render_table(
                frame,
                area,
                "FA",
                "SEC COMPANY FACTS · ACTUAL REPORTED VALUES",
                header,
                vec![
                    row("REVENUE", |value| &value.revenue_billions),
                    row("OPERATING INCOME", |value| &value.operating_income_billions),
                    row("NET INCOME", |value| &value.net_income_billions),
                    row("DILUTED EPS", |value| &value.diluted_eps),
                ],
                [
                    Constraint::Percentage(34),
                    Constraint::Percentage(22),
                    Constraint::Percentage(22),
                    Constraint::Percentage(22),
                ],
            );
        }
        ResearchView::Estimates if research.estimates.is_empty() => render_unavailable(
            frame,
            area,
            "EE",
            "CONSENSUS ESTIMATES",
            "SEC EDGAR does not provide analyst consensus estimates.",
        ),
        ResearchView::Estimates => render_table(
            frame,
            area,
            "EE",
            "CONSENSUS ESTIMATES · RANGE",
            ["PERIOD", "REVENUE", "EPS", "HIGH", "LOW"],
            research
                .estimates
                .iter()
                .map(|value| {
                    styled_row([
                        value.period.clone(),
                        value.revenue.clone(),
                        value.eps.clone(),
                        value.eps_high.clone(),
                        value.eps_low.clone(),
                    ])
                })
                .collect(),
            [
                Constraint::Percentage(18),
                Constraint::Percentage(24),
                Constraint::Percentage(18),
                Constraint::Percentage(20),
                Constraint::Percentage(20),
            ],
        ),
        ResearchView::Ownership if research.insider_transactions.is_empty() => render_unavailable(
            frame,
            area,
            "OWN",
            "SEC FORM 4 INSIDER ACTIVITY",
            "No recent non-derivative Form 4 transactions were returned.",
        ),
        ResearchView::Ownership => render_table(
            frame,
            area,
            "OWN",
            "LIVE SEC FORM 4 · NON-DERIVATIVE TRANSACTIONS",
            ["DATE", "INSIDER", "ROLE", "CODE", "A/D", "SHARES", "VALUE"],
            research
                .insider_transactions
                .iter()
                .enumerate()
                .map(|(index, value)| {
                    let row = styled_row([
                        value.transaction_date.clone(),
                        value.owner.clone(),
                        value.role.clone(),
                        value.transaction_code.clone(),
                        value.acquisition_disposition.clone(),
                        compact_number(value.shares),
                        value
                            .value_usd
                            .map(compact_usd)
                            .unwrap_or_else(|| "—".to_owned()),
                    ]);
                    if index == selected_insider {
                        row.style(Style::new().bg(CYAN.into()).fg(BG.into()).bold())
                    } else {
                        row
                    }
                })
                .collect(),
            [
                Constraint::Percentage(14),
                Constraint::Percentage(24),
                Constraint::Percentage(22),
                Constraint::Percentage(8),
                Constraint::Percentage(8),
                Constraint::Percentage(12),
                Constraint::Percentage(12),
            ],
        ),
        ResearchView::Filings if research.filings.is_empty() => render_unavailable(
            frame,
            area,
            "FIL",
            "REGULATORY FILINGS",
            "SEC submissions returned no supported recent filings.",
        ),
        ResearchView::Filings => render_table(
            frame,
            area,
            "FIL",
            "LIVE SEC REGULATORY FILINGS",
            ["FILED", "FORM", "PERIOD", "DESCRIPTION", "ACCESSION"],
            research
                .filings
                .iter()
                .map(|value| {
                    styled_row([
                        value.filed.clone(),
                        value.form.clone(),
                        value.period.clone(),
                        value.description.clone(),
                        value.accession.clone(),
                    ])
                })
                .collect(),
            [
                Constraint::Percentage(15),
                Constraint::Percentage(9),
                Constraint::Percentage(15),
                Constraint::Percentage(27),
                Constraint::Percentage(34),
            ],
        ),
        ResearchView::Peers if research.peers.is_empty() => render_unavailable(
            frame,
            area,
            "RV",
            "RELATIVE VALUE",
            "SEC EDGAR does not define comparable-company peer sets.",
        ),
        ResearchView::Peers => render_table(
            frame,
            area,
            "RV",
            "RELATIVE VALUE · CANONICAL INSTRUMENT LINKED",
            ["SYMBOL", "COMPANY", "P/E", "EV/EBITDA", "REV GR", "GM"],
            research
                .peers
                .iter()
                .map(|value| {
                    styled_row([
                        value.symbol.clone(),
                        value.name.clone(),
                        value.price_to_earnings.clone(),
                        value.ev_to_ebitda.clone(),
                        value.revenue_growth.clone(),
                        value.gross_margin.clone(),
                    ])
                })
                .collect(),
            [
                Constraint::Percentage(13),
                Constraint::Percentage(25),
                Constraint::Percentage(13),
                Constraint::Percentage(18),
                Constraint::Percentage(16),
                Constraint::Percentage(15),
            ],
        ),
    }
}

fn render_unavailable(
    frame: &mut Frame,
    area: Rect,
    code: &'static str,
    title: &'static str,
    message: &'static str,
) {
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled("UNAVAILABLE FROM CURRENT LIVE SOURCES", AMBER)),
            Line::from(""),
            Line::from(Span::styled(message, MUTED)),
        ])
        .block(terminal_block(code, title)),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::security::{
        Filing, InsiderTransaction, PeerComparison, SecurityDocumentOpenError, SecurityIdentity,
        SecurityPage, SecurityResearch, SecuritySnapshot,
    };
    use crossterm::event::{KeyModifiers, MouseButton, MouseEventKind};
    use std::sync::Mutex;

    struct StubQuery;

    #[derive(Default)]
    struct StubDocumentOpener {
        opened: Mutex<Vec<String>>,
    }

    impl SecurityDocumentOpener for StubDocumentOpener {
        fn open(&self, url: &str) -> Result<(), SecurityDocumentOpenError> {
            self.opened
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(url.to_owned());
            Ok(())
        }
    }

    fn click(column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    impl SecurityQuery for StubQuery {
        fn load_security(&self, symbol: &str) -> Result<SecurityPage, SecurityError> {
            Ok(stub_page(symbol))
        }
    }

    fn stub_page(symbol: &str) -> SecurityPage {
        let identity = SecurityIdentity::from_terminal_symbol(symbol);
        SecurityPage {
            snapshot: SecuritySnapshot {
                symbol: identity.terminal_symbol.clone(),
                name: "TEST COMPANY".to_owned(),
                last: "1.00".to_owned(),
                absolute_change: "+0.00".to_owned(),
                percent_change: "+0.00%".to_owned(),
                session_summary: "TEST".to_owned(),
                price_series: vec![(0.0, 1.0)],
                statistics: Vec::new(),
                source: "TEST".to_owned(),
            },
            research: SecurityResearch {
                identity,
                financials: Vec::new(),
                estimates: Vec::new(),
                owners: Vec::new(),
                insider_transactions: Vec::new(),
                insider_status: "TEST".to_owned(),
                filings: Vec::new(),
                peers: Vec::new(),
                source: "TEST".to_owned(),
            },
        }
    }

    fn insider_transaction(accession: &str) -> InsiderTransaction {
        InsiderTransaction {
            filed: "2026-07-30".to_owned(),
            transaction_date: "2026-07-28".to_owned(),
            owner: "TEST INSIDER".to_owned(),
            role: "DIRECTOR".to_owned(),
            transaction_code: "S".to_owned(),
            acquisition_disposition: "DISP".to_owned(),
            shares: 100.0,
            price_per_share: Some(10.0),
            value_usd: Some(1_000.0),
            shares_after: Some(900.0),
            ownership_nature: "DIRECT".to_owned(),
            plan_10b5_1: false,
            accession: accession.to_owned(),
            document_url: Some(format!("https://www.sec.gov/{accession}-index.htm")),
        }
    }

    fn filing(accession: &str) -> Filing {
        Filing {
            filed: "2026-07-30".to_owned(),
            form: "10-Q".to_owned(),
            period: "2026-06-30".to_owned(),
            description: "QUARTERLY REPORT".to_owned(),
            accession: accession.to_owned(),
            document_url: Some(format!("https://www.sec.gov/{accession}-index.htm")),
        }
    }

    fn peer(symbol: &str) -> PeerComparison {
        PeerComparison {
            symbol: symbol.to_owned(),
            name: "TEST PEER".to_owned(),
            price_to_earnings: "20.0x".to_owned(),
            ev_to_ebitda: "15.0x".to_owned(),
            revenue_growth: "+5.0%".to_owned(),
            gross_margin: "40.0%".to_owned(),
        }
    }

    #[test]
    fn research_commands_change_view_without_replacing_symbol() {
        let mut workspace = SecurityWorkspace::new(Arc::new(StubQuery));
        workspace.handle_command(&CommandInvocation {
            function: "FORM4".into(),
            args: vec![],
        });
        assert_eq!(workspace.research_view, ResearchView::Ownership);
        assert_eq!(workspace.symbol, "AAPL US");
    }

    #[test]
    fn typed_view_restores_instrument_tab_and_stable_form4_selection() {
        let mut source = SecurityWorkspace::new(Arc::new(StubQuery));
        let mut source_page = stub_page("AAPL US");
        source_page.research.identity = SecurityIdentity::from_sec_cik(320_193, "AAPL");
        source.instrument_id = source_page.research.identity.instrument_id.to_string();
        source_page.research.insider_transactions = vec![
            insider_transaction("FORM4-OLDER"),
            insider_transaction("FORM4-SELECTED"),
        ];
        source.page = Some(source_page);
        source.research_view = ResearchView::Ownership;
        source.selected_insider = 1;
        source.remember_selected_insider();
        let state = source.capture_view();

        assert_eq!(
            state.fields.get("instrument_id"),
            Some(&ViewValue::Text("sec:cik:0000320193".to_owned()))
        );
        assert_eq!(
            state.fields.get("research_view"),
            Some(&ViewValue::Text("ownership".to_owned()))
        );
        assert_eq!(
            state.fields.get("selected_insider_accession"),
            Some(&ViewValue::Text("FORM4-SELECTED".to_owned()))
        );

        let mut restored = SecurityWorkspace::new(Arc::new(StubQuery));
        let mut reordered = stub_page("AAPL US");
        reordered.research.identity = SecurityIdentity::from_sec_cik(320_193, "AAPL");
        reordered.research.insider_transactions = vec![
            insider_transaction("FORM4-SELECTED"),
            insider_transaction("FORM4-OLDER"),
        ];
        restored.instrument_id = reordered.research.identity.instrument_id.to_string();
        restored.page = Some(reordered);
        let report = restored.restore_view(&state);

        assert_eq!(report.restored_fields, 4);
        assert_eq!(report.skipped_fields, 0);
        assert_eq!(restored.symbol, "AAPL US");
        assert_eq!(restored.research_view, ResearchView::Ownership);
        assert_eq!(restored.selected_insider, 0);
        assert_eq!(
            restored.selected_insider_accession.as_deref(),
            Some("FORM4-SELECTED")
        );
        assert_eq!(restored.capture_view(), state);

        let mut transitioning = SecurityWorkspace::new(Arc::new(StubQuery));
        let mut stale_page = stub_page("AAPL US");
        stale_page.research.insider_transactions = vec![insider_transaction("FORM4-AAPL")];
        transitioning.page = Some(stale_page);
        let next_instrument = WorkspaceViewState::new(ID.as_str())
            .with_field(
                "instrument_id",
                ViewValue::Text("terminal:msft-us".to_owned()),
            )
            .with_field("symbol", ViewValue::Text("MSFT US".to_owned()))
            .with_field(
                "selected_insider_accession",
                ViewValue::Text("FORM4-MSFT".to_owned()),
            );
        transitioning.restore_view(&next_instrument);
        assert_eq!(
            transitioning.selected_insider_accession.as_deref(),
            Some("FORM4-MSFT")
        );

        let mut refreshed_page = stub_page("MSFT US");
        refreshed_page.research.insider_transactions = vec![insider_transaction("FORM4-MSFT")];
        transitioning.resolve_selected_insider(&refreshed_page);
        assert_eq!(transitioning.selected_insider, 0);
        assert_eq!(
            transitioning.selected_insider_accession.as_deref(),
            Some("FORM4-MSFT")
        );
    }

    #[test]
    fn typed_view_degrades_invalid_future_and_missing_form4_state() {
        let mut workspace = SecurityWorkspace::new(Arc::new(StubQuery));
        let mut page = stub_page("AAPL US");
        page.research.insider_transactions = vec![insider_transaction("FORM4-CURRENT")];
        workspace.page = Some(page);
        let invalid = WorkspaceViewState::new(ID.as_str())
            .with_field("instrument_id", ViewValue::Text(String::new()))
            .with_field("symbol", ViewValue::Text("aapl".to_owned()))
            .with_field("research_view", ViewValue::Text("future".to_owned()))
            .with_field(
                "selected_insider_accession",
                ViewValue::Text("../../unsafe".to_owned()),
            )
            .with_field("future_field", ViewValue::Boolean(true));
        let report = workspace.restore_view(&invalid);

        assert_eq!(report.restored_fields, 0);
        assert_eq!(report.skipped_fields, 5);
        assert_eq!(report.warnings.len(), 4);
        assert_eq!(workspace.symbol, "AAPL US");
        assert_eq!(workspace.research_view, ResearchView::Financials);

        let missing = WorkspaceViewState::new(ID.as_str()).with_field(
            "selected_insider_accession",
            ViewValue::Text("FORM4-RETIRED".to_owned()),
        );
        let report = workspace.restore_view(&missing);
        assert_eq!(report.restored_fields, 1);
        assert_eq!(workspace.selected_insider, 0);
        assert_eq!(
            workspace.document_status,
            "SAVED FORM 4 SELECTION UNAVAILABLE · USING FIRST VISIBLE ROW"
        );
        assert_eq!(
            workspace.selected_insider_accession.as_deref(),
            Some("FORM4-CURRENT")
        );
    }

    #[test]
    fn vim_keys_select_and_open_form4_source_filing() {
        let opener = Arc::new(StubDocumentOpener::default());
        let mut workspace = SecurityWorkspace::with_symbol_and_document_opener(
            Arc::new(StubQuery),
            "AAPL US",
            opener.clone(),
        );
        let mut page = stub_page("AAPL US");
        page.research.insider_transactions = vec![
            insider_transaction("0000000000-26-000001"),
            insider_transaction("0000000000-26-000002"),
        ];
        workspace.page = Some(page);
        workspace.research_view = ResearchView::Ownership;

        assert!(workspace.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE,)));
        assert!(workspace.handle_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE,)));

        assert_eq!(workspace.selected_insider, 1);
        assert_eq!(
            workspace.selected_insider_accession.as_deref(),
            Some("0000000000-26-000002")
        );
        assert_eq!(
            *opener
                .opened
                .lock()
                .unwrap_or_else(|error| error.into_inner()),
            vec!["https://www.sec.gov/0000000000-26-000002-index.htm"]
        );
        assert_eq!(workspace.document_status, "OPENED 0000000000-26-000002");
    }

    #[test]
    fn news_shortcut_emits_instrument_scoped_command() {
        let mut workspace = SecurityWorkspace::new(Arc::new(StubQuery));
        assert!(workspace.handle_key(KeyEvent::new(
            KeyCode::Char('n'),
            crossterm::event::KeyModifiers::NONE,
        )));
        assert_eq!(
            workspace.poll_intents(),
            vec![AppIntent::DispatchCommand {
                command: "NEWS --symbol=AAPL".into(),
                origin: ID,
            }]
        );
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
        let tabs = Layout::vertical([Constraint::Length(1), Constraint::Min(3)]).split(left[1])[0];
        let second_tab = tabs
            .x
            .saturating_add(" 1 FA FINANCIALS ".chars().count() as u16);

        assert!(workspace.handle_mouse(click(second_tab, tabs.y), area));

        assert_eq!(workspace.research_view, ResearchView::Estimates);
    }

    #[test]
    fn clicking_form4_rows_changes_the_selection() {
        let mut workspace = SecurityWorkspace::new(Arc::new(StubQuery));
        let mut page = stub_page("AAPL US");
        page.research.insider_transactions = vec![
            insider_transaction("0000000000-26-000001"),
            insider_transaction("0000000000-26-000002"),
        ];
        workspace.page = Some(page);
        workspace.research_view = ResearchView::Ownership;
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
        let table = Layout::vertical([Constraint::Length(1), Constraint::Min(3)]).split(left[1])[1];

        assert!(workspace.handle_mouse(click(table.x + 2, table.y + 4), area));

        assert_eq!(workspace.selected_insider, 1);
    }

    #[test]
    fn clicking_form4_chart_selects_the_nearest_dated_transaction() {
        let mut workspace = SecurityWorkspace::new(Arc::new(StubQuery));
        let mut first = insider_transaction("0000000000-26-000001");
        first.transaction_date = "2026-06-01".to_owned();
        let mut second = insider_transaction("0000000000-26-000002");
        second.transaction_date = "2026-07-28".to_owned();
        let mut page = stub_page("AAPL US");
        page.research.insider_transactions = vec![first, second];
        workspace.page = Some(page);
        workspace.research_view = ResearchView::Ownership;
        let area = Rect::new(0, 0, 160, 40);
        let rows = Layout::vertical([Constraint::Length(4), Constraint::Min(12)]).split(area);
        let grid = Layout::horizontal([
            Constraint::Percentage(62),
            Constraint::Percentage(19),
            Constraint::Percentage(19),
        ])
        .split(rows[1]);
        let chart = Layout::vertical([Constraint::Percentage(58), Constraint::Percentage(42)])
            .split(grid[0])[0];

        assert!(workspace.handle_mouse(
            click(chart.x + chart.width.saturating_sub(2), chart.y + 2),
            area,
        ));

        assert_eq!(workspace.selected_insider, 1);
    }

    #[test]
    fn visible_actions_route_tabs_charts_documents_and_peers() {
        let opener = Arc::new(StubDocumentOpener::default());
        let mut workspace = SecurityWorkspace::with_symbol_and_document_opener(
            Arc::new(StubQuery),
            "AAPL US",
            opener.clone(),
        );
        let mut page = stub_page("AAPL US");
        page.research.insider_transactions = vec![insider_transaction("FORM4-1")];
        page.research.filings = vec![filing("FILING-1")];
        page.research.peers = vec![peer("MSFT")];
        workspace.page = Some(page);
        let area = Rect::new(0, 0, 160, 40);

        let financial_actions = workspace.actions(area);
        assert!(financial_actions
            .iter()
            .any(|action| action.id == "refresh"));
        assert!(financial_actions
            .iter()
            .any(|action| action.id == "view:ownership"));
        assert!(financial_actions
            .iter()
            .any(|action| action.id == "chart:AAPL US" && action.preferred));
        assert!(workspace.activate_action("chart:AAPL US"));
        assert_eq!(
            std::mem::take(&mut workspace.pending_intents),
            vec![AppIntent::DispatchCommand {
                command: "CHART AAPL US".to_owned(),
                origin: ID,
            }]
        );

        assert!(workspace.activate_action("view:ownership"));
        assert!(workspace
            .actions(area)
            .iter()
            .any(|action| action.id == "ownership:0:FORM4-1" && action.preferred));
        assert!(workspace.activate_action("ownership:0:FORM4-1"));

        assert!(workspace.activate_action("view:filings"));
        assert!(workspace
            .actions(area)
            .iter()
            .any(|action| action.id == "filing:0:FILING-1" && action.preferred));
        assert!(workspace.activate_action("filing:0:FILING-1"));
        assert_eq!(
            *opener
                .opened
                .lock()
                .unwrap_or_else(|error| error.into_inner()),
            vec![
                "https://www.sec.gov/FORM4-1-index.htm",
                "https://www.sec.gov/FILING-1-index.htm",
            ]
        );

        assert!(workspace.activate_action("view:peers"));
        assert!(workspace
            .actions(area)
            .iter()
            .any(|action| action.id == "peer:0:MSFT" && action.preferred));
        assert!(workspace.activate_action("peer:0:MSFT"));
        assert_eq!(
            std::mem::take(&mut workspace.pending_intents),
            vec![AppIntent::DispatchCommand {
                command: "SEC MSFT".to_owned(),
                origin: ID,
            }]
        );
    }

    #[test]
    fn actions_are_viewport_safe_and_reject_stale_identities() {
        let mut workspace = SecurityWorkspace::new(Arc::new(StubQuery));
        let mut page = stub_page("AAPL US");
        page.research.filings = vec![filing("FILING-1")];
        workspace.page = Some(page);
        workspace.research_view = ResearchView::Filings;
        let area = Rect::new(0, 0, 80, 24);
        let actions = workspace.actions(area);

        assert!(actions.iter().all(|action| {
            action.area.width > 0
                && action.area.height > 0
                && action.area.x >= area.x
                && action.area.y >= area.y
                && action.area.right() <= area.right()
                && action.area.bottom() <= area.bottom()
        }));
        assert!(actions
            .iter()
            .any(|action| action.id == "filing:0:FILING-1"));

        workspace.page.as_mut().unwrap().research.filings[0].accession = "NEW-FILING".to_owned();
        assert!(!workspace.activate_action("filing:0:FILING-1"));
        workspace.research_view = ResearchView::Financials;
        assert!(!workspace.activate_action("filing:0:NEW-FILING"));
        assert!(!workspace.activate_action("chart:STALE US"));
        assert!(!workspace.activate_action("invented"));
    }

    #[test]
    fn loading_and_failed_states_expose_only_a_preferred_retry_action() {
        let mut workspace = SecurityWorkspace::new(Arc::new(StubQuery));
        workspace.page = None;
        let actions = workspace.actions(Rect::new(0, 0, 80, 24));

        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].id, "refresh");
        assert!(actions[0].preferred);
        let generation = workspace.desired_generation;
        assert!(workspace.activate_action("refresh"));
        assert_eq!(workspace.desired_generation, generation.wrapping_add(1));
    }

    #[test]
    fn security_provider_never_blocks_workspace_construction() {
        struct SlowQuery;

        impl SecurityQuery for SlowQuery {
            fn load_security(&self, symbol: &str) -> Result<SecurityPage, SecurityError> {
                std::thread::sleep(std::time::Duration::from_millis(200));
                Ok(stub_page(symbol))
            }
        }

        let started = std::time::Instant::now();
        let workspace = SecurityWorkspace::new(Arc::new(SlowQuery));
        assert!(started.elapsed() < std::time::Duration::from_millis(100));
        assert!(workspace.page.is_none());
    }

    #[test]
    fn completed_security_page_is_applied_from_the_background_worker() {
        let mut workspace = SecurityWorkspace::new(Arc::new(StubQuery));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        while workspace.page.is_none() && std::time::Instant::now() < deadline {
            workspace.poll_refresh();
            std::thread::yield_now();
        }

        assert_eq!(
            workspace
                .page
                .as_ref()
                .map(|page| page.snapshot.symbol.as_str()),
            Some("AAPL US")
        );
        assert!(workspace.error.is_none());
    }
}
