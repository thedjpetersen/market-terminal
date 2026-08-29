//! Responsive watchlist density and Unicode sparkline downsampling adapt the
//! corresponding table behavior from `makeev/alphai-tui` commit
//! `9143d2e1176d0a67a9f26960427cf370187fc2e6` (MIT, Copyright (c) 2026
//! Mikhail Makeev). This implementation plots only observations received by
//! Market Terminal's typed market-data ports; see `THIRD_PARTY_NOTICES.md`.

use std::{
    cell::Cell as StateCell,
    cmp::Ordering,
    collections::{HashMap, VecDeque},
    sync::{
        mpsc::{sync_channel, Receiver, SyncSender, TrySendError},
        Arc,
    },
    time::{Duration, Instant},
};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Cell, Paragraph, Row, Table},
    Frame,
};

use crate::{
    app::{
        AppIntent, CommandInvocation, ViewRestoreReport, ViewValue, Workspace, WorkspaceAction,
        WorkspaceDescriptor, WorkspaceViewState,
    },
    features::market_data::{
        DataQuality, MarketDataError, MarketDataQuery, Price, Quantity, QuoteSnapshot,
        QuoteSubscription, QuoteSubscriptionRequest, SubscriptionMetrics,
    },
    ui::{
        components::terminal_block,
        scroll_key, table_row_at,
        theme::{AMBER, BG, CYAN, GREEN, INK, MUTED, RED, YELLOW},
    },
};

use super::{
    MonitorColumn, SortDirection, SortField, WatchlistCatalog, WatchlistDefinition, WatchlistItem,
    ID,
};

const DEFAULT_SNAPSHOT_REFRESH_INTERVAL: Duration = Duration::from_secs(60);
const SESSION_TRACE_CAPACITY: usize = 64;

#[derive(Debug, Clone, Copy)]
struct MonitorAreas {
    header: Rect,
    table: Rect,
    footer: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MonitorControl {
    Open,
    SortField,
    SortDirection,
    Columns,
    Refresh,
}

impl MonitorControl {
    const fn action_id(self) -> &'static str {
        match self {
            Self::Open => "control:open",
            Self::SortField => "control:sort-field",
            Self::SortDirection => "control:sort-direction",
            Self::Columns => "control:columns",
            Self::Refresh => "control:refresh",
        }
    }

    fn from_action_id(id: &str) -> Option<Self> {
        match id {
            "control:open" => Some(Self::Open),
            "control:sort-field" => Some(Self::SortField),
            "control:sort-direction" => Some(Self::SortDirection),
            "control:columns" => Some(Self::Columns),
            "control:refresh" => Some(Self::Refresh),
            _ => None,
        }
    }
}

struct MonitorFooterSegment {
    key: &'static str,
    detail: String,
    control: Option<MonitorControl>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ColumnPreset {
    Configured,
    Trading,
    Compact,
}

impl ColumnPreset {
    const fn next(self) -> Self {
        match self {
            Self::Configured => Self::Trading,
            Self::Trading => Self::Compact,
            Self::Compact => Self::Configured,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Configured => "CONFIGURED",
            Self::Trading => "TRADING",
            Self::Compact => "COMPACT",
        }
    }

