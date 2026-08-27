use std::{
    cmp::Ordering,
    collections::HashMap,
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
    app::{AppIntent, CommandInvocation, Workspace, WorkspaceDescriptor},
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
}

#[derive(Debug, Clone)]
struct MonitorRow {
    item: WatchlistItem,
    quote: Option<QuoteSnapshot>,
}

struct QuoteRefresh {
    generation: u64,
    instruments: Vec<crate::features::market_data::CanonicalInstrumentId>,
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
            .map(|item| MonitorRow { item, quote: None })
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
                            row.quote = Some(quote.clone());
                        }
                    }
                    self.sort_rows();
                    self.selected = self.selected.min(self.rows.len().saturating_sub(1));
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
                        row.quote = Some(snapshot.clone());
                    }
                }
                self.sort_rows();
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
    }

    fn cycle_sort(&mut self) {
        self.definition.sort.field = self.definition.sort.field.next();
        self.sort_rows();
        self.selected = 0;
    }

    fn toggle_sort_direction(&mut self) {
        self.definition.sort.direction = self.definition.sort.direction.toggled();
        self.sort_rows();
        self.selected = 0;
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

    fn visible_columns(&self) -> Vec<MonitorColumn> {
        match self.column_preset {
            ColumnPreset::Configured => self.definition.visible_columns.clone(),
            ColumnPreset::Trading => WatchlistDefinition::trading_columns(),
            ColumnPreset::Compact => WatchlistDefinition::compact_columns(),
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
                if let Some(row) = self.rows.get(self.selected) {
                    self.pending_intents.push(AppIntent::DispatchCommand {
                        command: format!("SEC {}", row.item.symbol),
                        origin: ID,
                    });
                }
                true
            }
            _ => false,
        }
    }

    fn handle_mouse(&mut self, event: MouseEvent, area: Rect) -> bool {
        let areas = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(2),
        ])
        .split(area);
        if let Some(index) = table_row_at(event, areas[1], self.rows.len()) {
            self.selected = index;
            return true;
        }
        if crate::ui::is_primary_click(event, areas[2]) {
            let controls = [
                (" ↑↓/JK SELECT  ".to_owned(), None),
                ("ENTER OPEN SEC  ".to_owned(), Some(KeyCode::Enter)),
                (
                    format!(
                        "S/SHIFT-S SORT {} {}  ",
                        self.definition.sort.field.label(),
                        self.definition.sort.direction.marker()
                    ),
                    Some(KeyCode::Char('s')),
                ),
                (
                    format!("C COLUMNS {}  ", self.column_preset.label()),
                    Some(KeyCode::Char('c')),
                ),
                ("R REFRESH".to_owned(), Some(KeyCode::Char('r'))),
            ];
            let mut x = areas[2].x;
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
        if let Some(key) = scroll_key(event, areas[1]) {
            return self.handle_key(key);
        }
        false
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
        let areas = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(2),
        ])
        .split(area);
        let quality_counts = quality_counts(&self.rows);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    format!(" {} ", self.definition.name),
                    Style::new().bg(AMBER).fg(BG).bold(),
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
            areas[0],
        );

        let columns = self.visible_columns();
        let widths = columns
            .iter()
            .map(|column| column_width(*column))
            .collect::<Vec<_>>();
        let header = Row::new(columns.iter().map(|column| column.label()))
            .style(Style::new().fg(AMBER).bold())
            .bottom_margin(1);
        let rows = self.rows.iter().enumerate().map(|(index, row)| {
            let selected = index == self.selected;
            Row::new(
                columns
                    .iter()
                    .map(|column| render_cell(row, *column, selected)),
            )
            .style(if selected {
                Style::new().bg(CYAN).fg(BG).bold()
            } else {
                Style::new()
            })
        });
        frame.render_widget(
            Table::new(rows, widths)
                .header(header)
                .column_spacing(1)
                .block(terminal_block("WL", "LIVE SNAPSHOTS")),
            areas[1],
        );

        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" ↑↓/JK ", AMBER),
                Span::styled("SELECT  ", MUTED),
                Span::styled("ENTER ", AMBER),
                Span::styled("OPEN SEC  ", MUTED),
                Span::styled("S/SHIFT-S ", AMBER),
                Span::styled(
                    format!(
                        "SORT {} {}  ",
                        self.definition.sort.field.label(),
                        self.definition.sort.direction.marker()
                    ),
                    MUTED,
                ),
                Span::styled("C ", AMBER),
                Span::styled(format!("COLUMNS {}  ", self.column_preset.label()), MUTED),
                Span::styled("R ", AMBER),
                Span::styled("REPLAY", MUTED),
            ])),
            areas[2],
        );
    }
}

impl Drop for WatchlistWorkspace {
    fn drop(&mut self) {
        if let Some(subscription) = &mut self.subscription {
            subscription.cancel();
        }
    }
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
        MonitorColumn::Quality => quote
            .map(|quote| quote.quality.label())
            .unwrap_or_else(|| "NO SNAPSHOT".to_owned()),
        MonitorColumn::AsOf => quote
            .map(|quote| short_time(quote.as_of.as_str()))
            .unwrap_or_else(|| "--".to_owned()),
    };
    let style = if selected {
        Style::new().bg(CYAN).fg(BG).bold()
    } else {
        cell_style(quote, column, &value)
    };
    Cell::from(value).style(style)
}

fn cell_style(quote: Option<&QuoteSnapshot>, column: MonitorColumn, value: &str) -> Style {
    if matches!(column, MonitorColumn::Quality) {
        return match quote.map(|quote| quote.quality) {
            Some(DataQuality::RealTime) => Style::new().fg(GREEN),
            Some(
                DataQuality::Delayed { .. } | DataQuality::Stale { .. } | DataQuality::Derived,
            ) => Style::new().fg(YELLOW),
            _ => Style::new().fg(RED),
        };
    }
    if matches!(column, MonitorColumn::Change | MonitorColumn::ChangePercent) {
        return if value.starts_with('+') {
            Style::new().fg(GREEN)
        } else if value.starts_with('-') {
            Style::new().fg(RED)
        } else {
            Style::new().fg(INK)
        };
    }
    Style::new().fg(INK)
}

fn column_width(column: MonitorColumn) -> Constraint {
    match column {
        MonitorColumn::Symbol => Constraint::Length(12),
        MonitorColumn::Last | MonitorColumn::Change | MonitorColumn::Bid | MonitorColumn::Ask => {
            Constraint::Length(12)
        }
        MonitorColumn::ChangePercent => Constraint::Length(10),
        MonitorColumn::Volume => Constraint::Length(12),
        MonitorColumn::Quality => Constraint::Length(16),
        MonitorColumn::AsOf => Constraint::Min(12),
    }
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
    fn sort_and_column_controls_are_deterministic() {
        let mut workspace =
            WatchlistWorkspace::new(Arc::new(StubMarketData), Arc::new(StubCatalog));
        workspace.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        workspace.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));

        assert_eq!(workspace.definition.sort.field, SortField::Last);
        assert_eq!(workspace.column_preset, ColumnPreset::Trading);
    }

    #[test]
    fn formatting_keeps_terminal_values_dense() {
        assert_eq!(format_volume(Some(Quantity::new(41_820_000))), "41.82M");
        assert_eq!(short_time("2026-08-25T20:00:00Z"), "20:00:00");
    }
}
