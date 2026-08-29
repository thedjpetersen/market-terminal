//! The half-block candlestick renderer, width-aware OHLC aggregation, right
//! margin, last-bar marker, and Braille study overlay adapt chart behavior from
//! `makeev/alphai-tui` commit `9143d2e1176d0a67a9f26960427cf370187fc2e6`
//! (MIT, Copyright (c) 2026 Mikhail Makeev). They are integrated with this
//! workspace's provider-neutral history model; see `THIRD_PARTY_NOTICES.md`.

use std::ops::Range;
use std::sync::{
    mpsc::{sync_channel, Receiver, SyncSender, TrySendError},
    Arc,
};

use chrono::{DateTime, Local};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Style},
    symbols,
    text::{Line, Span},
    widgets::{Axis, Block, Borders, Chart, Dataset, GraphType, Paragraph, Sparkline, Wrap},
    Frame,
};

use crate::{
    app::{
        AppIntent, CommandInvocation, ViewRestoreReport, ViewValue, Workspace, WorkspaceAction,
        WorkspaceDescriptor, WorkspaceViewState,
    },
    ui::{
        components::terminal_block,
        contains, is_primary_click,
        theme::{ThemeColor, AMBER, BG, CYAN, GREEN, INK, MUTED, RED, YELLOW},
    },
};

use super::{
    controls::{chart_areas, pack_control_areas, ChartControl},
    domain::percent_change,
    indicators::{ema, rsi, sma, MOVING_AVERAGE_FAST, MOVING_AVERAGE_SLOW, RSI_PERIOD},
    ChartHistoryQuery, ChartInstrument, ChartPeriod, ChartSpecification, HistoryError,
    HistoryRequest, HistorySeries, Normalization, Study, ID,
};

const SERIES_COLORS: [ThemeColor; 4] = [CYAN, YELLOW, GREEN, RED];
const CANDLE_RIGHT_MARGIN_PERCENT: u16 = 18;
const MIN_VISIBLE_OBSERVATIONS: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChartDisplayMode {
    Candlesticks,
    Line,
}