    const fn key(self) -> &'static str {
        match self {
            Self::Configured => "configured",
            Self::Trading => "trading",
            Self::Compact => "compact",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "configured" => Some(Self::Configured),
            "trading" => Some(Self::Trading),
            "compact" => Some(Self::Compact),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
struct MonitorRow {
    item: WatchlistItem,
    quote: Option<QuoteSnapshot>,
    session_trace: VecDeque<f64>,
    last_trace_observation: Option<(String, Option<u64>)>,
}

struct QuoteRefresh {
    generation: u64,
    instruments: Vec<crate::foundation::InstrumentId>,
}

struct QuoteRefreshResult {
    generation: u64,
    result: Result<Vec<QuoteSnapshot>, MarketDataError>,
}

pub struct WatchlistWorkspace {
    market_data: Arc<dyn MarketDataQuery>,
    refresh_sender: SyncSender<QuoteRefresh>,
    refresh_receiver: Receiver<QuoteRefreshResult>,
    next_refresh_generation: u64,
    applied_refresh_generation: u64,
    snapshot_refresh_interval: Duration,
    next_snapshot_refresh: Instant,
    catalog: Arc<dyn WatchlistCatalog>,
    definition: WatchlistDefinition,
    rows: Vec<MonitorRow>,
    selected: usize,
    viewport_top: StateCell<usize>,
    viewport_rows: StateCell<usize>,
    column_preset: ColumnPreset,
    status: String,
    subscription: Option<Box<dyn QuoteSubscription>>,
    pending_intents: Vec<AppIntent>,
}

impl WatchlistWorkspace {
    pub fn new(market_data: Arc<dyn MarketDataQuery>, catalog: Arc<dyn WatchlistCatalog>) -> Self {
        Self::with_snapshot_refresh_interval(
            market_data,
            catalog,
            DEFAULT_SNAPSHOT_REFRESH_INTERVAL,
        )
    }

    pub fn with_snapshot_refresh_interval(
        market_data: Arc<dyn MarketDataQuery>,
        catalog: Arc<dyn WatchlistCatalog>,
        snapshot_refresh_interval: Duration,
    ) -> Self {
        let (refresh_sender, worker_receiver) = sync_channel::<QuoteRefresh>(1);
        let (worker_sender, refresh_receiver) = sync_channel::<QuoteRefreshResult>(1);
        let worker_market_data = market_data.clone();
        std::thread::Builder::new()
            .name("watchlist-market-data".to_owned())
            .spawn(move || {
                while let Ok(refresh) = worker_receiver.recv() {
                    let result = worker_market_data.quote_snapshots(&refresh.instruments);
                    if worker_sender
                        .send(QuoteRefreshResult {
                            generation: refresh.generation,
                            result,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .expect("watchlist market-data worker should start");
        let definition = catalog
            .load_watchlist(None)
            .unwrap_or_else(|| WatchlistDefinition::new("empty", "EMPTY", Vec::new()));
        let mut workspace = Self {
            market_data,
            refresh_sender,
            refresh_receiver,
            next_refresh_generation: 0,
            applied_refresh_generation: 0,
            snapshot_refresh_interval: snapshot_refresh_interval.max(Duration::from_millis(1)),
            next_snapshot_refresh: Instant::now(),
            catalog,
            definition,
            rows: Vec::new(),
            selected: 0,
            viewport_top: StateCell::new(0),
            viewport_rows: StateCell::new(0),
            column_preset: ColumnPreset::Configured,
            status: String::new(),
            subscription: None,
            pending_intents: Vec::new(),
        };
        workspace.reset_rows();
        workspace.refresh();
        workspace.start_subscription();
        workspace
    }

    fn load_watchlist(&mut self, name: Option<&str>) {
        let display_name = name.unwrap_or("DEFAULT").trim();
        if let Some(definition) = self.catalog.load_watchlist(name) {
            self.definition = definition;
            self.selected = 0;
            self.viewport_top.set(0);
            self.column_preset = ColumnPreset::Configured;
            self.reset_rows();
            self.refresh();
            self.start_subscription();
        } else {
            self.status = format!("WATCHLIST NOT FOUND: {display_name}");
        }
    }

    fn refresh(&mut self) {
        self.next_snapshot_refresh = Instant::now() + self.snapshot_refresh_interval;
        let ids = self
            .definition
            .items
            .iter()
            .map(|item| item.instrument_id.clone())
            .collect::<Vec<_>>();
        if ids.is_empty() {
            self.status = "WATCHLIST IS EMPTY".to_owned();
            return;
        }
        let generation = self.next_refresh_generation.wrapping_add(1);
        match self.refresh_sender.try_send(QuoteRefresh {
            generation,
            instruments: ids,
        }) {
            Ok(()) => {
                self.next_refresh_generation = generation;
                self.status = "LOADING LIVE QUOTES…".to_owned();
            }
            Err(TrySendError::Full(_)) => {
                self.status = "QUOTE REFRESH ALREADY IN PROGRESS".to_owned();
            }
            Err(TrySendError::Disconnected(_)) => {
                self.status = "QUOTE WORKER IS UNAVAILABLE".to_owned();
            }
        }
    }

    fn reset_rows(&mut self) {
        self.rows = self
            .definition
            .items
            .iter()
            .cloned()
            .map(|item| MonitorRow {
                item,
                quote: None,
                session_trace: VecDeque::with_capacity(SESSION_TRACE_CAPACITY),
                last_trace_observation: None,
            })
            .collect();
    }

    fn poll_refresh(&mut self) {
        while let Ok(refresh) = self.refresh_receiver.try_recv() {
            if refresh.generation < self.applied_refresh_generation {
                continue;
            }
            self.applied_refresh_generation = refresh.generation;
            match refresh.result {
                Ok(quotes) => {
                    let quotes = quotes
                        .into_iter()
                        .map(|quote| (quote.instrument_id.clone(), quote))
                        .collect::<HashMap<_, _>>();
                    for row in &mut self.rows {
                        if let Some(quote) = quotes.get(&row.item.instrument_id) {
                            apply_quote(row, quote.clone());
                        }
                    }
                    self.sort_rows_preserving_view();
                    self.status = format!("{} QUOTES LOADED", self.rows.len());
                }
                Err(error) => {
                    self.status = if self.rows.iter().all(|row| row.quote.is_none()) {
                        error.to_string()
                    } else {
                        format!("LAST KNOWN GOOD · {error}")
                    };
                }
            }
        }
    }

    fn start_subscription(&mut self) {
        if let Some(subscription) = &mut self.subscription {
            subscription.cancel();
        }
        self.subscription = None;
        let instruments = self
            .definition
            .items
            .iter()
            .map(|item| item.instrument_id.clone())
            .collect::<Vec<_>>();
        if instruments.is_empty() {
            return;
        }
        let request = QuoteSubscriptionRequest::new(instruments, self.definition.items.len())
            .expect("non-empty watchlist has non-zero stream capacity");
        match self.market_data.subscribe_quotes(request) {
            Ok(subscription) => self.subscription = Some(subscription),
            Err(MarketDataError::Unsupported(_)) => {}
            Err(error) => self.status = format!("SNAPSHOT MODE · {error}"),
        }
    }

    fn poll_subscription(&mut self) {
        let Some(subscription) = &mut self.subscription else {
            return;
        };
        let id = subscription.id();
        let result = subscription.drain();
        let metrics = subscription.metrics();
        match result {
            Ok(updates) if updates.is_empty() => {}
            Ok(updates) => {
                let updates = updates
                    .into_iter()
                    .map(|update| (update.snapshot.instrument_id.clone(), update.snapshot))
                    .collect::<HashMap<_, _>>();
                for row in &mut self.rows {
                    if let Some(snapshot) = updates.get(&row.item.instrument_id) {
                        apply_quote(row, snapshot.clone());
                    }
                }
                self.sort_rows_preserving_view();
                self.status = stream_status(id.value(), metrics);
            }
            Err(MarketDataError::Cancelled) => {
                self.subscription = None;
                self.status = "STREAM CANCELLED · LAST KNOWN GOOD".to_owned();
            }
            Err(error) => {
                self.status = format!("STREAM DEGRADED · LAST KNOWN GOOD · {error}");
            }
        }
    }

    fn move_selection(&mut self, delta: isize) {
        if self.rows.is_empty() {
            self.selected = 0;
            return;
        }
        self.selected = self
            .selected
            .saturating_add_signed(delta)
            .min(self.rows.len() - 1);
        self.reveal_selection();
    }

    fn cycle_sort(&mut self) {
        self.definition.sort.field = self.definition.sort.field.next();
        self.sort_rows();
        self.selected = 0;
        self.viewport_top.set(0);
    }

    fn toggle_sort_direction(&mut self) {
        self.definition.sort.direction = self.definition.sort.direction.toggled();
        self.sort_rows();
        self.selected = 0;
        self.viewport_top.set(0);
    }

    fn sort_rows(&mut self) {
        let field = self.definition.sort.field;
        let direction = self.definition.sort.direction;
        self.rows.sort_by(|left, right| {
            let order = compare_rows(left, right, field)
                .then_with(|| left.item.instrument_id.cmp(&right.item.instrument_id));
            match direction {
                SortDirection::Ascending => order,
                SortDirection::Descending => order.reverse(),
            }
        });
    }

    fn sort_rows_preserving_view(&mut self) {
        let selected = self
            .rows
            .get(self.selected)
            .map(|row| row.item.instrument_id.clone());
        let top = self
            .rows
            .get(self.viewport_top.get())
            .map(|row| row.item.instrument_id.clone());
        self.sort_rows();
        if let Some(selected) = selected {
            self.selected = self
                .row_index(&selected)
                .unwrap_or_else(|| self.selected.min(self.rows.len().saturating_sub(1)));
        } else {
            self.selected = self.selected.min(self.rows.len().saturating_sub(1));
        }
        if let Some(top) = top {
            self.viewport_top
                .set(self.row_index(&top).unwrap_or(self.viewport_top.get()));
        }
        self.reveal_selection();
    }

    fn row_index(&self, instrument_id: &crate::foundation::InstrumentId) -> Option<usize> {
        self.rows
            .iter()
            .position(|row| &row.item.instrument_id == instrument_id)
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
        self.viewport_top.set(top);
        self.clamp_viewport();
    }

    fn clamp_viewport(&self) {
        let capacity = self.viewport_rows.get();
        let maximum = self.rows.len().saturating_sub(capacity.max(1));
        self.viewport_top.set(self.viewport_top.get().min(maximum));
    }

    fn update_viewport(&self, table: Rect) -> (usize, usize) {
        let capacity = usize::from(table.height.saturating_sub(4));
        self.viewport_rows.set(capacity);
        self.reveal_selection();
        (self.viewport_top.get(), capacity)
    }

    fn visible_columns(&self, available_width: u16) -> Vec<MonitorColumn> {
        let desired = match self.column_preset {
            ColumnPreset::Configured => self.definition.visible_columns.clone(),
            ColumnPreset::Trading => WatchlistDefinition::trading_columns(),
            ColumnPreset::Compact => WatchlistDefinition::compact_columns(),
        };
        responsive_columns(desired, available_width)
    }

    fn footer_segments(&self) -> Vec<MonitorFooterSegment> {
        vec![
            MonitorFooterSegment {
                key: " ↑↓/JK ",
                detail: "SELECT  ".to_owned(),
                control: None,
            },
            MonitorFooterSegment {
                key: "ENTER ",
                detail: "OPEN SEC  ".to_owned(),
                control: Some(MonitorControl::Open),
            },
            MonitorFooterSegment {
                key: "S ",
                detail: format!("SORT {}  ", self.definition.sort.field.label()),
                control: Some(MonitorControl::SortField),
            },
            MonitorFooterSegment {
                key: "SHIFT-S ",
                detail: format!("DIR {}  ", self.definition.sort.direction.marker()),
                control: Some(MonitorControl::SortDirection),
            },
            MonitorFooterSegment {
                key: "C ",
                detail: format!("COLUMNS {}  ", self.column_preset.label()),
                control: Some(MonitorControl::Columns),
            },
            MonitorFooterSegment {
                key: "R ",
                detail: "REFRESH".to_owned(),
                control: Some(MonitorControl::Refresh),
            },
        ]
    }

    fn control_areas(&self, footer: Rect) -> Vec<(MonitorControl, Rect)> {
        let mut x = footer.x;
        let mut controls = Vec::new();
        for segment in self.footer_segments() {
            let width = (segment.key.chars().count() + segment.detail.chars().count()) as u16;
            if let Some(control) = segment.control {
                let visible_width = width.min(footer.right().saturating_sub(x));
                if visible_width > 0 {
                    controls.push((control, Rect::new(x, footer.y, visible_width, 1)));
                }
            }
            x = x.saturating_add(width);
        }
        controls
    }

    fn activate_control(&mut self, control: MonitorControl) -> bool {
        match control {
            MonitorControl::Open => self.open_selected(),
            MonitorControl::SortField => self.cycle_sort(),
            MonitorControl::SortDirection => self.toggle_sort_direction(),
            MonitorControl::Columns => self.column_preset = self.column_preset.next(),
            MonitorControl::Refresh => self.refresh(),
        }
        true
    }

    fn open_selected(&mut self) {
        if let Some(row) = self.rows.get(self.selected) {
            self.pending_intents.push(AppIntent::DispatchCommand {
                command: format!("SEC {}", row.item.symbol),
                origin: ID,
            });
        }
    }
}

impl Workspace for WatchlistWorkspace {
    fn descriptor(&self) -> WorkspaceDescriptor {
        WorkspaceDescriptor {
            id: ID,
            label: "MONITOR",
            hotkey: 'w',
            commands: &["MON", "WATCH", "WATCHLIST"],
        }
    }

    fn is_favorite(&self) -> bool {
        true
    }

    fn handle_command(&mut self, invocation: &CommandInvocation) -> bool {
        let name = invocation.args.join(" ");
        self.load_watchlist((!name.is_empty()).then_some(name.as_str()));
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
            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.toggle_sort_direction();
                true
            }
            KeyCode::Char('s') => {
                self.cycle_sort();
                true
            }
            KeyCode::Char('c') => {
                self.column_preset = self.column_preset.next();
                true
            }
            KeyCode::Char('r') => {
                self.refresh();
                true
            }
            KeyCode::Enter => {
                self.open_selected();
                true
            }
            KeyCode::Char('a') => {
                if let Some(row) = self.rows.get(self.selected) {
                    self.pending_intents.push(AppIntent::DispatchCommand {
                        command: format!("SHEET INSERT {}", row.item.symbol),
                        origin: ID,
                    });
                }
                true
            }
            _ => false,
        }
    }

    fn handle_mouse(&mut self, event: MouseEvent, area: Rect) -> bool {
        let areas = monitor_areas(area);
        let (viewport_top, viewport_rows) = self.update_viewport(areas.table);
        if let Some(index) = table_row_at(event, areas.table, viewport_rows.min(self.rows.len())) {
            let index = viewport_top.saturating_add(index);
            self.selected = index;
            return true;
        }
        if crate::ui::is_primary_click(event, areas.footer) {
            for (control, control_area) in self.control_areas(areas.footer) {
                if crate::ui::contains(control_area, event.column, event.row) {
                    return self.activate_control(control);
                }
            }
            return true;
        }
        if let Some(key) = scroll_key(event, areas.table) {
            return self.handle_key(key);
        }
        false
    }

    fn actions(&self, area: Rect) -> Vec<WorkspaceAction> {
        let areas = monitor_areas(area);
        let (viewport_top, viewport_rows) = self.update_viewport(areas.table);
        let visible_rows = viewport_rows.min(self.rows.len().saturating_sub(viewport_top));
        let preferred_row = (self.selected >= viewport_top
            && self.selected < viewport_top.saturating_add(visible_rows))
        .then_some(self.selected)
        .or_else(|| (visible_rows > 0).then_some(viewport_top));
        let mut actions = self
            .rows
            .iter()
            .skip(viewport_top)
            .take(visible_rows)
            .enumerate()
            .map(|(ordinal, row)| {
                let index = viewport_top.saturating_add(ordinal);
                let mut action = WorkspaceAction::new(
                    format!("row:{index}:{}", row.item.instrument_id.as_str()),
                    format!("Open {} security research", row.item.symbol),
                    Rect::new(
                        areas.table.x.saturating_add(1),
                        areas
                            .table
                            .y
                            .saturating_add(3 + u16::try_from(ordinal).unwrap_or(u16::MAX)),
                        areas.table.width.saturating_sub(2),
                        1,
                    ),
                );
                if Some(index) == preferred_row {
                    action = action.preferred();
                }
                action
            })
            .collect::<Vec<_>>();

        actions.extend(
            self.control_areas(areas.footer)
                .into_iter()
                .filter(|(control, _)| *control != MonitorControl::Open || !self.rows.is_empty())
                .map(|(control, area)| {
                    let label = match control {
                        MonitorControl::Open => "Open selected security".to_owned(),
                        MonitorControl::SortField => format!(
                            "Cycle sort field from {}",
                            self.definition.sort.field.label()
                        ),
                        MonitorControl::SortDirection => format!(
                            "Toggle sort direction from {}",
                            self.definition.sort.direction.marker()
                        ),
                        MonitorControl::Columns => {
                            format!("Cycle columns from {}", self.column_preset.label())
                        }
                        MonitorControl::Refresh => "Refresh monitor quotes".to_owned(),
                    };
                    WorkspaceAction::new(control.action_id(), label, area)
                }),
        );
        actions
    }

    fn activate_action(&mut self, id: &str) -> bool {
        if let Some(control) = MonitorControl::from_action_id(id) {
            return self.activate_control(control);
        }
        let Some(row_id) = id.strip_prefix("row:") else {
            return false;
        };
        let Some((index, instrument_id)) = row_id.split_once(':') else {
            return false;
        };
        let Ok(index) = index.parse::<usize>() else {
            return false;
        };
        let Some(row) = self.rows.get(index) else {
            return false;
        };
        if row.item.instrument_id.as_str() != instrument_id {
            return false;
        }
        self.selected = index;
        self.open_selected();
        true
    }

    fn poll_intents(&mut self) -> Vec<AppIntent> {
        self.poll_refresh();
        self.poll_subscription();
        if self.subscription.is_none() && Instant::now() >= self.next_snapshot_refresh {
            self.refresh();
        }
        std::mem::take(&mut self.pending_intents)
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let areas = monitor_areas(area);
        let (viewport_top, viewport_rows) = self.update_viewport(areas.table);
        let quality_counts = quality_counts(&self.rows);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    format!(" {} ", self.definition.name),
                    Style::new().bg(AMBER.into()).fg(BG.into()).bold(),
                ),
                Span::styled(format!(" {} INSTRUMENTS  ", self.rows.len()), INK),
                Span::styled(
                    format!(
                        "RT {}  DELAYED/STALE {}  BLOCKED {}  ",
                        quality_counts.0, quality_counts.1, quality_counts.2
                    ),
                    MUTED,
                ),
                Span::styled(self.status.as_str(), YELLOW),
            ]))
            .block(terminal_block("MON", "MARKET MONITOR")),
            areas.header,
        );

        let columns = self.visible_columns(areas.table.width.saturating_sub(2));
        let widths = columns
            .iter()
            .map(|column| column_width(*column))
            .collect::<Vec<_>>();
        let header = Row::new(columns.iter().map(|column| column.label()))
            .style(Style::new().fg(AMBER.into()).bold())
            .bottom_margin(1);
        let rows = self
            .rows
            .iter()
            .enumerate()
            .skip(viewport_top)
            .take(viewport_rows)
            .map(|(index, row)| {
                let selected = index == self.selected;
                Row::new(
                    columns
                        .iter()
                        .map(|column| render_cell(row, *column, selected)),
                )
                .style(if selected {
                    Style::new().bg(CYAN.into()).fg(BG.into()).bold()
                } else {
                    Style::new()
                })
            });
        frame.render_widget(
            Table::new(rows, widths)
                .header(header)
                .column_spacing(1)
                .block(terminal_block("WL", "LIVE SNAPSHOTS")),
            areas.table,
        );

        let mut footer = Vec::new();
        for segment in self.footer_segments() {
            footer.push(Span::styled(segment.key, AMBER));
            footer.push(Span::styled(segment.detail, MUTED));
        }
        frame.render_widget(Paragraph::new(Line::from(footer)), areas.footer);
    }