impl ChartDisplayMode {
    const fn toggled(self) -> Self {
        match self {
            Self::Candlesticks => Self::Line,
            Self::Line => Self::Candlesticks,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Candlesticks => "CANDLES",
            Self::Line => "LINE",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ChartLineMode {
    Smooth,
    Compatible,
}

impl ChartLineMode {
    const fn marker(self) -> symbols::Marker {
        match self {
            Self::Smooth => symbols::Marker::Braille,
            Self::Compatible => symbols::Marker::HalfBlock,
        }
    }

    const fn toggled(self) -> Self {
        match self {
            Self::Smooth => Self::Compatible,
            Self::Compatible => Self::Smooth,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Smooth => "SMOOTH",
            Self::Compatible => "COMPAT",
        }
    }
}

struct PreparedLine {
    name: String,
    points: Vec<(f64, f64)>,
    color: Color,
}

struct PreparedChart {
    lines: Vec<PreparedLine>,
    rsi_lines: Vec<PreparedLine>,
    primary_values: Vec<f64>,
    primary_closes: Vec<f64>,
    primary_bars: Vec<super::PriceBar>,
    volume_bars: Vec<u64>,
    y_bounds: [f64; 2],
    average_volume: u64,
    last: f64,
    change_percent: f64,
    quality: &'static str,
    source: String,
}

struct ChartWindow {
    range: Range<usize>,
    selected_index: usize,
    x_max: f64,
    y_bounds: [f64; 2],
}

#[derive(Clone, Copy)]
struct PlotAreas {
    price: Rect,
    rsi: Option<Rect>,
    volume: Option<Rect>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct ChartViewport {
    visible_observations: Option<usize>,
    pan_offset: usize,
}

impl ChartViewport {
    fn range(self, total: usize) -> Range<usize> {
        if total == 0 {
            return 0..0;
        }
        let visible = self.visible_observations.unwrap_or(total).clamp(1, total);
        let pan_offset = self.pan_offset.min(total.saturating_sub(visible));
        let end = total - pan_offset;
        end - visible..end
    }

    fn show_around(&mut self, total: usize, visible: usize, selected: usize) {
        if total == 0 || visible >= total {
            *self = Self::default();
            return;
        }
        let visible = visible.clamp(1, total);
        let selected = selected.min(total - 1);
        let start = selected
            .saturating_sub(visible / 2)
            .min(total.saturating_sub(visible));
        self.visible_observations = Some(visible);
        self.pan_offset = total.saturating_sub(start + visible);
    }
}

fn plot_areas(area: Rect, has_rsi: bool, has_volume: bool) -> PlotAreas {
    if has_rsi && has_volume && area.height >= 20 {
        let areas = Layout::vertical([
            Constraint::Percentage(60),
            Constraint::Percentage(18),
            Constraint::Percentage(22),
        ])
        .split(area);
        return PlotAreas {
            price: areas[0],
            rsi: Some(areas[1]),
            volume: Some(areas[2]),
        };
    }
    if has_rsi && area.height >= 14 {
        let areas =
            Layout::vertical([Constraint::Percentage(72), Constraint::Percentage(28)]).split(area);
        return PlotAreas {
            price: areas[0],
            rsi: Some(areas[1]),
            volume: None,
        };
    }
    if has_volume && area.height >= 12 {
        let areas =
            Layout::vertical([Constraint::Percentage(72), Constraint::Percentage(28)]).split(area);
        return PlotAreas {
            price: areas[0],
            rsi: None,
            volume: Some(areas[1]),
        };
    }
    PlotAreas {
        price: area,
        rsi: None,
        volume: None,
    }
}

struct ChartRefresh {
    generation: u64,
    requests: Vec<HistoryRequest>,
}

struct ChartRefreshResult {
    generation: u64,
    result: Result<Vec<HistorySeries>, HistoryError>,
}

pub struct ChartingWorkspace {
    specification: ChartSpecification,
    status: String,
    cursor_offset: usize,
    viewport: ChartViewport,
    display_mode: ChartDisplayMode,
    line_mode: ChartLineMode,
    refresh_sender: SyncSender<ChartRefresh>,
    refresh_receiver: Receiver<ChartRefreshResult>,
    pending_refresh: Option<ChartRefresh>,
    desired_generation: u64,
    history: Option<Vec<HistorySeries>>,
    history_error: Option<HistoryError>,
    pending_intents: Vec<AppIntent>,
}

impl ChartingWorkspace {
    pub fn new(query: Arc<dyn ChartHistoryQuery>) -> Self {
        Self::with_primary(query, ChartInstrument::from_terminal_subject("AAPL"))
    }

    pub fn with_primary(query: Arc<dyn ChartHistoryQuery>, primary: ChartInstrument) -> Self {
        let (refresh_sender, worker_receiver) = sync_channel::<ChartRefresh>(1);
        let (worker_sender, refresh_receiver) = sync_channel::<ChartRefreshResult>(1);
        std::thread::Builder::new()
            .name("chart-history".to_owned())
            .spawn(move || {
                while let Ok(mut refresh) = worker_receiver.recv() {
                    while let Ok(newer) = worker_receiver.try_recv() {
                        refresh = newer;
                    }
                    let result = refresh
                        .requests
                        .iter()
                        .map(|request| query.load_history(request))
                        .collect();
                    if worker_sender
                        .send(ChartRefreshResult {
                            generation: refresh.generation,
                            result,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .expect("chart history worker should start");
        let mut workspace = Self {
            specification: ChartSpecification::new(primary),
            status: "READY · ↑/↓ OR +/- ZOOM · ←/→ PAN · ,/. INSPECT · HOME LATEST · [/] PERIOD"
                .to_owned(),
            cursor_offset: 0,
            viewport: ChartViewport::default(),
            display_mode: ChartDisplayMode::Candlesticks,
            line_mode: ChartLineMode::Smooth,
            refresh_sender,
            refresh_receiver,
            pending_refresh: None,
            desired_generation: 0,
            history: None,
            history_error: None,
            pending_intents: Vec::new(),
        };
        workspace.queue_history();
        workspace
    }

    pub fn specification(&self) -> &ChartSpecification {
        &self.specification
    }

    fn controls(&self) -> Vec<ChartControl> {
        let mut controls = ChartPeriod::ALL
            .into_iter()
            .map(ChartControl::Period)
            .collect::<Vec<_>>();
        controls.extend([
            ChartControl::Normalization,
            ChartControl::MovingAverages,
            ChartControl::AverageKind,
            ChartControl::Rsi,
            ChartControl::Volume,
            ChartControl::Comparison,
        ]);
        if !self.specification.comparisons.is_empty() {
            controls.push(ChartControl::ClearComparisons);
        }
        controls.extend([
            ChartControl::ZoomIn,
            ChartControl::ZoomOut,
            ChartControl::PanBack,
            ChartControl::PanForward,
            ChartControl::InspectBack,
            ChartControl::InspectForward,
            ChartControl::Latest,
            ChartControl::DisplayMode,
            ChartControl::LineMode,
            ChartControl::InsertSheet,
            ChartControl::Refresh,
        ]);
        controls
    }

    fn control_label(&self, control: ChartControl) -> String {
        match control {
            ChartControl::Period(period) => format!(" {} ", period.label()),
            ChartControl::Normalization => {
                format!(" N {} ", self.specification.normalization.label())
            }
            ChartControl::MovingAverages => " M MA ".to_owned(),
            ChartControl::AverageKind => " E SMA/EMA ".to_owned(),
            ChartControl::Rsi => " I RSI ".to_owned(),
            ChartControl::Volume => " V VOL ".to_owned(),
            ChartControl::Comparison => " C SPY ".to_owned(),
            ChartControl::ClearComparisons => " X CLR ".to_owned(),
            ChartControl::ZoomIn => " + ZOOM ".to_owned(),
            ChartControl::ZoomOut => " - OUT ".to_owned(),
            ChartControl::PanBack => " ← PAN ".to_owned(),
            ChartControl::PanForward => " PAN → ".to_owned(),
            ChartControl::InspectBack => " , BACK ".to_owned(),
            ChartControl::InspectForward => " . FWD ".to_owned(),
            ChartControl::Latest => " HOME NOW ".to_owned(),
            ChartControl::DisplayMode => format!(" K {} ", self.display_mode.label()),
            ChartControl::LineMode => format!(" L {} ", self.line_mode.label()),
            ChartControl::InsertSheet => " A SHEET ".to_owned(),
            ChartControl::Refresh => " F9 REFRESH ".to_owned(),
        }
    }

    fn control_areas(&self, area: Rect) -> Vec<(ChartControl, Rect)> {
        pack_control_areas(
            area,
            self.controls().into_iter().map(|control| {
                let width = self.control_label(control).chars().count() as u16;
                (control, width)
            }),
        )
    }

    fn has_moving_averages(&self) -> bool {
        self.specification.studies.iter().any(|study| {
            matches!(
                study,
                Study::SimpleMovingAverage { .. } | Study::ExponentialMovingAverage { .. }
            )
        })
    }

    fn inspection_max_offset(&self) -> usize {
        self.chart_length().saturating_sub(1)
    }

    fn chart_length(&self) -> usize {
        self.history
            .as_ref()
            .and_then(|series| series.first())
            .map_or(0, |series| series.bars.len())
    }

    fn selected_index(&self, total: usize) -> usize {
        total
            .saturating_sub(1)
            .saturating_sub(self.cursor_offset.min(total.saturating_sub(1)))
    }

    fn visible_range(&self, total: usize) -> Range<usize> {
        self.viewport.range(total)
    }

    fn reset_view(&mut self) {
        self.cursor_offset = 0;
        self.viewport = ChartViewport::default();
    }

    fn constrain_view_to_history(&mut self) {
        let total = self.chart_length();
        if total == 0 {
            return;
        }
        self.cursor_offset = self.cursor_offset.min(total - 1);
        let visible = self
            .viewport
            .visible_observations
            .unwrap_or(total)
            .clamp(1, total);
        if visible >= total {
            self.viewport = ChartViewport::default();
        } else {
            self.viewport.visible_observations = Some(visible);
            self.viewport.pan_offset = self.viewport.pan_offset.min(total - visible);
            self.reveal_inspection(total);
        }
    }

    fn zoom_view(&mut self, inward: bool) {
        let total = self.chart_length();
        if total == 0 {
            self.status = "ZOOM UNAVAILABLE · HISTORY LOADING".to_owned();
            return;
        }
        let current = self.visible_range(total).len();
        let minimum = MIN_VISIBLE_OBSERVATIONS.min(total).max(1);
        let visible = if inward {
            if current <= minimum {
                self.status = format!("ZOOM LIMIT · {current} OBSERVATIONS");
                return;
            }
            (current * 3 / 4).max(minimum).min(current - 1)
        } else {
            if current >= total {
                self.status = format!("FULL HISTORY · {total} OBSERVATIONS");
                return;
            }
            (current * 4 / 3).max(current + 1).min(total)
        };
        self.viewport
            .show_around(total, visible, self.selected_index(total));
        let range = self.visible_range(total);
        self.status = format!(
            "ZOOM · {}-{} OF {total} · {} OBSERVATIONS",
            range.start + 1,
            range.end,
            range.len()
        );
    }

    fn pan_view(&mut self, backward: bool) {
        let total = self.chart_length();
        let range = self.visible_range(total);
        if total == 0 {
            self.status = "PAN UNAVAILABLE · HISTORY LOADING".to_owned();
            return;
        }
        if range.len() >= total {
            self.status = "PAN UNAVAILABLE · ZOOM IN FIRST".to_owned();
            return;
        }
        let maximum = total - range.len();
        let previous = self.viewport.pan_offset.min(maximum);
        let step = (range.len() / 5).max(1);
        let next = if backward {
            previous.saturating_add(step).min(maximum)
        } else {
            previous.saturating_sub(step)
        };
        let moved = previous.abs_diff(next);
        self.viewport.pan_offset = next;
        self.cursor_offset = if backward {
            self.cursor_offset
                .saturating_add(moved)
                .min(total.saturating_sub(1))
        } else {
            self.cursor_offset.saturating_sub(moved)
        };
        let range = self.visible_range(total);
        self.status = format!(
            "PAN · {}-{} OF {total} · INSPECT {}",
            range.start + 1,
            range.end,
            self.selected_index(total) + 1
        );
    }

    fn move_inspection(&mut self, backward: bool) {
        let total = self.chart_length();
        if total == 0 {
            self.cursor_offset = if backward {
                self.cursor_offset.saturating_add(1).min(10_000)
            } else {
                self.cursor_offset.saturating_sub(1)
            };
            self.status = format!("INSPECT · {} OBSERVATION(S) BACK", self.cursor_offset);
            return;
        }
        self.cursor_offset = if backward {
            self.cursor_offset
                .saturating_add(1)
                .min(total.saturating_sub(1))
        } else {
            self.cursor_offset.saturating_sub(1)
        };
        self.reveal_inspection(total);
        self.status = format!(
            "INSPECT · OBSERVATION {} OF {total}",
            self.selected_index(total) + 1
        );
    }

    fn reveal_inspection(&mut self, total: usize) {
        let range = self.visible_range(total);
        if range.is_empty() || range.len() >= total {
            return;
        }
        let selected = self.selected_index(total);
        if selected < range.start {
            self.viewport.pan_offset = total.saturating_sub(selected + range.len());
        } else if selected >= range.end {
            self.viewport.pan_offset = total.saturating_sub(selected + 1);
        }
    }

    fn control_enabled(&self, control: ChartControl) -> bool {
        let total = self.chart_length();
        let visible = self.visible_range(total).len();
        match control {
            ChartControl::ClearComparisons => !self.specification.comparisons.is_empty(),
            ChartControl::ZoomIn => {
                total > MIN_VISIBLE_OBSERVATIONS.min(total) && visible > MIN_VISIBLE_OBSERVATIONS
            }
            ChartControl::ZoomOut => total > 0 && visible < total,
            ChartControl::PanBack => {
                total > 0
                    && visible < total
                    && self.viewport.pan_offset < total.saturating_sub(visible)
            }
            ChartControl::PanForward => {
                total > 0
                    && visible < total
                    && self.viewport.pan_offset.min(total.saturating_sub(visible)) > 0
            }
            ChartControl::InspectBack => self.cursor_offset < self.inspection_max_offset(),
            ChartControl::InspectForward | ChartControl::Latest => self.cursor_offset > 0,
            _ => true,
        }
    }

    fn control_active(&self, control: ChartControl) -> bool {
        match control {
            ChartControl::Period(period) => period == self.specification.period,
            ChartControl::Normalization => {
                self.specification.normalization == Normalization::PercentChange
            }
            ChartControl::MovingAverages => self.has_moving_averages(),
            ChartControl::AverageKind => self
                .specification
                .studies
                .iter()
                .any(|study| matches!(study, Study::ExponentialMovingAverage { .. })),
            ChartControl::Rsi => self
                .specification
                .has_study(Study::RelativeStrengthIndex { period: RSI_PERIOD }),
            ChartControl::Volume => self.specification.has_study(Study::Volume),
            ChartControl::Comparison | ChartControl::ClearComparisons => {
                !self.specification.comparisons.is_empty()
            }
            ChartControl::Latest => self.cursor_offset == 0,
            ChartControl::DisplayMode => self.display_mode == ChartDisplayMode::Line,
            ChartControl::LineMode => self.line_mode == ChartLineMode::Compatible,
            ChartControl::ZoomIn
            | ChartControl::ZoomOut
            | ChartControl::PanBack
            | ChartControl::PanForward
            | ChartControl::InspectBack
            | ChartControl::InspectForward
            | ChartControl::InsertSheet
            | ChartControl::Refresh => false,
        }
    }

    fn control_action_label(&self, control: ChartControl) -> String {
        match control {
            ChartControl::Period(period) => format!("Show {} chart history", period.label()),
            ChartControl::Normalization => format!(
                "Toggle normalization from {}",
                self.specification.normalization.label()
            ),
            ChartControl::MovingAverages => {
                if self.has_moving_averages() {
                    "Hide moving averages".to_owned()
                } else {
                    "Show default moving averages".to_owned()
                }
            }
            ChartControl::AverageKind => "Switch SMA and EMA studies".to_owned(),
            ChartControl::Rsi => "Toggle Wilder RSI 14".to_owned(),
            ChartControl::Volume => "Toggle volume histogram".to_owned(),
            ChartControl::Comparison => "Toggle SPY comparison".to_owned(),
            ChartControl::ClearComparisons => "Clear all comparisons".to_owned(),
            ChartControl::ZoomIn => "Zoom into fewer visible observations".to_owned(),
            ChartControl::ZoomOut => "Zoom out toward full history".to_owned(),
            ChartControl::PanBack => "Pan the visible window backward".to_owned(),
            ChartControl::PanForward => "Pan the visible window forward".to_owned(),
            ChartControl::InspectBack => "Inspect previous observation".to_owned(),
            ChartControl::InspectForward => "Inspect next observation".to_owned(),
            ChartControl::Latest => "Inspect latest observation".to_owned(),
            ChartControl::DisplayMode => {
                format!("Toggle chart style from {}", self.display_mode.label())
            }
            ChartControl::LineMode => {
                format!("Toggle line rendering from {}", self.line_mode.label())
            }
            ChartControl::InsertSheet => {
                format!(
                    "Insert {} into Spreadsheet",
                    self.specification.primary.symbol
                )
            }
            ChartControl::Refresh => {
                format!(
                    "Refresh {} chart history",
                    self.specification.primary.symbol
                )
            }
        }
    }

    fn activate_control(&mut self, control: ChartControl) -> bool {
        if !self.control_enabled(control) {
            return false;
        }
        match control {
            ChartControl::Period(period) => {
                if period != self.specification.period {
                    self.specification.period = period;
                    self.reset_view();
                    self.queue_history();
                }
            }
            ChartControl::Normalization => {
                self.specification.normalization = self.specification.normalization.toggled();
                self.status = format!("MODE · {}", self.specification.normalization.label());
            }
            ChartControl::MovingAverages => self.toggle_default_moving_averages(),
            ChartControl::AverageKind => self.toggle_moving_average_kind(),
            ChartControl::Rsi => {
                let _ = self
                    .specification
                    .toggle_study(Study::RelativeStrengthIndex { period: RSI_PERIOD });
                self.status = "RSI 14 TOGGLED".to_owned();
            }
            ChartControl::Volume => {
                let _ = self.specification.toggle_study(Study::Volume);
                self.status = "VOLUME TOGGLED".to_owned();
            }
            ChartControl::Comparison => {
                self.toggle_default_comparison();
                self.queue_history();
            }
            ChartControl::ClearComparisons => {
                self.specification.comparisons.clear();
                self.status = "COMPARISONS CLEARED".to_owned();
                self.queue_history();
            }
            ChartControl::ZoomIn => self.zoom_view(true),
            ChartControl::ZoomOut => self.zoom_view(false),
            ChartControl::PanBack => self.pan_view(true),
            ChartControl::PanForward => self.pan_view(false),
            ChartControl::InspectBack => self.move_inspection(true),
            ChartControl::InspectForward => self.move_inspection(false),
            ChartControl::Latest => {
                self.cursor_offset = 0;
                self.viewport.pan_offset = 0;
                self.status = "INSPECT · LATEST OBSERVATION".to_owned();
            }
            ChartControl::DisplayMode => self.toggle_display_mode(),
            ChartControl::LineMode => {
                self.line_mode = self.line_mode.toggled();
                self.status = format!("LINE MODE · {}", self.line_mode.label());
            }
            ChartControl::InsertSheet => {
                self.pending_intents.push(AppIntent::DispatchCommand {
                    command: format!("SHEET INSERT {}", self.specification.primary.symbol),
                    origin: ID,
                });
            }
            ChartControl::Refresh => self.queue_history(),
        }
        true
    }

    fn toggle_default_moving_averages(&mut self) {
        let has_average = self.specification.studies.iter().any(|study| {
            matches!(
                study,
                Study::SimpleMovingAverage { .. } | Study::ExponentialMovingAverage { .. }
            )
        });
        if has_average {
            self.specification.studies.retain(|study| {
                !matches!(
                    study,
                    Study::SimpleMovingAverage { .. } | Study::ExponentialMovingAverage { .. }
                )
            });
            self.status = "MOVING AVERAGES HIDDEN".to_owned();
        } else {
            self.specification.studies.push(Study::SimpleMovingAverage {
                window: MOVING_AVERAGE_FAST,
            });
            self.specification.studies.push(Study::SimpleMovingAverage {
                window: MOVING_AVERAGE_SLOW,
            });
            self.status = "SMA 20/100 SHOWN".to_owned();
        }
    }

    fn toggle_moving_average_kind(&mut self) {
        let mut changed = false;
        for study in &mut self.specification.studies {
            *study = match *study {
                Study::SimpleMovingAverage { window } => {
                    changed = true;
                    Study::ExponentialMovingAverage { window }
                }
                Study::ExponentialMovingAverage { window } => {
                    changed = true;
                    Study::SimpleMovingAverage { window }
                }
                unchanged => unchanged,
            };
        }
        if changed {
            self.status = "MOVING AVERAGE KIND SWITCHED".to_owned();
        } else {
            self.specification
                .studies
                .push(Study::ExponentialMovingAverage {
                    window: MOVING_AVERAGE_FAST,
                });
            self.specification
                .studies
                .push(Study::ExponentialMovingAverage {
                    window: MOVING_AVERAGE_SLOW,
                });
            self.status = "EMA 20/100 SHOWN".to_owned();
        }
    }

    fn toggle_default_comparison(&mut self) {
        let comparison = ChartInstrument::from_terminal_subject("SPY");
        if let Some(index) = self
            .specification
            .comparisons
            .iter()
            .position(|current| current.canonical_id == comparison.canonical_id)
        {
            self.specification.comparisons.remove(index);
            self.status = "SPY COMPARISON REMOVED".to_owned();
        } else {
            match self.specification.add_comparison(comparison) {
                Ok(()) => {
                    self.specification.normalization = Normalization::PercentChange;
                    self.status = "SPY COMPARISON ADDED · NORMALIZED".to_owned();
                }
                Err(error) => self.status = format!("CHART ERROR · {error}"),
            }
        }
    }

    fn history_requests(&self) -> Vec<HistoryRequest> {
        let mut instruments = Vec::with_capacity(1 + self.specification.comparisons.len());
        instruments.push(self.specification.primary.clone());
        instruments.extend(self.specification.comparisons.iter().cloned());

        instruments
            .into_iter()
            .map(|instrument| HistoryRequest::new(instrument, self.specification.period))
            .collect()
    }

    fn queue_history(&mut self) {
        self.desired_generation = self.desired_generation.wrapping_add(1);
        self.pending_refresh = Some(ChartRefresh {
            generation: self.desired_generation,
            requests: self.history_requests(),
        });
        self.history = None;
        self.history_error = None;
        self.status = format!(
            "LOADING LIVE HISTORY · {} · {}",
            self.specification.primary.symbol,
            self.specification.period.label()
        );
        self.dispatch_pending_refresh();
    }

    fn dispatch_pending_refresh(&mut self) {
        let Some(refresh) = self.pending_refresh.take() else {
            return;
        };
        match self.refresh_sender.try_send(refresh) {
            Ok(()) => {}
            Err(TrySendError::Full(refresh)) => self.pending_refresh = Some(refresh),
            Err(TrySendError::Disconnected(_)) => {
                self.history_error = Some(HistoryError::Unavailable(
                    "chart history worker stopped".to_owned(),
                ));
                self.status = "CHART HISTORY WORKER STOPPED".to_owned();
            }
        }
    }

    fn poll_history(&mut self) {
        while let Ok(refresh) = self.refresh_receiver.try_recv() {
            if refresh.generation != self.desired_generation {
                continue;
            }
            match refresh.result {
                Ok(series) => {
                    self.status = format!("{} LIVE SERIES LOADED", series.len());
                    self.history = Some(series);
                    self.history_error = None;
                    self.constrain_view_to_history();
                }
                Err(error) => {
                    self.status = error.to_string();
                    self.history = None;
                    self.history_error = Some(error);
                }
            }
        }
        self.dispatch_pending_refresh();
    }

    fn prepared_chart(&self) -> Result<PreparedChart, HistoryError> {
        let series = self.history.as_deref().ok_or_else(|| {
            self.history_error.clone().unwrap_or_else(|| {
                HistoryError::Unavailable("live chart history is loading".to_owned())
            })
        })?;
        prepare_chart(&self.specification, series)
    }

    fn effective_display_mode(&self) -> ChartDisplayMode {
        if self.display_mode == ChartDisplayMode::Candlesticks
            && self.specification.normalization == Normalization::Price
            && self.specification.comparisons.is_empty()
        {
            ChartDisplayMode::Candlesticks
        } else {
            ChartDisplayMode::Line
        }
    }

    fn chart_window(&self, chart: &PreparedChart) -> ChartWindow {
        let total = chart.primary_values.len();
        let range = self.visible_range(total);
        let selected_index = if range.is_empty() {
            0
        } else {
            self.selected_index(total)
                .clamp(range.start, range.end.saturating_sub(1))
        };
        let (minimum, maximum) = chart
            .lines
            .iter()
            .flat_map(|line| {
                line.points
                    .iter()
                    .filter(|point| range.contains(&(point.0 as usize)))
                    .map(|point| point.1)
            })
            .filter(|value| value.is_finite())
            .fold(
                (f64::INFINITY, f64::NEG_INFINITY),
                |(minimum, maximum), value| (minimum.min(value), maximum.max(value)),
            );
        let y_bounds = if minimum.is_finite() && maximum.is_finite() {
            padded_bounds(minimum, maximum)
        } else {
            chart.y_bounds
        };
        ChartWindow {
            x_max: range.len().saturating_sub(1).max(1) as f64,
            range,
            selected_index,
            y_bounds,
        }
    }

    fn toggle_display_mode(&mut self) {
        self.display_mode = self.display_mode.toggled();
        self.status = format!("CHART STYLE · {}", self.display_mode.label());
    }

    fn render_header(&self, frame: &mut Frame, area: Rect, chart: &PreparedChart) {
        let change_style = if chart.change_percent >= 0.0 {
            GREEN
        } else {
            RED
        };
        let comparisons = if self.specification.comparisons.is_empty() {
            "NONE".to_owned()
        } else {
            self.specification
                .comparisons
                .iter()
                .map(|instrument| instrument.symbol.as_str())
                .collect::<Vec<_>>()
                .join(" · ")
        };
        let studies = if self.specification.studies.is_empty() {
            "NONE".to_owned()
        } else {
            self.specification
                .studies
                .iter()
                .map(|study| study.label())
                .collect::<Vec<_>>()
                .join(" · ")
        };

        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    format!(" {}  ", self.specification.primary.symbol),
                    Style::new().bg(AMBER.into()).fg(BG.into()).bold(),
                ),
                Span::styled(
                    format!(" {:.2}  ", chart.last),
                    Style::new().fg(CYAN.into()).bold(),
                ),
                Span::styled(format!("{:+.2}%  ", chart.change_percent), change_style),
                Span::styled(
                    format!(
                        "{} · {} · {}  |  COMPARE {}  |  {}  |  {} · {}",
                        self.specification.period.label(),
                        self.specification.normalization.label(),
                        match self.effective_display_mode() {
                            ChartDisplayMode::Candlesticks => "CANDLES + OHLC",
                            ChartDisplayMode::Line => self.line_mode.label(),
                        },
                        comparisons,
                        studies,
                        chart.quality,
                        chart.source,
                    ),
                    MUTED,
                ),
            ]))
            .block(Block::new().borders(Borders::ALL).border_style(AMBER))
            .alignment(Alignment::Left),
            area,
        );
    }

    fn render_price_chart(
        &self,
        frame: &mut Frame,
        area: Rect,
        chart: &PreparedChart,
        window: &ChartWindow,
    ) {
        let columns = if area.width >= 110 {
            Layout::horizontal([Constraint::Percentage(78), Constraint::Percentage(22)]).split(area)
        } else {
            Layout::horizontal([Constraint::Percentage(100), Constraint::Length(0)]).split(area)
        };
        if self.effective_display_mode() == ChartDisplayMode::Candlesticks {
            render_candlesticks(frame, columns[0], chart, window);
        } else {
            let selected_x = window.selected_index.saturating_sub(window.range.start) as f64;
            let cursor = [
                (selected_x, window.y_bounds[0]),
                (selected_x, window.y_bounds[1]),
            ];
            let zero_baseline = [(0.0, 0.0), (window.x_max, 0.0)];
            let visible_lines = chart
                .lines
                .iter()
                .map(|line| visible_points(&line.points, &window.range))
                .collect::<Vec<_>>();
            let mut datasets = chart
                .lines
                .iter()
                .zip(&visible_lines)
                .map(|(line, points)| {
                    Dataset::default()
                        .name(line.name.clone())
                        .marker(self.line_mode.marker())
                        .graph_type(GraphType::Line)
                        .style(line.color)
                        .data(points)
                })
                .collect::<Vec<_>>();
            if self.specification.normalization == Normalization::PercentChange {
                datasets.push(
                    Dataset::default()
                        .name("0% BASE")
                        .marker(symbols::Marker::Dot)
                        .graph_type(GraphType::Line)
                        .style(MUTED)
                        .data(&zero_baseline),
                );
            }
            datasets.push(
                Dataset::default()
                    .name("INSPECT")
                    .marker(symbols::Marker::Dot)
                    .graph_type(GraphType::Line)
                    .style(AMBER)
                    .data(&cursor),
            );
            let middle = (window.y_bounds[0] + window.y_bounds[1]) / 2.0;
            let lower_middle = (window.y_bounds[0] + middle) / 2.0;
            let upper_middle = (middle + window.y_bounds[1]) / 2.0;
            let y_labels = [
                format!("{:.2}", window.y_bounds[0]),
                format!("{lower_middle:.2}"),
                format!("{middle:.2}"),
                format!("{upper_middle:.2}"),
                format!("{:.2}", window.y_bounds[1]),
            ];
            let axis_title = match self.specification.normalization {
                Normalization::Price => "PRICE",
                Normalization::PercentChange => "% CHANGE",
            };
            let title = line_inspection_title(chart, window, self.specification.normalization);
            let price_chart = Chart::new(datasets)
                .block(terminal_block("GRAPH", &title))
                .x_axis(
                    Axis::default()
                        .bounds([0.0, window.x_max])
                        .labels(window_time_labels(chart, &window.range))
                        .style(MUTED),
                )
                .y_axis(
                    Axis::default()
                        .title(axis_title)
                        .bounds(window.y_bounds)
                        .labels(y_labels)
                        .style(AMBER),
                );
            frame.render_widget(price_chart, columns[0]);
        }

        if columns[1].width > 0 {
            self.render_statistics(frame, columns[1], chart, window);
        }
    }

    fn render_volume_chart(
        &self,
        frame: &mut Frame,
        area: Rect,
        chart: &PreparedChart,
        window: &ChartWindow,
    ) {
        let volume_bars = &chart.volume_bars[window.range.clone()];
        let volume = Sparkline::default()
            .data(volume_bars)
            .max(volume_bars.iter().copied().max().unwrap_or(1).max(1))
            .style(AMBER)
            .block(terminal_block("VOL", "VOLUME HISTOGRAM"));
        frame.render_widget(volume, area);
    }

    fn render_rsi_chart(
        &self,
        frame: &mut Frame,
        area: Rect,
        chart: &PreparedChart,
        window: &ChartWindow,
    ) {
        let lower_threshold = [(0.0, 30.0), (window.x_max, 30.0)];
        let upper_threshold = [(0.0, 70.0), (window.x_max, 70.0)];
        let selected_x = window.selected_index.saturating_sub(window.range.start) as f64;
        let cursor = [(selected_x, 0.0), (selected_x, 100.0)];
        let visible_lines = chart
            .rsi_lines
            .iter()
            .map(|line| visible_points(&line.points, &window.range))
            .collect::<Vec<_>>();
        let mut datasets = chart
            .rsi_lines
            .iter()
            .zip(&visible_lines)
            .map(|(line, points)| {
                Dataset::default()
                    .name(line.name.clone())
                    .marker(self.line_mode.marker())
                    .graph_type(GraphType::Line)
                    .style(line.color)
                    .data(points)
            })
            .collect::<Vec<_>>();
        datasets.extend([
            Dataset::default()
                .name("30")
                .marker(symbols::Marker::Dot)
                .graph_type(GraphType::Line)
                .style(MUTED)
                .data(&lower_threshold),
            Dataset::default()
                .name("70")
                .marker(symbols::Marker::Dot)
                .graph_type(GraphType::Line)
                .style(MUTED)
                .data(&upper_threshold),
            Dataset::default()
                .name("INSPECT")
                .marker(symbols::Marker::Dot)
                .graph_type(GraphType::Line)
                .style(AMBER)
                .data(&cursor),
        ]);

        frame.render_widget(
            Chart::new(datasets)
                .block(terminal_block("RSI", "WILDER RELATIVE STRENGTH"))
                .x_axis(Axis::default().bounds([0.0, window.x_max]).style(MUTED))
                .y_axis(
                    Axis::default()
                        .bounds([0.0, 100.0])
                        .labels(["0", "30", "50", "70", "100"])
                        .style(AMBER),
                ),
            area,
        );
    }

    fn render_statistics(
        &self,
        frame: &mut Frame,
        area: Rect,
        chart: &PreparedChart,
        window: &ChartWindow,
    ) {
        let selected_index = window.selected_index;
        let Some(selected_bar) = chart.primary_bars.get(selected_index) else {
            return;
        };
        let selected_value = chart
            .primary_values
            .get(selected_index)
            .copied()
            .unwrap_or_default();
        let observation = selected_index + 1;
        let total = chart.primary_values.len();
        let visible_bars = &chart.primary_bars[window.range.clone()];
        let view_high = visible_bars
            .iter()
            .map(|bar| bar.high)
            .fold(f64::NEG_INFINITY, f64::max);
        let view_low = visible_bars
            .iter()
            .map(|bar| bar.low)
            .fold(f64::INFINITY, f64::min);
        let mut lines = vec![
            Line::from(Span::styled("INSPECTION", AMBER)),
            Line::from(Span::styled(format!("OBS  {observation}/{total}"), MUTED)),
            Line::from(Span::styled(
                inspection_time_label(selected_bar.timestamp),
                MUTED,
            )),
            Line::from(Span::styled(format!("O    {:.2}", selected_bar.open), INK)),
            Line::from(Span::styled(
                format!("H    {:.2}", selected_bar.high),
                GREEN,
            )),
            Line::from(Span::styled(format!("L    {:.2}", selected_bar.low), RED)),
            Line::from(Span::styled(
                format!("C    {:.2}", selected_bar.close),
                CYAN,
            )),
            Line::from(Span::styled(format!("PLOT {selected_value:+.2}"), INK)),
            Line::from(Span::styled(
                format!("VOL  {}", compact_volume(selected_bar.volume)),
                AMBER,
            )),
            Line::from(""),
            Line::from(Span::styled(
                format!("VIEW {}-{}", window.range.start + 1, window.range.end),
                AMBER,
            )),
            Line::from(Span::styled(format!("HIGH {:.2}", view_high), GREEN)),
            Line::from(Span::styled(format!("LOW  {:.2}", view_low), RED)),
            Line::from(Span::styled(
                format!("SPAN {:.2}", view_high - view_low),
                MUTED,
            )),
            Line::from(Span::styled(
                format!("AVG  {}", compact_volume(chart.average_volume)),
                MUTED,
            )),
            Line::from(""),
            Line::from(Span::styled("SERIES", AMBER)),
        ];
        lines.extend(chart.lines.iter().map(|line| {
            let value = line
                .points
                .iter()
                .find(|point| point.0 as usize == selected_index)
                .map(|point| point.1);
            Line::from(vec![
                Span::styled("■ ", line.color),
                Span::styled(format!("{:<7}", line.name), MUTED),
                Span::styled(
                    value
                        .map(|value| format!("{value:+.2}"))
                        .unwrap_or_else(|| "—".to_owned()),
                    line.color,
                ),
            ])
        }));
        lines.extend(chart.rsi_lines.iter().map(|line| {
            let value = line
                .points
                .iter()
                .find(|point| point.0 as usize == selected_index)
                .map(|point| point.1);
            Line::from(vec![
                Span::styled("■ ", line.color),
                Span::styled(format!("{:<7}", line.name), MUTED),
                Span::styled(
                    value
                        .map(|value| format!("{value:.2}"))
                        .unwrap_or_else(|| "—".to_owned()),
                    line.color,
                ),
            ])
        }));
        frame.render_widget(
            Paragraph::new(lines).block(terminal_block("STAT", "MARKET PROFILE")),
            area,
        );
    }

    fn render_error(&self, frame: &mut Frame, area: Rect, error: &HistoryError) {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled("HISTORY QUERY FAILED", RED)),
                Line::from(""),
                Line::from(Span::styled(error.to_string(), INK)),
                Line::from(Span::styled(
                    "The last chart specification is preserved. Retry or choose another period.",
                    MUTED,
                )),
            ])
            .block(terminal_block("ERR", "CHART DATA"))
            .wrap(Wrap { trim: true }),
            area,
        );
    }
}

impl Workspace for ChartingWorkspace {
    fn descriptor(&self) -> WorkspaceDescriptor {
        WorkspaceDescriptor {
            id: ID,
            label: "CHARTING",
            hotkey: 'c',
            commands: &["CHART", "GRAPH"],
        }
    }

    fn is_favorite(&self) -> bool {
        true
    }

    fn handle_command(&mut self, invocation: &CommandInvocation) -> bool {
        let previous_primary = self.specification.primary.clone();
        let previous_period = self.specification.period;
        let previous_comparisons = self.specification.comparisons.clone();
        apply_chart_command(
            &mut self.specification,
            &mut self.display_mode,
            &invocation.args,
            &mut self.status,
        );
        self.reset_view();
        if self.specification.primary != previous_primary
            || self.specification.period != previous_period
            || self.specification.comparisons != previous_comparisons
        {
            self.queue_history();
        }
        true
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char(']') | KeyCode::Char('t') => {
                self.specification.period = self.specification.period.next();
                self.reset_view();
                self.queue_history();
                true
            }
            KeyCode::Char('[') | KeyCode::Char('T') => {
                self.specification.period = self.specification.period.previous();
                self.reset_view();
                self.queue_history();
                true
            }
            KeyCode::Up | KeyCode::Char('+' | '=') => {
                self.zoom_view(true);
                true
            }
            KeyCode::Down | KeyCode::Char('-' | '_') => {
                self.zoom_view(false);
                true
            }
            KeyCode::Left => {
                self.pan_view(true);
                true
            }
            KeyCode::Right => {
                self.pan_view(false);
                true
            }
            KeyCode::Char('n') => self.activate_control(ChartControl::Normalization),
            KeyCode::Char('v') | KeyCode::Char('b') => self.activate_control(ChartControl::Volume),
            KeyCode::Char('s') | KeyCode::Char('m') => {
                self.activate_control(ChartControl::MovingAverages)
            }
            KeyCode::Char('e') => self.activate_control(ChartControl::AverageKind),
            KeyCode::Char('i') => self.activate_control(ChartControl::Rsi),
            KeyCode::Char('c') => self.activate_control(ChartControl::Comparison),
            KeyCode::Char('x') => {
                if self.specification.comparisons.is_empty() {
                    true
                } else {
                    self.activate_control(ChartControl::ClearComparisons)
                }
            }
            KeyCode::Char(',') => {
                self.move_inspection(true);
                true
            }
            KeyCode::Char('.') => {
                self.move_inspection(false);
                true
            }
            KeyCode::Home | KeyCode::Char('E') => {
                self.cursor_offset = 0;
                self.viewport.pan_offset = 0;
                self.status = "INSPECT · LATEST OBSERVATION".to_owned();
                true
            }
            KeyCode::Char('l') => self.activate_control(ChartControl::LineMode),
            KeyCode::Char('a') => self.activate_control(ChartControl::InsertSheet),
            KeyCode::Char('k') => self.activate_control(ChartControl::DisplayMode),
            KeyCode::F(9) => self.activate_control(ChartControl::Refresh),
            _ => false,
        }
    }

    fn handle_mouse(&mut self, event: MouseEvent, area: Rect) -> bool {
        let areas = chart_areas(area);
        if is_primary_click(event, areas.header) {
            return self.activate_control(ChartControl::Refresh);
        }
        if contains(areas.plot, event.column, event.row) {
            match event.kind {
                MouseEventKind::ScrollUp => {
                    return self.handle_key(KeyEvent::new(KeyCode::Char(','), KeyModifiers::NONE));
                }
                MouseEventKind::ScrollDown => {
                    return self.handle_key(KeyEvent::new(KeyCode::Char('.'), KeyModifiers::NONE));
                }
                _ => {}
            }
        }
        if is_primary_click(event, areas.footer) {
            for (control, control_area) in self.control_areas(areas.footer) {
                if contains(control_area, event.column, event.row) {
                    return self.activate_control(control);
                }
            }
            return true;
        }
        if !is_primary_click(event, areas.plot) {
            return false;
        }
        let Ok(chart) = self.prepared_chart() else {
            return true;
        };
        if chart.primary_values.is_empty() {
            return true;
        }
        let areas = plot_areas(
            areas.plot,
            !chart.rsi_lines.is_empty(),
            self.specification.has_study(Study::Volume),
        );
        let price_area = areas.price;
        let chart_area = if price_area.width >= 110 {
            Layout::horizontal([Constraint::Percentage(78), Constraint::Percentage(22)])
                .split(price_area)[0]
        } else {
            price_area
        };
        if !contains(chart_area, event.column, event.row) {
            return true;
        }
        let plot_x = chart_area.x.saturating_add(1);
        let plot_width = chart_area.width.saturating_sub(2).max(1);
        let relative = event
            .column
            .saturating_sub(plot_x)
            .min(plot_width.saturating_sub(1));
        let total = chart.primary_values.len();
        let range = self.visible_range(total);
        let selected = range.start
            + usize::from(relative) * range.len().saturating_sub(1)
                / usize::from(plot_width.saturating_sub(1).max(1));
        self.cursor_offset = total.saturating_sub(1).saturating_sub(selected);
        self.status = format!("INSPECT · OBSERVATION {} OF {total}", selected + 1);
        true
    }

    fn actions(&self, area: Rect) -> Vec<WorkspaceAction> {
        let areas = chart_areas(area);
        let mut actions = self
            .control_areas(areas.footer)
            .into_iter()
            .map(|(control, control_area)| {
                let mut action = WorkspaceAction::new(
                    control.action_id(),
                    self.control_action_label(control),
                    control_area,
                );
                if !self.control_enabled(control) {
                    action = action.disabled();
                }
                if control == ChartControl::Period(self.specification.period) {
                    action = action.preferred();
                }
                action
            })
            .collect::<Vec<_>>();
        actions.push(WorkspaceAction::new(
            "control:refresh-header",
            format!(
                "Refresh {} chart history from header",
                self.specification.primary.symbol
            ),
            areas.header,
        ));
        actions
    }

    fn activate_action(&mut self, id: &str) -> bool {
        if id == "control:refresh-header" {
            return self.activate_control(ChartControl::Refresh);
        }
        ChartControl::from_action_id(id).is_some_and(|control| self.activate_control(control))
    }

    fn poll_intents(&mut self) -> Vec<AppIntent> {
        self.poll_history();
        std::mem::take(&mut self.pending_intents)
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let areas = chart_areas(area);

        match self.prepared_chart() {
            Ok(chart) => {
                let window = self.chart_window(&chart);
                self.render_header(frame, areas.header, &chart);
                let plots = plot_areas(
                    areas.plot,
                    !chart.rsi_lines.is_empty(),
                    self.specification.has_study(Study::Volume),
                );
                self.render_price_chart(frame, plots.price, &chart, &window);
                if let Some(rsi_area) = plots.rsi {
                    self.render_rsi_chart(frame, rsi_area, &chart, &window);
                }
                if let Some(volume_area) = plots.volume {
                    self.render_volume_chart(frame, volume_area, &chart, &window);
                }
            }
            Err(error) => {
                frame.render_widget(
                    Paragraph::new(Line::from(vec![
                        Span::styled(
                            format!(" {}  ", self.specification.primary.symbol),
                            Style::new().bg(AMBER.into()).fg(BG.into()).bold(),
                        ),
                        Span::styled(self.status.as_str(), MUTED),
                    ]))
                    .block(Block::new().borders(Borders::ALL).border_style(AMBER)),
                    areas.header,
                );
                self.render_error(frame, areas.plot, &error);
            }
        }

        for (control, control_area) in self.control_areas(areas.footer) {
            let style = if !self.control_enabled(control) {
                Style::new().fg(MUTED.into())
            } else if self.control_active(control) {
                Style::new().bg(CYAN.into()).fg(BG.into()).bold()
            } else {
                Style::new().fg(AMBER.into())
            };
            frame.render_widget(
                Paragraph::new(self.control_label(control)).style(style),
                control_area,
            );
        }
    }

    fn capture_view(&self) -> WorkspaceViewState {
        WorkspaceViewState::new(ID.as_str())
            .with_field(
                "primary_id",
                ViewValue::Text(self.specification.primary.canonical_id.to_string()),
            )
            .with_field(
                "primary_symbol",
                ViewValue::Text(self.specification.primary.symbol.clone()),
            )
            .with_field(
                "period",
                ViewValue::Text(self.specification.period.label().to_owned()),
            )
            .with_field(
                "normalized",
                ViewValue::Boolean(
                    self.specification.normalization == Normalization::PercentChange,
                ),
            )
            .with_field(
                "comparison_ids",
                ViewValue::TextList(
                    self.specification
                        .comparisons
                        .iter()
                        .map(|instrument| instrument.canonical_id.to_string())
                        .collect(),
                ),
            )
            .with_field(
                "comparison_symbols",
                ViewValue::TextList(
                    self.specification
                        .comparisons
                        .iter()
                        .map(|instrument| instrument.symbol.clone())
                        .collect(),
                ),
            )
            .with_field(
                "studies",
                ViewValue::TextList(
                    self.specification
                        .studies
                        .iter()
                        .map(|study| study.label())
                        .collect(),
                ),
            )
            .with_field(
                "cursor_offset",
                ViewValue::Unsigned(self.cursor_offset as u64),
            )
            .with_field(
                "visible_observations",
                ViewValue::Unsigned(self.viewport.visible_observations.unwrap_or(0) as u64),
            )
            .with_field(
                "pan_offset",
                ViewValue::Unsigned(self.viewport.pan_offset as u64),
            )
            .with_field(
                "display_mode",
                ViewValue::Text(self.display_mode.label().to_owned()),
            )
            .with_field(
                "line_mode",
                ViewValue::Text(self.line_mode.label().to_owned()),
            )
    }

    fn restore_view(&mut self, state: &WorkspaceViewState) -> ViewRestoreReport {
        if !state.workspace.eq_ignore_ascii_case(ID.as_str()) {
            return ViewRestoreReport::warning(format!(
                "saved state belongs to {}, not charting",
                state.workspace
            ));
        }
        let mut report = ViewRestoreReport::default();
        let previous_primary = self.specification.primary.clone();
        let previous_period = self.specification.period;
        let previous_comparisons = self.specification.comparisons.clone();

        let primary_id = state.fields.get("primary_id").and_then(ViewValue::as_text);
        let primary_symbol = state
            .fields
            .get("primary_symbol")
            .and_then(ViewValue::as_text);
        if let (Some(id), Some(symbol)) = (primary_id, primary_symbol) {
            if id.trim().is_empty() || symbol.trim().is_empty() {
                report.skipped_fields += 2;
                report
                    .warnings
                    .push("chart primary instrument is invalid".to_owned());
            } else {
                self.specification
                    .set_primary(ChartInstrument::new(id, symbol));
                report.restored_fields += 2;
            }
        }
        if let Some(value) = state.fields.get("period") {
            match value.as_text().and_then(ChartPeriod::parse) {
                Some(period) => {
                    self.specification.period = period;
                    report.restored_fields += 1;
                }
                None => report.skipped_fields += 1,
            }
        }
        if let Some(normalized) = state
            .fields
            .get("normalized")
            .and_then(ViewValue::as_boolean)
        {
            self.specification.normalization = if normalized {
                Normalization::PercentChange
            } else {
                Normalization::Price
            };
            report.restored_fields += 1;
        }

        let comparison_ids = state
            .fields
            .get("comparison_ids")
            .and_then(ViewValue::as_text_list);
        let comparison_symbols = state
            .fields
            .get("comparison_symbols")
            .and_then(ViewValue::as_text_list);
        if let (Some(ids), Some(symbols)) = (comparison_ids, comparison_symbols) {
            self.specification.comparisons.clear();
            if ids.len() != symbols.len() {
                report.skipped_fields += 2;
                report
                    .warnings
                    .push("chart comparison identities are incomplete".to_owned());
            } else {
                for (id, symbol) in ids.iter().zip(symbols) {
                    if self
                        .specification
                        .add_comparison(ChartInstrument::new(id, symbol))
                        .is_err()
                    {
                        report.skipped_fields += 1;
                    } else {
                        report.restored_fields += 1;
                    }
                }
            }
        }
        if let Some(studies) = state
            .fields
            .get("studies")
            .and_then(ViewValue::as_text_list)
        {
            let parsed = studies
                .iter()
                .filter_map(|study| parse_saved_study(study))
                .collect::<Vec<_>>();
            report.restored_fields += parsed.len();
            report.skipped_fields += studies.len().saturating_sub(parsed.len());
            self.specification.studies = parsed;
        }
        if let Some(cursor) = state
            .fields
            .get("cursor_offset")
            .and_then(ViewValue::as_unsigned)
            .and_then(|value| usize::try_from(value).ok())
        {
            self.cursor_offset = cursor.min(10_000);
            report.restored_fields += 1;
        }
        if let Some(visible) = state
            .fields
            .get("visible_observations")
            .and_then(ViewValue::as_unsigned)
            .and_then(|value| usize::try_from(value).ok())
        {
            self.viewport.visible_observations = (visible > 0).then_some(visible);
            report.restored_fields += 1;
        }
        if let Some(offset) = state
            .fields
            .get("pan_offset")
            .and_then(ViewValue::as_unsigned)
            .and_then(|value| usize::try_from(value).ok())
        {
            self.viewport.pan_offset = offset.min(10_000);
            report.restored_fields += 1;
        }
        if let Some(mode) = state
            .fields
            .get("display_mode")
            .and_then(ViewValue::as_text)
        {
            match mode {
                "CANDLES" => self.display_mode = ChartDisplayMode::Candlesticks,
                "LINE" => self.display_mode = ChartDisplayMode::Line,
                _ => report.skipped_fields += 1,
            }
            if matches!(mode, "CANDLES" | "LINE") {
                report.restored_fields += 1;
            }
        }
        if let Some(mode) = state.fields.get("line_mode").and_then(ViewValue::as_text) {
            match mode {
                "SMOOTH" => self.line_mode = ChartLineMode::Smooth,
                "COMPAT" => self.line_mode = ChartLineMode::Compatible,
                _ => report.skipped_fields += 1,
            }
            if matches!(mode, "SMOOTH" | "COMPAT") {
                report.restored_fields += 1;
            }
        }

        const KNOWN_FIELDS: [&str; 12] = [
            "primary_id",
            "primary_symbol",
            "period",
            "normalized",
            "comparison_ids",
            "comparison_symbols",
            "studies",
            "cursor_offset",
            "visible_observations",
            "pan_offset",
            "display_mode",
            "line_mode",
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
                .push(format!("ignored {unknown} future chart field(s)"));
        }
        if self.specification.primary != previous_primary
            || self.specification.period != previous_period
            || self.specification.comparisons != previous_comparisons
        {
            self.queue_history();
        } else {
            self.constrain_view_to_history();
        }
        self.status = format!(
            "SAVED VIEW RESTORED · {} FIELD(S) · {} SKIPPED",
            report.restored_fields, report.skipped_fields
        );
        report
    }
}