    fn capture_view(&self) -> WorkspaceViewState {
        let mut state = WorkspaceViewState::new(ID.as_str())
            .with_field("watchlist_id", ViewValue::Text(self.definition.id.clone()))
            .with_field(
                "sort_field",
                ViewValue::Text(sort_field_key(self.definition.sort.field).to_owned()),
            )
            .with_field(
                "sort_direction",
                ViewValue::Text(sort_direction_key(self.definition.sort.direction).to_owned()),
            )
            .with_field(
                "column_preset",
                ViewValue::Text(self.column_preset.key().to_owned()),
            )
            .with_field(
                "configured_columns",
                ViewValue::TextList(
                    self.definition
                        .visible_columns
                        .iter()
                        .map(|column| monitor_column_key(*column).to_owned())
                        .collect(),
                ),
            );
        if let Some(row) = self.rows.get(self.selected) {
            state = state.with_field(
                "selected_instrument_id",
                ViewValue::Text(row.item.instrument_id.as_str().to_owned()),
            );
        }
        if let Some(row) = self.rows.get(self.viewport_top.get()) {
            state = state.with_field(
                "top_instrument_id",
                ViewValue::Text(row.item.instrument_id.as_str().to_owned()),
            );
        }
        state
    }

    fn restore_view(&mut self, state: &WorkspaceViewState) -> ViewRestoreReport {
        if !state.workspace.eq_ignore_ascii_case(ID.as_str()) {
            return ViewRestoreReport::warning(format!(
                "saved state belongs to {}, not monitor",
                state.workspace
            ));
        }

        let mut report = ViewRestoreReport::default();
        if let Some(value) = state.fields.get("watchlist_id") {
            match value.as_text().filter(|value| valid_view_token(value, 64)) {
                Some(id) if id.eq_ignore_ascii_case(&self.definition.id) => {
                    report.restored_fields += 1;
                }
                Some(id) => match self.catalog.load_watchlist(Some(id)) {
                    Some(definition) => {
                        self.definition = definition;
                        self.reset_rows();
                        self.refresh();
                        self.start_subscription();
                        report.restored_fields += 1;
                    }
                    None => {
                        report.skipped_fields += 1;
                        report
                            .warnings
                            .push("saved monitor watchlist is unavailable".to_owned());
                    }
                },
                None => {
                    report.skipped_fields += 1;
                    report
                        .warnings
                        .push("saved monitor watchlist identity is invalid".to_owned());
                }
            }
        }

        restore_sort_field(state, &mut self.definition.sort.field, &mut report);
        restore_sort_direction(state, &mut self.definition.sort.direction, &mut report);
        restore_columns(state, &mut self.definition.visible_columns, &mut report);
        if let Some(value) = state.fields.get("column_preset") {
            match value.as_text().and_then(ColumnPreset::parse) {
                Some(preset) => {
                    self.column_preset = preset;
                    report.restored_fields += 1;
                }
                None => {
                    report.skipped_fields += 1;
                    report
                        .warnings
                        .push("saved monitor column preset is unavailable".to_owned());
                }
            }
        }

        self.sort_rows();
        self.selected = 0;
        self.viewport_top.set(0);
        restore_row_identity(
            state,
            "selected_instrument_id",
            "selected row",
            &self.rows,
            &mut self.selected,
            &mut report,
        );
        let mut top = 0;
        restore_row_identity(
            state,
            "top_instrument_id",
            "viewport anchor",
            &self.rows,
            &mut top,
            &mut report,
        );
        self.viewport_top.set(top);
        self.clamp_viewport();

        const KNOWN_FIELDS: [&str; 7] = [
            "watchlist_id",
            "sort_field",
            "sort_direction",
            "column_preset",
            "configured_columns",
            "selected_instrument_id",
            "top_instrument_id",
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
                .push(format!("ignored {unknown} future monitor field(s)"));
        }
        if !state.children.is_empty() {
            report.skipped_fields += state.children.len();
            report.warnings.push(format!(
                "ignored {} future monitor child state(s)",
                state.children.len()
            ));
        }
        report
    }
}

fn sort_field_key(field: SortField) -> &'static str {
    match field {
        SortField::Symbol => "symbol",
        SortField::Last => "last",
        SortField::ChangePercent => "change_percent",
        SortField::Volume => "volume",
    }
}