fn parse_saved_study(value: &str) -> Option<Study> {
    if value == "VOLUME" {
        return Some(Study::Volume);
    }
    let (kind, parameter) = value.split_once(' ')?;
    let parameter = parameter.parse::<usize>().ok()?;
    match kind {
        "SMA" if parameter > 1 => Some(Study::SimpleMovingAverage { window: parameter }),
        "EMA" if parameter > 1 => Some(Study::ExponentialMovingAverage { window: parameter }),
        "RSI" if parameter > 1 => Some(Study::RelativeStrengthIndex { period: parameter }),
        _ => None,
    }
}

fn prepare_chart(
    specification: &ChartSpecification,
    series: &[HistorySeries],
) -> Result<PreparedChart, HistoryError> {
    let Some(primary) = series.first() else {
        return Err(HistoryError::Unavailable("no series returned".to_owned()));
    };
    if primary.bars.is_empty() {
        return Err(HistoryError::Unavailable(format!(
            "{} returned no observations",
            primary.instrument.symbol
        )));
    }

    let first_close = primary
        .bars
        .first()
        .map(|bar| bar.close)
        .unwrap_or_default();
    let last = primary.bars.last().map(|bar| bar.close).unwrap_or_default();
    let change_percent = if first_close.abs() < f64::EPSILON {
        0.0
    } else {
        ((last / first_close) - 1.0) * 100.0
    };
    let volume_bars = primary
        .bars
        .iter()
        .map(|bar| bar.volume)
        .collect::<Vec<_>>();
    let primary_closes = primary.bars.iter().map(|bar| bar.close).collect::<Vec<_>>();
    let primary_bars = primary.bars.clone();

    let mut lines = series
        .iter()
        .enumerate()
        .filter(|(_, current)| !current.bars.is_empty())
        .map(|(index, current)| {
            let closes = current.bars.iter().map(|bar| bar.close).collect::<Vec<_>>();
            let values = match specification.normalization {
                Normalization::Price => closes,
                Normalization::PercentChange => percent_change(&closes),
            };
            PreparedLine {
                name: current.instrument.symbol.clone(),
                points: values
                    .into_iter()
                    .enumerate()
                    .map(|(point, value)| (point as f64, value))
                    .collect(),
                color: SERIES_COLORS[index % SERIES_COLORS.len()].into(),
            }
        })
        .collect::<Vec<_>>();

    let mut rsi_lines = Vec::new();
    for study in &specification.studies {
        if let Study::RelativeStrengthIndex { period } = *study {
            let points = rsi(&primary_closes, period)
                .into_iter()
                .enumerate()
                .filter_map(|(index, value)| value.map(|value| (index as f64, value)))
                .collect::<Vec<_>>();
            if !points.is_empty() {
                rsi_lines.push(PreparedLine {
                    name: format!("RSI {period}"),
                    points,
                    color: CYAN.into(),
                });
            }
            continue;
        }

        let (moving_average, name, color) = match *study {
            Study::SimpleMovingAverage { window } => (
                sma(&primary_closes, window),
                format!("SMA {window}"),
                if window == MOVING_AVERAGE_FAST {
                    AMBER.into()
                } else {
                    Color::LightMagenta
                },
            ),
            Study::ExponentialMovingAverage { window } => (
                ema(&primary_closes, window),
                format!("EMA {window}"),
                if window == MOVING_AVERAGE_FAST {
                    AMBER.into()
                } else {
                    Color::LightMagenta
                },
            ),
            Study::RelativeStrengthIndex { .. } | Study::Volume => continue,
        };
        let points = moving_average
            .into_iter()
            .enumerate()
            .filter_map(|(index, value)| {
                value.map(|average| {
                    let plotted = match specification.normalization {
                        Normalization::Price => average,
                        Normalization::PercentChange if first_close.abs() < f64::EPSILON => 0.0,
                        Normalization::PercentChange => ((average / first_close) - 1.0) * 100.0,
                    };
                    (index as f64, plotted)
                })
            })
            .collect::<Vec<_>>();
        if !points.is_empty() {
            lines.push(PreparedLine {
                name,
                points,
                color,
            });
        }
    }

    let (minimum, maximum) = lines
        .iter()
        .flat_map(|line| line.points.iter().map(|point| point.1))
        .filter(|value| value.is_finite())
        .fold(
            (f64::INFINITY, f64::NEG_INFINITY),
            |(minimum, maximum), value| (minimum.min(value), maximum.max(value)),
        );
    let y_bounds = padded_bounds(minimum, maximum);
    let primary_values = lines
        .first()
        .map(|line| line.points.iter().map(|point| point.1).collect())
        .unwrap_or_default();
    let average_volume = if volume_bars.is_empty() {
        0
    } else {
        (volume_bars
            .iter()
            .map(|value| u128::from(*value))
            .sum::<u128>()
            / volume_bars.len() as u128) as u64
    };
    Ok(PreparedChart {
        lines,
        rsi_lines,
        primary_values,
        primary_closes,
        primary_bars,
        volume_bars,
        y_bounds,
        average_volume,
        last,
        change_percent,
        quality: primary.quality.label(),
        source: primary.source.clone(),
    })
}