fn parse_sort_field(value: &str) -> Option<SortField> {
    match value {
        "symbol" => Some(SortField::Symbol),
        "last" => Some(SortField::Last),
        "change_percent" => Some(SortField::ChangePercent),
        "volume" => Some(SortField::Volume),
        _ => None,
    }
}

fn sort_direction_key(direction: SortDirection) -> &'static str {
    match direction {
        SortDirection::Ascending => "ascending",
        SortDirection::Descending => "descending",
    }
}

fn parse_sort_direction(value: &str) -> Option<SortDirection> {
    match value {
        "ascending" => Some(SortDirection::Ascending),
        "descending" => Some(SortDirection::Descending),
        _ => None,
    }
}

fn monitor_column_key(column: MonitorColumn) -> &'static str {
    match column {
        MonitorColumn::Symbol => "symbol",
        MonitorColumn::Last => "last",
        MonitorColumn::Change => "change",
        MonitorColumn::ChangePercent => "change_percent",
        MonitorColumn::Bid => "bid",
        MonitorColumn::Ask => "ask",
        MonitorColumn::Volume => "volume",
        MonitorColumn::DayRange => "day_range",
        MonitorColumn::Sparkline => "sparkline",
        MonitorColumn::Quality => "quality",
        MonitorColumn::AsOf => "as_of",
    }
}

fn parse_monitor_column(value: &str) -> Option<MonitorColumn> {
    match value {
        "symbol" => Some(MonitorColumn::Symbol),
        "last" => Some(MonitorColumn::Last),
        "change" => Some(MonitorColumn::Change),
        "change_percent" => Some(MonitorColumn::ChangePercent),
        "bid" => Some(MonitorColumn::Bid),
        "ask" => Some(MonitorColumn::Ask),
        "volume" => Some(MonitorColumn::Volume),
        "day_range" => Some(MonitorColumn::DayRange),
        "sparkline" => Some(MonitorColumn::Sparkline),
        "quality" => Some(MonitorColumn::Quality),
        "as_of" => Some(MonitorColumn::AsOf),
        _ => None,
    }
}

fn restore_sort_field(
    state: &WorkspaceViewState,
    target: &mut SortField,
    report: &mut ViewRestoreReport,
) {
    let Some(value) = state.fields.get("sort_field") else {
        return;
    };
    match value.as_text().and_then(parse_sort_field) {
        Some(value) => {
            *target = value;
            report.restored_fields += 1;
        }
        None => {
            report.skipped_fields += 1;
            report
                .warnings
                .push("saved monitor sort field is unavailable".to_owned());
        }
    }
}