fn visible_points(points: &[(f64, f64)], range: &Range<usize>) -> Vec<(f64, f64)> {
    points
        .iter()
        .filter(|point| range.contains(&(point.0 as usize)))
        .map(|point| (point.0 - range.start as f64, point.1))
        .collect()
}

fn window_time_labels(chart: &PreparedChart, range: &Range<usize>) -> [String; 3] {
    let Some(first) = chart.primary_bars.get(range.start) else {
        return [String::new(), String::new(), String::new()];
    };
    let last_index = range.end.saturating_sub(1);
    let middle_index = range.start + range.len().saturating_sub(1) / 2;
    let last = chart.primary_bars.get(last_index).unwrap_or(first);
    let middle = chart.primary_bars.get(middle_index).unwrap_or(first);
    [
        candle_time_label(first.timestamp, last.timestamp),
        candle_time_label(middle.timestamp, first.timestamp),
        candle_time_label(last.timestamp, first.timestamp),
    ]
}

fn inspection_time_label(timestamp: i64) -> String {
    DateTime::from_timestamp(timestamp, 0)
        .map(|time| {
            time.with_timezone(&Local)
                .format("%d %b %H:%M")
                .to_string()
                .to_ascii_uppercase()
        })
        .unwrap_or_else(|| "TIME UNAVAILABLE".to_owned())
}

fn line_inspection_title(
    chart: &PreparedChart,
    window: &ChartWindow,
    normalization: Normalization,
) -> String {
    let close = chart
        .primary_closes
        .get(window.selected_index)
        .copied()
        .unwrap_or_default();
    let plotted = chart
        .primary_values
        .get(window.selected_index)
        .copied()
        .unwrap_or_default();
    let time = chart.primary_bars.get(window.selected_index).map_or_else(
        || "TIME UNAVAILABLE".to_owned(),
        |bar| inspection_time_label(bar.timestamp),
    );
    let plotted = match normalization {
        Normalization::Price => format!("PLOT {plotted:.2}"),
        Normalization::PercentChange => format!("PLOT {plotted:+.2}%"),
    };
    format!(
        "PX {close:.2} · {plotted} · {time} · VIEW {}-{}/{}",
        window.range.start + 1,
        window.range.end,
        chart.primary_values.len()
    )
}

fn candle_inspection_title(chart: &PreparedChart, window: &ChartWindow) -> String {
    let Some(bar) = chart.primary_bars.get(window.selected_index) else {
        return "CANDLESTICKS".to_owned();
    };
    format!(
        "CANDLESTICKS · C {:.2} · O {:.2} H {:.2} L {:.2} · {} · VIEW {}-{}/{}",
        bar.close,
        bar.open,
        bar.high,
        bar.low,
        inspection_time_label(bar.timestamp),
        window.range.start + 1,
        window.range.end,
        chart.primary_bars.len()
    )
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Candle {
    timestamp: i64,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
}

fn render_candlesticks(frame: &mut Frame, area: Rect, chart: &PreparedChart, window: &ChartWindow) {
    let title = candle_inspection_title(chart, window);
    let block = terminal_block("OHLC", &title);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if window.range.is_empty() || inner.width < 12 || inner.height < 4 {
        return;
    }

    let candles = chart
        .primary_bars
        .get(window.range.clone())
        .unwrap_or_default()
        .iter()
        .map(|bar| Candle {
            timestamp: bar.timestamp,
            open: bar.open,
            high: bar.high,
            low: bar.low,
            close: bar.close,
        })
        .collect::<Vec<_>>();
    if candles.is_empty() {
        return;
    }
    let (low, high) = candles
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), candle| {
            (low.min(candle.low), high.max(candle.high))
        });
    let padding = ((high - low) * 0.05).max(high.abs() * 0.0005).max(1e-9);
    let (y_low, y_high) = (low - padding, high + padding);
    let y_labels = [
        format!("{y_high:.2}"),
        format!("{:.2}", (y_low + y_high) / 2.0),
        format!("{y_low:.2}"),
    ];
    let gutter = y_labels
        .iter()
        .map(|label| label.chars().count())
        .max()
        .unwrap_or_default() as u16
        + 1;
    if inner.width <= gutter + 2 || inner.height <= 2 {
        return;
    }
    let plot = Rect::new(
        inner.x + gutter,
        inner.y,
        inner.width - gutter,
        inner.height - 1,
    );
    let margin = candle_margin_columns(plot.width, CANDLE_RIGHT_MARGIN_PERCENT);
    let usable = plot.width - margin;
    let max_candles = (usize::from(usable) / 2).max(1);
    let ranges = candle_bucket_ranges(candles.len(), max_candles);
    let display = ranges
        .iter()
        .map(|range| aggregate_candles(&candles[range.clone()]))
        .collect::<Vec<_>>();
    let sample_indices = ranges
        .iter()
        .map(|range| range.end.saturating_sub(1))
        .collect::<Vec<_>>();
    let count = display.len();
    if count == 0 {
        return;
    }
    let slot = (usize::from(usable) / count).clamp(2, 4) as u16;
    let body_width = slot - 1;
    let slot_x =
        |index: usize| plot.x + usable - (count.saturating_sub(index) as u16).saturating_mul(slot);
    let candle_center = |index: usize| slot_x(index) + slot - body_width + body_width / 2;
    let selected_display = ranges
        .iter()
        .position(|range| range.contains(&window.selected_index.saturating_sub(window.range.start)))
        .unwrap_or(count - 1);

    let buffer = frame.buffer_mut();
    let cursor_x = candle_center(selected_display);
    for row in plot.y..plot.y + plot.height {
        if let Some(cell) = buffer.cell_mut((cursor_x, row)) {
            cell.set_char('┊').set_fg(AMBER.into());
        }
    }

    for (index, candle) in display.iter().enumerate() {
        let previous_close = index.checked_sub(1).map(|previous| display[previous].close);
        let color = candle_color(candle, previous_close);
        let body_x = slot_x(index) + slot - body_width;
        let wick_x = body_x + body_width / 2;
        for (row, glyph) in candle_column(candle, y_low, y_high, plot.height) {
            for column in body_x..body_x + body_width {
                let glyph = if column == wick_x {
                    glyph
                } else {
                    candle_body_only(glyph)
                };
                if glyph == ' ' {
                    continue;
                }
                if let Some(cell) = buffer.cell_mut((column, plot.y + row)) {
                    cell.set_char(glyph).set_fg(color);
                }
            }
        }
    }

    let mut overlay = BrailleOverlay::new(plot.width, plot.height);
    for line in chart.lines.iter().skip(1) {
        let mut previous = None;
        for (display_index, sample_index) in sample_indices.iter().copied().enumerate() {
            let sample_index = window.range.start + sample_index;
            let point = line
                .points
                .iter()
                .find(|point| point.0 as usize == sample_index)
                .filter(|point| (y_low..=y_high).contains(&point.1))
                .map(|point| {
                    let center = candle_center(display_index);
                    (
                        i32::from(center.saturating_sub(plot.x)) * 2,
                        candle_scale(point.1, y_low, y_high, usize::from(plot.height) * 4) as i32,
                    )
                });
            match (previous, point) {
                (Some(start), Some(end)) => overlay.line(start, end, line.color),
                (None, Some(point)) => overlay.dot(point.0, point.1, line.color),
                _ => {}
            }
            previous = point;
        }
    }
    overlay.blit(buffer, plot);

    if margin > 0 {
        let visible_last = candles.last().map_or(0.0, |candle| candle.close);
        let row = plot.y
            + (candle_scale(visible_last, y_low, y_high, usize::from(plot.height) * 2) / 2) as u16;
        for column in plot.x + usable..plot.x + plot.width {
            if let Some(cell) = buffer.cell_mut((column, row)) {
                cell.set_char('─').set_fg(CYAN.into());
            }
        }
        let tag = format!("{visible_last:.2}");
        let tag_width = tag.chars().count() as u16;
        if margin >= tag_width {
            buffer.set_string(
                plot.x + plot.width - tag_width,
                row,
                tag,
                Style::new().fg(CYAN.into()).bold(),
            );
        }
    }

    let label_rows = [plot.y, plot.y + plot.height / 2, plot.y + plot.height - 1];
    for (label, row) in y_labels.iter().zip(label_rows) {
        buffer.set_string(plot.x - 1 - label.chars().count() as u16, row, label, MUTED);
    }
    let axis_row = plot.y + plot.height;
    let first_timestamp = candles[0].timestamp;
    let last_timestamp = candles[candles.len() - 1].timestamp;
    let first = candle_time_label(first_timestamp, last_timestamp);
    let last = candle_time_label(last_timestamp, first_timestamp);
    buffer.set_string(plot.x, axis_row, &first, MUTED);
    let last_x = plot.x + plot.width.saturating_sub(last.chars().count() as u16);
    buffer.set_string(last_x, axis_row, &last, MUTED);
    if plot.width >= 34 {
        let middle = candle_time_label(
            candles[sample_indices[count / 2]].timestamp,
            first_timestamp,
        );
        let width = middle.chars().count() as u16;
        let middle_x = (slot_x(count / 2) + slot / 2)
            .saturating_sub(width / 2)
            .clamp(plot.x, plot.x + plot.width - width);
        buffer.set_string(middle_x, axis_row, middle, MUTED);
    }
}

fn candle_margin_columns(width: u16, percent: u16) -> u16 {
    (u32::from(width) * u32::from(percent) / 100).min(u32::from(width.saturating_sub(2))) as u16
}

fn candle_bucket_ranges(length: usize, maximum: usize) -> Vec<Range<usize>> {
    let count = maximum.min(length);
    if count == 0 {
        return Vec::new();
    }
    (0..count)
        .map(|index| (index * length / count)..((index + 1) * length / count))
        .collect()
}

fn aggregate_candles(candles: &[Candle]) -> Candle {
    let (high, low) = candles
        .iter()
        .fold((f64::NEG_INFINITY, f64::INFINITY), |(high, low), candle| {
            (high.max(candle.high), low.min(candle.low))
        });
    Candle {
        timestamp: candles[0].timestamp,
        open: candles[0].open,
        high,
        low,
        close: candles[candles.len() - 1].close,
    }
}

fn candle_scale(value: f64, low: f64, high: f64, sub_rows: usize) -> usize {
    (((high - value) / (high - low) * sub_rows as f64) as usize).min(sub_rows - 1)
}

#[derive(Clone, Copy, PartialEq)]
enum CandleHalf {
    Body,
    Wick,
    Empty,
}

fn candle_column(candle: &Candle, low: f64, high: f64, rows: u16) -> Vec<(u16, char)> {
    let sub_rows = usize::from(rows) * 2;
    let body_top = candle_scale(candle.open.max(candle.close), low, high, sub_rows);
    let body_bottom = candle_scale(candle.open.min(candle.close), low, high, sub_rows);
    let wick_top = candle_scale(candle.high, low, high, sub_rows);
    let wick_bottom = candle_scale(candle.low, low, high, sub_rows);
    let half = |sub_row: usize| {
        if (body_top..=body_bottom).contains(&sub_row) {
            CandleHalf::Body
        } else if (wick_top..=wick_bottom).contains(&sub_row) {
            CandleHalf::Wick
        } else {
            CandleHalf::Empty
        }
    };
    (0..rows)
        .filter_map(|row| {
            let glyph = match (half(usize::from(row) * 2), half(usize::from(row) * 2 + 1)) {
                (CandleHalf::Body, CandleHalf::Body) => '█',
                (CandleHalf::Body, _) => '▀',
                (_, CandleHalf::Body) => '▄',
                (CandleHalf::Wick, CandleHalf::Wick) => '│',
                (CandleHalf::Wick, CandleHalf::Empty) => '╵',
                (CandleHalf::Empty, CandleHalf::Wick) => '╷',
                (CandleHalf::Empty, CandleHalf::Empty) => return None,
            };
            Some((row, glyph))
        })
        .collect()
}

fn candle_body_only(glyph: char) -> char {
    match glyph {
        '█' | '▀' | '▄' => glyph,
        _ => ' ',
    }
}

fn candle_color(candle: &Candle, previous_close: Option<f64>) -> Color {
    if candle.close > candle.open {
        return GREEN.into();
    }
    if candle.close < candle.open {
        return RED.into();
    }
    match previous_close {
        Some(previous) if candle.close < previous => RED.into(),
        Some(_) => GREEN.into(),
        None => YELLOW.into(),
    }
}

fn candle_time_label(timestamp: i64, other_timestamp: i64) -> String {
    let format = if (timestamp - other_timestamp).abs() <= 2 * 86_400 {
        "%H:%M"
    } else {
        "%d %b"
    };
    DateTime::from_timestamp(timestamp, 0)
        .map(|time| time.with_timezone(&Local).format(format).to_string())
        .unwrap_or_default()
}

const BRAILLE_BITS: [[u8; 2]; 4] = [[0x01, 0x08], [0x02, 0x10], [0x04, 0x20], [0x40, 0x80]];

struct BrailleOverlay {
    columns: u16,
    rows: u16,
    cells: Vec<(u8, Color)>,
}

impl BrailleOverlay {
    fn new(columns: u16, rows: u16) -> Self {
        Self {
            columns,
            rows,
            cells: vec![(0, Color::Reset); usize::from(columns) * usize::from(rows)],
        }
    }

    fn dot(&mut self, x: i32, y: i32, color: Color) {
        if x < 0 || y < 0 {
            return;
        }
        let (column, row) = (x as u16 / 2, y as u16 / 4);
        if column >= self.columns || row >= self.rows {
            return;
        }
        let cell =
            &mut self.cells[usize::from(row) * usize::from(self.columns) + usize::from(column)];
        cell.0 |= BRAILLE_BITS[y as usize % 4][x as usize % 2];
        cell.1 = color;
    }

    fn line(&mut self, (x0, y0): (i32, i32), (x1, y1): (i32, i32), color: Color) {
        let (delta_x, delta_y) = ((x1 - x0).abs(), -(y1 - y0).abs());
        let (step_x, step_y) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
        let (mut x, mut y, mut error) = (x0, y0, delta_x + delta_y);
        loop {
            self.dot(x, y, color);
            if x == x1 && y == y1 {
                return;
            }
            let doubled = 2 * error;
            if doubled >= delta_y {
                error += delta_y;
                x += step_x;
            }
            if doubled <= delta_x {
                error += delta_x;
                y += step_y;
            }
        }
    }

    fn blit(&self, buffer: &mut ratatui::buffer::Buffer, plot: Rect) {
        for row in 0..self.rows {
            for column in 0..self.columns {
                let (mask, color) =
                    self.cells[usize::from(row) * usize::from(self.columns) + usize::from(column)];
                if mask == 0 {
                    continue;
                }
                let Some(cell) = buffer.cell_mut((plot.x + column, plot.y + row)) else {
                    continue;
                };
                if matches!(cell.symbol(), "█" | "▀" | "▄") {
                    continue;
                }
                cell.set_char(char::from_u32(0x2800 + u32::from(mask)).unwrap_or('·'))
                    .set_fg(color);
            }
        }
    }
}