fn restore_sort_direction(
    state: &WorkspaceViewState,
    target: &mut SortDirection,
    report: &mut ViewRestoreReport,
) {
    let Some(value) = state.fields.get("sort_direction") else {
        return;
    };
    match value.as_text().and_then(parse_sort_direction) {
        Some(value) => {
            *target = value;
            report.restored_fields += 1;
        }
        None => {
            report.skipped_fields += 1;
            report
                .warnings
                .push("saved monitor sort direction is unavailable".to_owned());
        }
    }
}

fn restore_columns(
    state: &WorkspaceViewState,
    target: &mut Vec<MonitorColumn>,
    report: &mut ViewRestoreReport,
) {
    let Some(value) = state.fields.get("configured_columns") else {
        return;
    };
    let parsed = value.as_text_list().and_then(|values| {
        let columns = values
            .iter()
            .map(|value| parse_monitor_column(value))
            .collect::<Option<Vec<_>>>()?;
        (!columns.is_empty()
            && columns.len() <= WatchlistDefinition::full_columns().len()
            && columns.contains(&MonitorColumn::Symbol)
            && columns
                .iter()
                .enumerate()
                .all(|(index, column)| !columns[..index].contains(column)))
        .then_some(columns)
    });
    match parsed {
        Some(columns) => {
            *target = columns;
            report.restored_fields += 1;
        }
        None => {
            report.skipped_fields += 1;
            report
                .warnings
                .push("saved monitor column set is invalid".to_owned());
        }
    }
}

fn restore_row_identity(
    state: &WorkspaceViewState,
    field: &str,
    label: &str,
    rows: &[MonitorRow],
    target: &mut usize,
    report: &mut ViewRestoreReport,
) {
    let Some(value) = state.fields.get(field) else {
        return;
    };
    match value.as_text().filter(|value| valid_view_token(value, 256)) {
        Some(instrument_id) => match rows
            .iter()
            .position(|row| row.item.instrument_id.as_str() == instrument_id)
        {
            Some(index) => {
                *target = index;
                report.restored_fields += 1;
            }
            None => {
                report.skipped_fields += 1;
                report
                    .warnings
                    .push(format!("saved monitor {label} is no longer available"));
            }
        },
        None => {
            report.skipped_fields += 1;
            report
                .warnings
                .push(format!("saved monitor {label} identity is invalid"));
        }
    }
}

fn valid_view_token(value: &str, maximum_bytes: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum_bytes
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

fn monitor_areas(area: Rect) -> MonitorAreas {
    let areas = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(8),
        Constraint::Length(2),
    ])
    .split(area);
    MonitorAreas {
        header: areas[0],
        table: areas[1],
        footer: areas[2],
    }
}

impl Drop for WatchlistWorkspace {
    fn drop(&mut self) {
        if let Some(subscription) = &mut self.subscription {
            subscription.cancel();
        }
    }
}

fn apply_quote(row: &mut MonitorRow, quote: QuoteSnapshot) {
    if let Some(last) = quote.last.filter(|_| quote.quality.is_usable()) {
        let observation = (
            quote.provenance.source_timestamp.as_str().to_owned(),
            quote.provenance.sequence,
        );
        if row.last_trace_observation.as_ref() != Some(&observation) {
            if row.session_trace.len() == SESSION_TRACE_CAPACITY {
                row.session_trace.pop_front();
            }
            row.session_trace.push_back(last.value());
            row.last_trace_observation = Some(observation);
        }
    }
    row.quote = Some(quote);
}

fn stream_status(id: u64, metrics: SubscriptionMetrics) -> String {
    format!(
        "LIVE #{id} · RX {} COALESCED {} DROPPED {}",
        metrics.received, metrics.coalesced, metrics.dropped
    )
}

fn compare_rows(left: &MonitorRow, right: &MonitorRow, field: SortField) -> Ordering {
    match field {
        SortField::Symbol => left.item.symbol.cmp(&right.item.symbol),
        SortField::Last => quote_number(left, |quote| quote.last.map(Price::value))
            .total_cmp(&quote_number(right, |quote| quote.last.map(Price::value))),
        SortField::ChangePercent => quote_number(left, |quote| {
            quote.change.map(|change| change.percent.value())
        })
        .total_cmp(&quote_number(right, |quote| {
            quote.change.map(|change| change.percent.value())
        })),
        SortField::Volume => quote_number(left, |quote| {
            quote.volume.map(|volume| volume.value() as f64)
        })
        .total_cmp(&quote_number(right, |quote| {
            quote.volume.map(|volume| volume.value() as f64)
        })),
    }
}

fn quote_number(row: &MonitorRow, read: impl FnOnce(&QuoteSnapshot) -> Option<f64>) -> f64 {
    row.quote
        .as_ref()
        .and_then(read)
        .unwrap_or(f64::NEG_INFINITY)
}

fn render_cell(row: &MonitorRow, column: MonitorColumn, selected: bool) -> Cell<'static> {
    let quote = row.quote.as_ref();
    let value = match column {
        MonitorColumn::Symbol => row.item.symbol.clone(),
        MonitorColumn::Last => format_price(quote.and_then(|quote| quote.last)),
        MonitorColumn::Change => {
            format_signed_price(quote.and_then(|quote| quote.change.map(|change| change.absolute)))
        }
        MonitorColumn::ChangePercent => quote
            .and_then(|quote| quote.change)
            .map(|change| format!("{:+.2}%", change.percent.value()))
            .unwrap_or_else(|| "--".to_owned()),
        MonitorColumn::Bid => format_price(quote.and_then(|quote| quote.bid)),
        MonitorColumn::Ask => format_price(quote.and_then(|quote| quote.ask)),
        MonitorColumn::Volume => format_volume(quote.and_then(|quote| quote.volume)),
        MonitorColumn::DayRange => quote
            .and_then(QuoteSnapshot::day_range)
            .map(|(low, high)| format!("{:.2}–{:.2}", low.value(), high.value()))
            .unwrap_or_else(|| "--".to_owned()),
        MonitorColumn::Sparkline => spark_line(&row.session_trace, 16),
        MonitorColumn::Quality => quote
            .map(|quote| quote.quality.label())
            .unwrap_or_else(|| "NO SNAPSHOT".to_owned()),
        MonitorColumn::AsOf => quote
            .map(|quote| short_time(quote.as_of.as_str()))
            .unwrap_or_else(|| "--".to_owned()),
    };
    let style = if selected {
        Style::new().bg(CYAN.into()).fg(BG.into()).bold()
    } else {
        cell_style(quote, column, &value)
    };
    Cell::from(value).style(style)
}

fn cell_style(quote: Option<&QuoteSnapshot>, column: MonitorColumn, value: &str) -> Style {
    if matches!(column, MonitorColumn::Quality) {
        return match quote.map(|quote| quote.quality) {
            Some(DataQuality::RealTime) => Style::new().fg(GREEN.into()),
            Some(
                DataQuality::Delayed { .. } | DataQuality::Stale { .. } | DataQuality::Derived,
            ) => Style::new().fg(YELLOW.into()),
            _ => Style::new().fg(RED.into()),
        };
    }
    if matches!(
        column,
        MonitorColumn::Change | MonitorColumn::ChangePercent | MonitorColumn::Sparkline
    ) {
        let direction = if column == MonitorColumn::Sparkline {
            quote
                .and_then(|quote| quote.change)
                .map(|change| change.absolute.value())
                .unwrap_or_default()
        } else if value.starts_with('+') {
            1.0
        } else if value.starts_with('-') {
            -1.0
        } else {
            0.0
        };
        return if direction > 0.0 {
            Style::new().fg(GREEN.into())
        } else if direction < 0.0 {
            Style::new().fg(RED.into())
        } else {
            Style::new().fg(INK.into())
        };
    }
    Style::new().fg(INK.into())
}