fn compact_volume(volume: u64) -> String {
    if volume >= 1_000_000_000 {
        format!("{:.1}B", volume as f64 / 1_000_000_000.0)
    } else if volume >= 1_000_000 {
        format!("{:.1}M", volume as f64 / 1_000_000.0)
    } else if volume >= 1_000 {
        format!("{:.1}K", volume as f64 / 1_000.0)
    } else {
        volume.to_string()
    }
}

fn padded_bounds(minimum: f64, maximum: f64) -> [f64; 2] {
    if !minimum.is_finite() || !maximum.is_finite() {
        return [0.0, 1.0];
    }
    let range = maximum - minimum;
    let padding = if range.abs() < f64::EPSILON {
        maximum.abs().max(1.0) * 0.02
    } else {
        range * 0.08
    };
    [minimum - padding, maximum + padding]
}

fn apply_chart_command(
    specification: &mut ChartSpecification,
    display_mode: &mut ChartDisplayMode,
    args: &[String],
    status: &mut String,
) {
    if args.is_empty() {
        *status = "CHART READY · PROVIDE AN INSTRUMENT OR USE KEYBOARD CONTROLS".to_owned();
        return;
    }
    status.clear();

    let primary_end = args
        .iter()
        .position(|token| is_option_token(token))
        .unwrap_or(args.len());
    if primary_end > 0 {
        specification.set_primary(ChartInstrument::from_terminal_subject(
            &args[..primary_end].join(" "),
        ));
        specification.comparisons.clear();
    }

    let mut index = primary_end;
    while index < args.len() {
        let token = args[index].to_ascii_uppercase();
        match token.as_str() {
            "COMPARE" | "VS" => {
                index += 1;
                let start = index;
                while index < args.len() && !is_option_token(&args[index]) {
                    for symbol in args[index]
                        .split(',')
                        .filter(|symbol| !symbol.trim().is_empty())
                    {
                        if let Err(error) = specification
                            .add_comparison(ChartInstrument::from_terminal_subject(symbol))
                        {
                            *status = format!("CHART ERROR · {error}");
                        }
                    }
                    index += 1;
                }
                if index > start && specification.normalization == Normalization::Price {
                    specification.normalization = Normalization::PercentChange;
                }
            }
            "PERIOD" => {
                if let Some(period) = args
                    .get(index + 1)
                    .and_then(|value| ChartPeriod::parse(value))
                {
                    specification.period = period;
                    index += 2;
                } else {
                    *status = "CHART ERROR · EXPECTED PERIOD 1D/1M/6M/YTD/1Y/5Y".to_owned();
                    index += 1;
                }
            }
            "NORMALIZE" | "NORMALIZED" | "PERFORMANCE" => {
                specification.normalization = Normalization::PercentChange;
                index += 1;
            }
            "PRICE" | "ABSOLUTE" => {
                specification.normalization = Normalization::Price;
                index += 1;
            }
            "STYLE" => match args.get(index + 1).map(|value| value.to_ascii_uppercase()) {
                Some(style) if matches!(style.as_str(), "CANDLES" | "CANDLE" | "OHLC") => {
                    *display_mode = ChartDisplayMode::Candlesticks;
                    index += 2;
                }
                Some(style) if style == "LINE" => {
                    *display_mode = ChartDisplayMode::Line;
                    index += 2;
                }
                _ => {
                    *status = "CHART ERROR · EXPECTED STYLE CANDLES/LINE".to_owned();
                    index += 1;
                }
            },
            "VOLUME" => {
                if !specification.has_study(Study::Volume) {
                    let _ = specification.toggle_study(Study::Volume);
                }
                index += 1;
            }
            "SMA" | "STUDY" => {
                let parsed_window = args
                    .get(index + 1)
                    .and_then(|value| parse_sma_window(value));
                let window = parsed_window.unwrap_or(20);
                if let Err(error) =
                    specification.toggle_study(Study::SimpleMovingAverage { window })
                {
                    *status = format!("CHART ERROR · {error}");
                }
                index += usize::from(parsed_window.is_some()) + 1;
            }
            "EMA" => {
                let parsed_window = args
                    .get(index + 1)
                    .and_then(|value| parse_ema_window(value));
                let window = parsed_window.unwrap_or(MOVING_AVERAGE_FAST);
                if let Err(error) =
                    specification.toggle_study(Study::ExponentialMovingAverage { window })
                {
                    *status = format!("CHART ERROR · {error}");
                }
                index += usize::from(parsed_window.is_some()) + 1;
            }
            "RSI" => {
                let parsed_period = args
                    .get(index + 1)
                    .and_then(|value| parse_rsi_period(value));
                let period = parsed_period.unwrap_or(RSI_PERIOD);
                let study = Study::RelativeStrengthIndex { period };
                if !specification.has_study(study) {
                    if let Err(error) = specification.toggle_study(study) {
                        *status = format!("CHART ERROR · {error}");
                    }
                }
                index += usize::from(parsed_period.is_some()) + 1;
            }
            _ => {
                if let Some(period) = ChartPeriod::parse(&token) {
                    specification.period = period;
                } else if let Some(window) = parse_ema_window(&token) {
                    let study = Study::ExponentialMovingAverage { window };
                    if !specification.has_study(study) {
                        let _ = specification.toggle_study(study);
                    }
                } else if let Some(period) = parse_rsi_period(&token) {
                    let study = Study::RelativeStrengthIndex { period };
                    if !specification.has_study(study) {
                        let _ = specification.toggle_study(study);
                    }
                } else if let Some(window) = parse_sma_window(&token) {
                    let study = Study::SimpleMovingAverage { window };
                    if !specification.has_study(study) {
                        let _ = specification.toggle_study(study);
                    }
                }
                index += 1;
            }
        }
    }

    if !status.starts_with("CHART ERROR") {
        *status = format!(
            "{} · {} · {} COMPARISON(S)",
            specification.primary.symbol,
            specification.period.label(),
            specification.comparisons.len()
        );
    }
}

fn is_option_token(token: &str) -> bool {
    let upper = token.to_ascii_uppercase();
    matches!(
        upper.as_str(),
        "COMPARE"
            | "VS"
            | "PERIOD"
            | "NORMALIZE"
            | "NORMALIZED"
            | "PERFORMANCE"
            | "PRICE"
            | "ABSOLUTE"
            | "STYLE"
            | "VOLUME"
            | "SMA"
            | "EMA"
            | "RSI"
            | "STUDY"
    ) || ChartPeriod::parse(&upper).is_some()
        || parse_sma_window(&upper).is_some()
        || parse_ema_window(&upper).is_some()
        || parse_rsi_period(&upper).is_some()
}

fn parse_sma_window(value: &str) -> Option<usize> {
    let upper = value.to_ascii_uppercase();
    let digits = upper
        .strip_prefix("SMA")
        .or_else(|| upper.strip_prefix("MA"))
        .unwrap_or(&upper);
    let window = digits.parse::<usize>().ok()?;
    (window >= 2).then_some(window)
}

fn parse_ema_window(value: &str) -> Option<usize> {
    let upper = value.to_ascii_uppercase();
    let digits = if let Some(digits) = upper.strip_prefix("EMA") {
        digits
    } else if upper.chars().all(|character| character.is_ascii_digit()) {
        &upper
    } else {
        return None;
    };
    let window = digits.parse::<usize>().ok()?;
    (window >= 2).then_some(window)
}