fn column_width(column: MonitorColumn) -> Constraint {
    Constraint::Length(column_width_value(column))
}

fn column_width_value(column: MonitorColumn) -> u16 {
    match column {
        MonitorColumn::Symbol => 12,
        MonitorColumn::Last | MonitorColumn::Change | MonitorColumn::Bid | MonitorColumn::Ask => 12,
        MonitorColumn::ChangePercent => 10,
        MonitorColumn::Volume => 12,
        MonitorColumn::DayRange => 22,
        MonitorColumn::Sparkline => 16,
        MonitorColumn::Quality => 16,
        MonitorColumn::AsOf => 12,
    }
}

fn responsive_columns(mut columns: Vec<MonitorColumn>, available_width: u16) -> Vec<MonitorColumn> {
    for expendable in [
        MonitorColumn::AsOf,
        MonitorColumn::Ask,
        MonitorColumn::Bid,
        MonitorColumn::Volume,
        MonitorColumn::Change,
        MonitorColumn::DayRange,
        MonitorColumn::Quality,
        MonitorColumn::Sparkline,
        MonitorColumn::ChangePercent,
    ] {
        if columns_width(&columns) <= available_width {
            break;
        }
        if let Some(index) = columns.iter().position(|column| *column == expendable) {
            columns.remove(index);
        }
    }
    columns
}

fn columns_width(columns: &[MonitorColumn]) -> u16 {
    columns
        .iter()
        .copied()
        .map(column_width_value)
        .sum::<u16>()
        .saturating_add(columns.len().saturating_sub(1) as u16)
}

/// Downsamples the bounded provider-observation trace into Unicode blocks.
fn spark_line(values: &VecDeque<f64>, width: usize) -> String {
    const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    if values.is_empty() || width == 0 {
        return "--".to_owned();
    }
    let chunk = (values.len() as f64 / width as f64).max(1.0);
    let mut sampled = Vec::with_capacity(width);
    let mut offset = 0.0;
    while (offset as usize) < values.len() && sampled.len() < width {
        let start = offset as usize;
        let end = (((offset + chunk) as usize).max(start + 1)).min(values.len());
        let average = values.range(start..end).sum::<f64>() / (end - start) as f64;
        sampled.push(average);
        offset += chunk;
    }
    let (low, high) = sampled
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), value| {
            (low.min(*value), high.max(*value))
        });
    let span = (high - low).max(1e-9);
    sampled
        .iter()
        .map(|value| BARS[(((value - low) / span) * 7.0).round() as usize])
        .collect()
}

fn format_price(price: Option<Price>) -> String {
    price
        .map(|price| format!("{:.2}", price.value()))
        .unwrap_or_else(|| "--".to_owned())
}

fn format_signed_price(price: Option<Price>) -> String {
    price
        .map(|price| format!("{:+.2}", price.value()))
        .unwrap_or_else(|| "--".to_owned())
}

fn format_volume(volume: Option<Quantity>) -> String {
    let Some(volume) = volume.map(Quantity::value) else {
        return "--".to_owned();
    };
    if volume >= 1_000_000_000 {
        format!("{:.2}B", volume as f64 / 1_000_000_000.0)
    } else if volume >= 1_000_000 {
        format!("{:.2}M", volume as f64 / 1_000_000.0)
    } else if volume >= 1_000 {
        format!("{:.1}K", volume as f64 / 1_000.0)
    } else {
        volume.to_string()
    }
}

fn short_time(timestamp: &str) -> String {
    timestamp
        .split_once('T')
        .map(|(_, time)| time.trim_end_matches('Z').to_owned())
        .unwrap_or_else(|| timestamp.to_owned())
}