fn parse_rsi_period(value: &str) -> Option<usize> {
    let upper = value.to_ascii_uppercase();
    let digits = if let Some(digits) = upper.strip_prefix("RSI") {
        digits
    } else if upper.chars().all(|character| character.is_ascii_digit()) {
        &upper
    } else {
        return None;
    };
    let period = digits.parse::<usize>().ok()?;
    (period >= 2).then_some(period)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::charting::{HistoryQuality, PriceBar};
    use crossterm::event::{KeyModifiers, MouseButton};
    use ratatui::{backend::TestBackend, Terminal};

    struct StubHistory;

    impl ChartHistoryQuery for StubHistory {
        fn load_history(&self, request: &HistoryRequest) -> Result<HistorySeries, HistoryError> {
            Ok(HistorySeries {
                instrument: request.instrument.clone(),
                bars: [100.0, 110.0, 121.0]
                    .into_iter()
                    .enumerate()
                    .map(|(index, close)| PriceBar {
                        timestamp: index as i64,
                        open: close,
                        high: close,
                        low: close,
                        close,
                        volume: 1_000_000 + index as u64,
                    })
                    .collect(),
                quality: HistoryQuality::Replayed,
                source: "TEST".to_owned(),
            })
        }
    }

    fn dense_workspace() -> ChartingWorkspace {
        let mut workspace = ChartingWorkspace::new(Arc::new(StubHistory));
        workspace.history = Some(vec![HistorySeries {
            instrument: ChartInstrument::from_terminal_subject("AAPL"),
            bars: (0..64)
                .map(|index| {
                    let close = 100.0 + f64::from(index);
                    PriceBar {
                        timestamp: 1_700_000_000 + i64::from(index) * 86_400,
                        open: close - 0.5,
                        high: close + 1.0,
                        low: close - 1.0,
                        close,
                        volume: 1_000_000 + index as u64,
                    }
                })
                .collect(),
            quality: HistoryQuality::Replayed,
            source: "TEST".to_owned(),
        }]);
        workspace.history_error = None;
        workspace.pending_refresh = None;
        workspace
    }

    #[test]
    fn chart_command_sets_subject_comparisons_period_mode_and_study() {
        let mut workspace = ChartingWorkspace::new(Arc::new(StubHistory));
        workspace.handle_command(&CommandInvocation {
            function: "CHART".to_owned(),
            args: [
                "MSFT", "COMPARE", "SPY,QQQ", "6M", "SMA50", "EMA12", "RSI7", "STYLE", "LINE",
            ]
            .into_iter()
            .map(ToOwned::to_owned)
            .collect(),
        });

        assert_eq!(workspace.specification.primary.symbol, "MSFT");
        assert_eq!(workspace.specification.period, ChartPeriod::SixMonths);
        assert_eq!(
            workspace.specification.normalization,
            Normalization::PercentChange
        );
        assert_eq!(workspace.specification.comparisons.len(), 2);
        assert!(workspace
            .specification
            .has_study(Study::SimpleMovingAverage { window: 50 }));
        assert!(workspace
            .specification
            .has_study(Study::ExponentialMovingAverage { window: 12 }));
        assert!(workspace
            .specification
            .has_study(Study::RelativeStrengthIndex { period: 7 }));
        assert_eq!(workspace.display_mode, ChartDisplayMode::Line);
    }

    #[test]
    fn keyboard_controls_change_only_chart_specification() {
        let mut workspace = ChartingWorkspace::new(Arc::new(StubHistory));
        let initial_period = workspace.specification.period;

        assert!(workspace.handle_key(KeyEvent::new(KeyCode::Char(']'), KeyModifiers::NONE)));
        assert_eq!(workspace.specification.period, initial_period.next());
        assert!(workspace.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE)));
        assert_eq!(
            workspace.specification.normalization,
            Normalization::PercentChange
        );
        assert!(workspace.handle_key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE)));
        assert!(!workspace.specification.has_study(Study::Volume));
        assert!(workspace.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE)));
        assert!(workspace
            .specification
            .has_study(Study::ExponentialMovingAverage {
                window: MOVING_AVERAGE_FAST
            }));
        assert!(workspace.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE)));
        assert!(!workspace
            .specification
            .has_study(Study::RelativeStrengthIndex { period: RSI_PERIOD }));
        assert!(workspace.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE)));
        assert_eq!(workspace.display_mode, ChartDisplayMode::Line);
    }

    #[test]
    fn inspection_cursor_moves_back_and_returns_to_latest() {
        let mut workspace = ChartingWorkspace::new(Arc::new(StubHistory));
        assert!(workspace.handle_key(KeyEvent::new(KeyCode::Char(','), KeyModifiers::NONE)));
        assert!(workspace.handle_key(KeyEvent::new(KeyCode::Char(','), KeyModifiers::NONE)));
        assert_eq!(workspace.cursor_offset, 2);
        assert!(workspace.handle_key(KeyEvent::new(KeyCode::Char('.'), KeyModifiers::NONE)));
        assert_eq!(workspace.cursor_offset, 1);
        assert!(workspace.handle_key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE)));
        assert_eq!(workspace.cursor_offset, 0);
    }

    #[test]
    fn arrow_keys_zoom_and_pan_the_visible_history_around_the_inspection_cursor() {
        let mut workspace = dense_workspace();
        assert_eq!(workspace.visible_range(64), 0..64);

        assert!(workspace.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)));
        let zoomed = workspace.visible_range(64);
        assert!(zoomed.len() < 64);
        assert_eq!(zoomed.end, 64);

        assert!(workspace.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE)));
        let panned = workspace.visible_range(64);
        assert!(panned.start < zoomed.start);
        assert!(panned.end < zoomed.end);
        assert!(panned.contains(&workspace.selected_index(64)));

        assert!(workspace.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE)));
        assert_eq!(workspace.visible_range(64), zoomed);
        assert!(workspace.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)));
        assert!(workspace.visible_range(64).len() > zoomed.len());
    }

    #[test]
    fn inspected_ohlc_and_line_values_render_even_without_the_statistics_sidebar() {
        let mut workspace = dense_workspace();
        for _ in 0..5 {
            assert!(workspace.handle_key(KeyEvent::new(KeyCode::Char(','), KeyModifiers::NONE)));
        }
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();

        terminal
            .draw(|frame| workspace.render(frame, frame.area()))
            .unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("C 158.00"));

        assert!(workspace.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE)));
        terminal
            .draw(|frame| workspace.render(frame, frame.area()))
            .unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("PX 158.00"));
    }

    #[test]
    fn plot_click_inspects_within_the_zoomed_and_panned_window() {
        let mut workspace = dense_workspace();
        workspace.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        workspace.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        let visible = workspace.visible_range(64);
        let area = Rect::new(0, 0, 120, 30);
        let plot = chart_areas(area).plot;

        assert!(workspace.handle_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: plot.x + 1,
                row: plot.y + 1,
                modifiers: KeyModifiers::NONE,
            },
            area,
        ));
        assert_eq!(workspace.selected_index(64), visible.start);
    }

    #[test]
    fn action_registry_shares_responsive_control_geometry_and_revalidates_state() {
        let mut workspace = ChartingWorkspace::new(Arc::new(StubHistory));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        while workspace.history.is_none() && std::time::Instant::now() < deadline {
            workspace.poll_history();
            std::thread::yield_now();
        }
        let area = Rect::new(7, 4, 80, 24);
        let chart = chart_areas(area);
        let actions = workspace.actions(area);
        let ids = actions
            .iter()
            .map(|action| action.id.as_str())
            .collect::<std::collections::HashSet<_>>();

        assert_eq!(ids.len(), actions.len());
        assert!(actions.iter().all(|action| {
            action.area.x >= area.x
                && action.area.y >= area.y
                && action.area.right() <= area.right()
                && action.area.bottom() <= area.bottom()
        }));
        assert!(actions
            .iter()
            .any(|action| action.id == "period:1Y" && action.preferred));
        assert!(actions
            .iter()
            .any(|action| action.id == "control:inspect-back" && action.enabled));
        assert!(actions
            .iter()
            .any(|action| action.id == "control:inspect-forward" && !action.enabled));
        assert_eq!(
            workspace
                .control_areas(chart.footer)
                .into_iter()
                .map(|(control, area)| (control.action_id(), area))
                .collect::<Vec<_>>(),
            actions
                .iter()
                .filter(|action| action.id != "control:refresh-header")
                .map(|action| (action.id.clone(), action.area))
                .collect::<Vec<_>>()
        );

        let normalization = actions
            .iter()
            .find(|action| action.id == "control:normalization")
            .unwrap()
            .area;
        assert!(workspace.handle_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: normalization.x,
                row: normalization.y,
                modifiers: KeyModifiers::NONE,
            },
            area,
        ));
        assert_eq!(
            workspace.specification.normalization,
            Normalization::PercentChange
        );

        assert!(workspace.activate_action("control:inspect-back"));
        assert_eq!(workspace.cursor_offset, 1);
        assert!(workspace.activate_action("control:inspect-forward"));
        assert_eq!(workspace.cursor_offset, 0);
        assert!(workspace.activate_action("period:6M"));
        assert_eq!(workspace.specification.period, ChartPeriod::SixMonths);
        assert!(!workspace.activate_action("period:bogus"));
        assert!(!workspace.activate_action("control:clear-comparisons"));

        let narrow = workspace.actions(Rect::new(0, 0, 42, 18));
        assert!(narrow
            .iter()
            .all(|action| { action.area.right() <= 42 && action.area.bottom() <= 18 }));
    }

    #[test]
    fn prepared_chart_keeps_ema_and_rsi_on_their_own_scales() {
        let instrument = ChartInstrument::from_terminal_subject("AAPL");
        let mut specification = ChartSpecification::new(instrument.clone());
        specification.studies = vec![
            Study::ExponentialMovingAverage { window: 5 },
            Study::RelativeStrengthIndex { period: 3 },
        ];
        let bars = (0..12)
            .map(|index| {
                let close = 100.0 + f64::from(index) + f64::from(index % 3);
                PriceBar {
                    timestamp: i64::from(index),
                    open: close,
                    high: close + 1.0,
                    low: close - 1.0,
                    close,
                    volume: 1_000 + index as u64,
                }
            })
            .collect();
        let chart = prepare_chart(
            &specification,
            &[HistorySeries {
                instrument,
                bars,
                quality: HistoryQuality::Delayed,
                source: "TEST".to_owned(),
            }],
        )
        .unwrap();

        assert!(chart.lines.iter().any(|line| line.name == "EMA 5"));
        assert_eq!(chart.rsi_lines.len(), 1);
        assert_eq!(chart.rsi_lines[0].name, "RSI 3");
        assert!(chart.rsi_lines[0]
            .points
            .iter()
            .all(|(_, value)| (0.0..=100.0).contains(value)));
        assert!(chart.y_bounds[0] > 90.0);
    }

    #[test]
    fn responsive_plot_layout_prioritizes_rsi_when_height_is_tight() {
        let roomy = plot_areas(Rect::new(0, 0, 120, 30), true, true);
        assert!(roomy.rsi.is_some());
        assert!(roomy.volume.is_some());

        let tight = plot_areas(Rect::new(0, 0, 120, 16), true, true);
        assert!(tight.rsi.is_some());
        assert!(tight.volume.is_none());

        let tiny = plot_areas(Rect::new(0, 0, 120, 10), true, true);
        assert!(tiny.rsi.is_none());
        assert!(tiny.volume.is_none());
        assert_eq!(tiny.price.height, 10);
    }

    #[test]
    fn line_mode_can_fall_back_for_limited_terminal_fonts() {
        let mut workspace = ChartingWorkspace::new(Arc::new(StubHistory));
        assert_eq!(workspace.line_mode.label(), "SMOOTH");
        assert!(workspace.handle_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE)));
        assert_eq!(workspace.line_mode.label(), "COMPAT");
    }

    #[test]
    fn comparisons_force_the_effective_chart_style_to_line() {
        let mut workspace = ChartingWorkspace::new(Arc::new(StubHistory));
        assert_eq!(
            workspace.effective_display_mode(),
            ChartDisplayMode::Candlesticks
        );

        workspace.toggle_default_comparison();

        assert_eq!(workspace.effective_display_mode(), ChartDisplayMode::Line);
    }

    #[test]
    fn half_block_candles_preserve_wicks_bodies_and_doji() {
        let candle = Candle {
            timestamp: 0,
            open: 2.0,
            high: 9.0,
            low: 1.0,
            close: 6.0,
        };
        assert_eq!(
            candle_column(&candle, 0.0, 10.0, 5),
            vec![(0, '╷'), (1, '│'), (2, '█'), (3, '█'), (4, '▀')]
        );

        let doji = Candle {
            timestamp: 0,
            open: 5.0,
            high: 5.0,
            low: 5.0,
            close: 5.0,
        };
        assert_eq!(candle_column(&doji, 0.0, 10.0, 5).len(), 1);
    }

    #[test]
    fn candle_buckets_fill_width_and_preserve_ohlc_extremes() {
        assert_eq!(candle_bucket_ranges(10, 4), vec![0..2, 2..5, 5..7, 7..10]);
        let candles = [
            Candle {
                timestamp: 1,
                open: 10.0,
                high: 12.0,
                low: 9.0,
                close: 11.0,
            },
            Candle {
                timestamp: 2,
                open: 11.0,
                high: 15.0,
                low: 10.5,
                close: 14.0,
            },
        ];

        let merged = aggregate_candles(&candles);

        assert_eq!(
            (merged.open, merged.high, merged.low, merged.close),
            (10.0, 15.0, 9.0, 14.0)
        );
    }

    #[test]
    fn candlestick_renderer_draws_ohlc_into_a_terminal_buffer() {
        let specification = ChartSpecification::new(ChartInstrument::from_terminal_subject("AAPL"));
        let series = StubHistory
            .load_history(&HistoryRequest::new(
                specification.primary.clone(),
                specification.period,
            ))
            .unwrap();
        let chart = prepare_chart(&specification, &[series]).unwrap();
        let window = ChartWindow {
            range: 0..chart.primary_values.len(),
            selected_index: 1,
            x_max: chart.primary_values.len().saturating_sub(1).max(1) as f64,
            y_bounds: chart.y_bounds,
        };
        let mut terminal = Terminal::new(TestBackend::new(70, 14)).unwrap();

        terminal
            .draw(|frame| render_candlesticks(frame, frame.area(), &chart, &window))
            .unwrap();

        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("CANDLESTICKS"));
        assert!(rendered.contains('▀') || rendered.contains('▄'));

        let mut tiny = Terminal::new(TestBackend::new(12, 8)).unwrap();
        tiny.draw(|frame| render_candlesticks(frame, frame.area(), &chart, &window))
            .unwrap();
    }

    #[test]
    fn normalized_comparisons_share_a_zero_origin() {
        let mut spec = ChartSpecification::new(ChartInstrument::from_terminal_subject("AAPL"));
        spec.normalization = Normalization::PercentChange;
        spec.add_comparison(ChartInstrument::from_terminal_subject("SPY"))
            .unwrap();
        let query = StubHistory;
        let series: Vec<HistorySeries> = [&spec.primary, &spec.comparisons[0]]
            .into_iter()
            .map(|instrument| {
                query
                    .load_history(&HistoryRequest::new(instrument.clone(), spec.period))
                    .unwrap()
            })
            .collect();

        let chart = prepare_chart(&spec, &series).unwrap();
        assert_eq!(chart.lines[0].points[0].1, 0.0);
        assert_eq!(chart.lines[1].points[0].1, 0.0);
        assert!((chart.lines[0].points[2].1 - 21.0).abs() < 1e-10);
    }

    #[test]
    fn graph_is_an_exact_workspace_command_alias() {
        let workspace = ChartingWorkspace::new(Arc::new(StubHistory));
        assert_eq!(workspace.descriptor().commands, &["CHART", "GRAPH"]);
        assert_eq!(workspace.descriptor().id, ID);
    }

    #[test]
    fn a_symbol_named_line_is_not_confused_with_the_style_option() {
        let mut workspace = ChartingWorkspace::new(Arc::new(StubHistory));

        workspace.handle_command(&CommandInvocation {
            function: "CHART".to_owned(),
            args: vec!["LINE".to_owned()],
        });

        assert_eq!(workspace.specification.primary.symbol, "LINE");
        assert_eq!(workspace.display_mode, ChartDisplayMode::Candlesticks);
    }

    #[test]
    fn history_loading_never_blocks_workspace_construction() {
        struct SlowHistory;

        impl ChartHistoryQuery for SlowHistory {
            fn load_history(
                &self,
                request: &HistoryRequest,
            ) -> Result<HistorySeries, HistoryError> {
                std::thread::sleep(std::time::Duration::from_millis(200));
                StubHistory.load_history(request)
            }
        }

        let started = std::time::Instant::now();
        let workspace = ChartingWorkspace::new(Arc::new(SlowHistory));
        assert!(started.elapsed() < std::time::Duration::from_millis(100));
        assert!(workspace.history.is_none());
    }

    #[test]
    fn completed_history_is_applied_from_the_background_worker() {
        let mut workspace = ChartingWorkspace::new(Arc::new(StubHistory));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        while workspace.history.is_none() && std::time::Instant::now() < deadline {
            workspace.poll_history();
            std::thread::yield_now();
        }

        assert_eq!(workspace.history.as_ref().map(Vec::len), Some(1));
        assert!(workspace.history_error.is_none());
    }
}