fn quality_counts(rows: &[MonitorRow]) -> (usize, usize, usize) {
    rows.iter().fold((0, 0, 0), |mut counts, row| {
        match row.quote.as_ref().map(|quote| quote.quality) {
            Some(DataQuality::RealTime) => counts.0 += 1,
            Some(
                DataQuality::Delayed { .. } | DataQuality::Stale { .. } | DataQuality::Derived,
            ) => counts.1 += 1,
            _ => counts.2 += 1,
        }
        counts
    })
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

    use super::*;
    use crate::features::market_data::{
        CacheStatus, CanonicalInstrumentId, DataProvenance, HistoryRequest, Percent, PriceBar,
        PriceChange, ProviderId, UtcTimestamp,
    };
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

    struct StubMarketData;

    fn click(column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    impl MarketDataQuery for StubMarketData {
        fn quote_snapshots(
            &self,
            instruments: &[CanonicalInstrumentId],
        ) -> Result<Vec<QuoteSnapshot>, MarketDataError> {
            Ok(instruments
                .iter()
                .enumerate()
                .map(|(index, instrument_id)| QuoteSnapshot {
                    instrument_id: instrument_id.clone(),
                    symbol: instrument_id.as_str().to_ascii_uppercase(),
                    currency: "USD".to_owned(),
                    last: Some(Price::new(100.0 + index as f64)),
                    change: Some(PriceChange {
                        absolute: Price::new(index as f64),
                        percent: Percent::new(index as f64),
                    }),
                    bid: Some(Price::new(99.0)),
                    ask: Some(Price::new(101.0)),
                    day_low: Some(Price::new(98.5)),
                    day_high: Some(Price::new(102.5)),
                    volume: Some(Quantity::new(1_000 + index as u64)),
                    as_of: UtcTimestamp::new("2026-08-25T20:00:00Z"),
                    quality: DataQuality::RealTime,
                    provenance: DataProvenance {
                        provider: ProviderId::new("stub"),
                        source_timestamp: UtcTimestamp::new("2026-08-25T20:00:00Z"),
                        received_at: UtcTimestamp::new("2026-08-25T20:00:01Z"),
                        sequence: Some(index as u64),
                        cache_status: CacheStatus::Live,
                    },
                })
                .collect())
        }

        fn price_history(
            &self,
            _request: &HistoryRequest,
        ) -> Result<Vec<PriceBar>, MarketDataError> {
            Ok(Vec::new())
        }
    }

    struct StubCatalog;

    impl WatchlistCatalog for StubCatalog {
        fn load_watchlist(&self, name: Option<&str>) -> Option<WatchlistDefinition> {
            let symbols = match name.map(str::to_ascii_uppercase).as_deref() {
                None => &["AAPL", "MSFT"][..],
                Some("MACRO") => &["EURUSD", "SPX"][..],
                _ => return None,
            };
            Some(WatchlistDefinition::new(
                name.unwrap_or("DEFAULT"),
                name.unwrap_or("DEFAULT"),
                symbols
                    .iter()
                    .map(|symbol| {
                        WatchlistItem::new(
                            CanonicalInstrumentId::new(symbol.to_ascii_lowercase()),
                            *symbol,
                            *symbol,
                        )
                    })
                    .collect(),
            ))
        }
    }

    struct LongCatalog;

    impl WatchlistCatalog for LongCatalog {
        fn load_watchlist(&self, name: Option<&str>) -> Option<WatchlistDefinition> {
            if name.is_some_and(|name| !name.eq_ignore_ascii_case("long")) {
                return None;
            }
            Some(
                WatchlistDefinition::new(
                    "long",
                    "LONG RECOVERY MONITOR",
                    (0..16)
                        .map(|index| {
                            WatchlistItem::new(
                                CanonicalInstrumentId::new(format!("test:listed:s{index:02}")),
                                format!("S{index:02}"),
                                format!("Security {index:02}"),
                            )
                        })
                        .collect(),
                )
                .with_columns(vec![
                    MonitorColumn::Symbol,
                    MonitorColumn::Last,
                    MonitorColumn::ChangePercent,
                    MonitorColumn::Volume,
                    MonitorColumn::Quality,
                ]),
            )
        }
    }

    struct CountingMarketData {
        requests: Arc<AtomicUsize>,
    }

    impl MarketDataQuery for CountingMarketData {
        fn quote_snapshots(
            &self,
            instruments: &[CanonicalInstrumentId],
        ) -> Result<Vec<QuoteSnapshot>, MarketDataError> {
            self.requests.fetch_add(1, AtomicOrdering::Relaxed);
            StubMarketData.quote_snapshots(instruments)
        }

        fn price_history(
            &self,
            request: &HistoryRequest,
        ) -> Result<Vec<PriceBar>, MarketDataError> {
            StubMarketData.price_history(request)
        }
    }

    struct SlowMarketData;

    impl MarketDataQuery for SlowMarketData {
        fn quote_snapshots(
            &self,
            instruments: &[CanonicalInstrumentId],
        ) -> Result<Vec<QuoteSnapshot>, MarketDataError> {
            std::thread::sleep(std::time::Duration::from_millis(100));
            StubMarketData.quote_snapshots(instruments)
        }

        fn price_history(
            &self,
            request: &HistoryRequest,
        ) -> Result<Vec<PriceBar>, MarketDataError> {
            StubMarketData.price_history(request)
        }
    }

    #[test]
    fn slow_live_snapshot_loading_does_not_block_workspace_construction() {
        let started = std::time::Instant::now();
        let workspace = WatchlistWorkspace::new(Arc::new(SlowMarketData), Arc::new(StubCatalog));

        assert!(started.elapsed() < std::time::Duration::from_millis(75));
        assert_eq!(workspace.rows.len(), 2);
        assert_eq!(workspace.status, "LOADING LIVE QUOTES…");
    }

    #[test]
    fn query_only_providers_are_polled_on_a_bounded_interval() {
        let requests = Arc::new(AtomicUsize::new(0));
        let mut workspace = WatchlistWorkspace::with_snapshot_refresh_interval(
            Arc::new(CountingMarketData {
                requests: requests.clone(),
            }),
            Arc::new(StubCatalog),
            Duration::from_millis(2),
        );
        let first_deadline = Instant::now() + Duration::from_secs(1);
        while requests.load(AtomicOrdering::Relaxed) < 1 && Instant::now() < first_deadline {
            std::thread::yield_now();
        }
        assert_eq!(requests.load(AtomicOrdering::Relaxed), 1);

        std::thread::sleep(Duration::from_millis(3));
        workspace.poll_intents();
        let second_deadline = Instant::now() + Duration::from_secs(1);
        while requests.load(AtomicOrdering::Relaxed) < 2 && Instant::now() < second_deadline {
            workspace.poll_intents();
            std::thread::yield_now();
        }
        assert_eq!(requests.load(AtomicOrdering::Relaxed), 2);
    }

    #[test]
    fn aliases_and_named_watchlist_commands_are_supported() {
        let mut workspace =
            WatchlistWorkspace::new(Arc::new(StubMarketData), Arc::new(StubCatalog));
        assert_eq!(
            workspace.descriptor().commands,
            &["MON", "WATCH", "WATCHLIST"]
        );

        workspace.handle_command(&CommandInvocation {
            function: "MON".to_owned(),
            args: vec!["MACRO".to_owned()],
        });

        assert_eq!(workspace.definition.name, "MACRO");
        assert_eq!(workspace.rows.len(), 2);
    }

    #[test]
    fn keyboard_selection_opens_canonical_row_in_security() {
        let mut workspace =
            WatchlistWorkspace::new(Arc::new(StubMarketData), Arc::new(StubCatalog));
        workspace.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        workspace.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(
            workspace.poll_intents(),
            vec![AppIntent::DispatchCommand {
                command: "SEC MSFT".to_owned(),
                origin: ID,
            }]
        );
    }

    #[test]
    fn clicking_a_monitor_row_selects_it() {
        let mut workspace =
            WatchlistWorkspace::new(Arc::new(StubMarketData), Arc::new(StubCatalog));

        assert!(workspace.handle_mouse(click(2, 7), Rect::new(0, 0, 120, 30)));

        assert_eq!(workspace.selected, 1);
    }

    #[test]
    fn visible_actions_route_rows_and_discrete_footer_controls() {
        let mut workspace =
            WatchlistWorkspace::new(Arc::new(StubMarketData), Arc::new(StubCatalog));
        let area = Rect::new(0, 0, 160, 30);
        let actions = workspace.actions(area);

        let first = actions
            .iter()
            .find(|action| action.id.starts_with("row:0:"))
            .unwrap();
        assert!(first.preferred);
        assert!(actions
            .iter()
            .any(|action| action.id == "control:sort-field"));
        assert!(actions
            .iter()
            .any(|action| action.id == "control:sort-direction"));
        assert!(actions.iter().any(|action| action.id == "control:columns"));
        assert!(actions.iter().any(|action| action.id == "control:refresh"));
        assert!(actions.iter().all(|action| {
            action.area.x >= area.x
                && action.area.y >= area.y
                && action.area.right() <= area.right()
                && action.area.bottom() <= area.bottom()
        }));

        assert!(workspace.activate_action("control:sort-field"));
        assert_eq!(workspace.definition.sort.field, SortField::Last);
        let direction = workspace.definition.sort.direction;
        assert!(workspace.activate_action("control:sort-direction"));
        assert_eq!(workspace.definition.sort.direction, direction.toggled());
        assert!(workspace.activate_action("control:columns"));
        assert_eq!(workspace.column_preset, ColumnPreset::Trading);

        let second = workspace
            .actions(area)
            .into_iter()
            .find(|action| action.id.starts_with("row:1:"))
            .unwrap()
            .id;
        assert!(workspace.activate_action(&second));
        assert!(matches!(
            workspace.poll_intents().as_slice(),
            [AppIntent::DispatchCommand { command, origin }]
                if command.starts_with("SEC ") && *origin == ID
        ));
        assert!(!workspace.activate_action("row:1:stale-identity"));
        assert!(!workspace.activate_action("control:unknown"));
    }

    #[test]
    fn narrow_action_geometry_contains_only_visible_rows_and_controls() {
        let workspace = WatchlistWorkspace::new(Arc::new(StubMarketData), Arc::new(StubCatalog));
        let area = Rect::new(0, 0, 42, 18);
        let actions = workspace.actions(area);

        assert!(actions.iter().any(|action| action.id.starts_with("row:")));
        assert!(!actions.iter().any(|action| action.id == "control:refresh"));
        assert!(actions.iter().all(|action| {
            action.area.x >= area.x
                && action.area.y >= area.y
                && action.area.right() <= area.right()
                && action.area.bottom() <= area.bottom()
        }));
    }

    #[test]
    fn sort_and_column_controls_are_deterministic() {
        let mut workspace =
            WatchlistWorkspace::new(Arc::new(StubMarketData), Arc::new(StubCatalog));
        workspace.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        workspace.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));

        assert_eq!(workspace.definition.sort.field, SortField::Last);
        assert_eq!(workspace.column_preset, ColumnPreset::Trading);
    }

    #[test]
    fn typed_monitor_view_round_trips_sort_columns_selection_and_scroll() {
        let mut source = WatchlistWorkspace::new(Arc::new(StubMarketData), Arc::new(LongCatalog));
        let area = Rect::new(0, 0, 100, 14);
        source.actions(area);
        source.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        source.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::SHIFT));
        source.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
        for _ in 0..11 {
            source.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        }
        source.actions(area);

        let saved = source.capture_view();
        assert_eq!(
            saved.fields.get("watchlist_id"),
            Some(&ViewValue::Text("long".to_owned()))
        );
        assert_eq!(
            saved.fields.get("sort_field"),
            Some(&ViewValue::Text("last".to_owned()))
        );
        assert_eq!(
            saved.fields.get("sort_direction"),
            Some(&ViewValue::Text("descending".to_owned()))
        );
        assert_eq!(
            saved.fields.get("column_preset"),
            Some(&ViewValue::Text("trading".to_owned()))
        );
        assert_ne!(
            saved.fields.get("selected_instrument_id"),
            saved.fields.get("top_instrument_id")
        );

        let mut restored = WatchlistWorkspace::new(Arc::new(StubMarketData), Arc::new(LongCatalog));
        let report = restored.restore_view(&saved);

        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert_eq!(report.skipped_fields, 0);
        assert_eq!(restored.capture_view(), saved);
        let actions = restored.actions(area);
        let selected_id = saved
            .fields
            .get("selected_instrument_id")
            .and_then(ViewValue::as_text)
            .unwrap();
        assert!(actions
            .iter()
            .any(|action| { action.preferred && action.id.ends_with(selected_id) }));
    }

    #[test]
    fn monitor_view_degrades_each_invalid_or_retired_field_independently() {
        let mut workspace =
            WatchlistWorkspace::new(Arc::new(StubMarketData), Arc::new(StubCatalog));
        let state = WorkspaceViewState::new(ID.as_str())
            .with_field("watchlist_id", ViewValue::Text("retired".to_owned()))
            .with_field("sort_field", ViewValue::Text("alpha".to_owned()))
            .with_field("sort_direction", ViewValue::Text("sideways".to_owned()))
            .with_field(
                "configured_columns",
                ViewValue::TextList(vec!["last".to_owned(), "last".to_owned()]),
            )
            .with_field("column_preset", ViewValue::Text("dense".to_owned()))
            .with_field(
                "selected_instrument_id",
                ViewValue::Text("missing:instrument".to_owned()),
            )
            .with_field("future_field", ViewValue::Boolean(true))
            .with_child(WorkspaceViewState::new("future"));

        let report = workspace.restore_view(&state);

        assert_eq!(report.restored_fields, 0);
        assert_eq!(report.skipped_fields, 8);
        assert_eq!(workspace.definition.id, "DEFAULT");
        assert_eq!(workspace.definition.sort.field, SortField::Symbol);
        assert!(report
            .warnings
            .iter()
            .any(|warning| warning.contains("watchlist is unavailable")));
        assert!(report
            .warnings
            .iter()
            .any(|warning| warning.contains("future monitor field")));
    }

    #[test]
    fn monitor_view_matches_rows_by_identity_after_catalog_reordering() {
        struct ReorderedCatalog;
        impl WatchlistCatalog for ReorderedCatalog {
            fn load_watchlist(&self, _name: Option<&str>) -> Option<WatchlistDefinition> {
                Some(WatchlistDefinition::new(
                    "reordered",
                    "REORDERED",
                    ["MSFT", "AAPL"]
                        .into_iter()
                        .map(|symbol| {
                            WatchlistItem::new(
                                CanonicalInstrumentId::new(symbol.to_ascii_lowercase()),
                                symbol,
                                symbol,
                            )
                        })
                        .collect(),
                ))
            }
        }
        let state = WorkspaceViewState::new(ID.as_str())
            .with_field("watchlist_id", ViewValue::Text("reordered".to_owned()))
            .with_field("selected_instrument_id", ViewValue::Text("msft".to_owned()))
            .with_field("top_instrument_id", ViewValue::Text("aapl".to_owned()));
        let mut workspace =
            WatchlistWorkspace::new(Arc::new(StubMarketData), Arc::new(ReorderedCatalog));

        let report = workspace.restore_view(&state);

        assert_eq!(report.restored_fields, 3);
        assert_eq!(workspace.rows[workspace.selected].item.symbol, "MSFT");
        assert_eq!(
            workspace.rows[workspace.viewport_top.get()].item.symbol,
            "AAPL"
        );
    }

    #[test]
    fn live_resorts_preserve_and_keep_the_selected_instrument_visible() {
        let mut workspace =
            WatchlistWorkspace::new(Arc::new(StubMarketData), Arc::new(LongCatalog));
        workspace.viewport_rows.set(4);
        workspace.selected = 10;
        workspace.viewport_top.set(8);
        let selected = workspace.rows[10].item.instrument_id.clone();
        for (index, row) in workspace.rows.iter_mut().enumerate() {
            row.quote = Some(
                StubMarketData
                    .quote_snapshots(std::slice::from_ref(&row.item.instrument_id))
                    .unwrap()
                    .remove(0),
            );
            row.quote.as_mut().unwrap().last = Some(Price::new((16 - index) as f64));
        }
        workspace.definition.sort = crate::features::watchlist::SortSpec {
            field: SortField::Last,
            direction: SortDirection::Ascending,
        };

        workspace.sort_rows_preserving_view();

        assert_eq!(
            workspace.rows[workspace.selected].item.instrument_id,
            selected
        );
        assert!(workspace.viewport_top.get() <= workspace.selected);
        assert!(workspace.selected < workspace.viewport_top.get() + 4);
    }

    #[test]
    fn formatting_keeps_terminal_values_dense() {
        assert_eq!(format_volume(Some(Quantity::new(41_820_000))), "41.82M");
        assert_eq!(short_time("2026-08-25T20:00:00Z"), "20:00:00");
    }

    #[test]
    fn responsive_density_keeps_sparklines_without_squeezing_columns() {
        let columns = responsive_columns(WatchlistDefinition::full_columns(), 70);

        assert_eq!(
            columns,
            vec![
                MonitorColumn::Symbol,
                MonitorColumn::Last,
                MonitorColumn::ChangePercent,
                MonitorColumn::Sparkline,
                MonitorColumn::Quality,
            ]
        );
        assert_eq!(columns_width(&columns), 70);
    }

    #[test]
    fn session_trace_records_each_provider_observation_once() {
        let item = WatchlistItem::new(
            CanonicalInstrumentId::new("us:listed:aapl"),
            "AAPL",
            "Apple",
        );
        let mut row = MonitorRow {
            item: item.clone(),
            quote: None,
            session_trace: VecDeque::new(),
            last_trace_observation: None,
        };
        let mut quote = StubMarketData
            .quote_snapshots(std::slice::from_ref(&item.instrument_id))
            .unwrap()
            .remove(0);

        apply_quote(&mut row, quote.clone());
        apply_quote(&mut row, quote.clone());
        quote.last = Some(Price::new(104.0));
        quote.provenance.sequence = Some(2);
        apply_quote(&mut row, quote);

        assert_eq!(row.session_trace, VecDeque::from([100.0, 104.0]));
        assert_eq!(spark_line(&row.session_trace, 16), "▁█");
    }

    #[test]
    fn sparkline_downsamples_to_the_requested_width() {
        let values = VecDeque::from([1.0, 2.0, 3.0, 4.0]);

        assert_eq!(spark_line(&values, 4), "▁▃▆█");
        assert_eq!(spark_line(&values, 2), "▁█");
        assert_eq!(spark_line(&VecDeque::new(), 8), "--");
    }
}
