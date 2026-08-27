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
    app::{AppIntent, CommandInvocation, Workspace, WorkspaceDescriptor},
    ui::{
        components::terminal_block,
        contains, is_primary_click,
        theme::{ThemeColor, AMBER, BG, CYAN, GREEN, INK, MUTED, RED, YELLOW},
    },
};

use super::{
    domain::percent_change,
    indicators::{ema, rsi, sma, MOVING_AVERAGE_FAST, MOVING_AVERAGE_SLOW, RSI_PERIOD},
    ChartHistoryQuery, ChartInstrument, ChartPeriod, ChartSpecification, HistoryError,
    HistoryRequest, HistorySeries, Normalization, Study, ID,
};

const SERIES_COLORS: [ThemeColor; 4] = [CYAN, YELLOW, GREEN, RED];
const CANDLE_RIGHT_MARGIN_PERCENT: u16 = 18;

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
    x_max: f64,
    y_bounds: [f64; 2],
    volume_max: u64,
    session_high: f64,
    session_low: f64,
    average_volume: u64,
    last: f64,
    change_percent: f64,
    quality: &'static str,
    source: String,
}

#[derive(Clone, Copy)]
struct PlotAreas {
    price: Rect,
    rsi: Option<Rect>,
    volume: Option<Rect>,
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
    display_mode: ChartDisplayMode,
    line_mode: ChartLineMode,
    refresh_sender: SyncSender<ChartRefresh>,
    refresh_receiver: Receiver<ChartRefreshResult>,
    pending_refresh: Option<ChartRefresh>,
    desired_generation: u64,
    history: Option<Vec<HistorySeries>>,
    history_error: Option<HistoryError>,
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
            status: "READY · K CANDLES/LINE · [/] PERIOD · M MA · E SMA/EMA · I RSI · V VOLUME"
                .to_owned(),
            cursor_offset: 0,
            display_mode: ChartDisplayMode::Candlesticks,
            line_mode: ChartLineMode::Smooth,
            refresh_sender,
            refresh_receiver,
            pending_refresh: None,
            desired_generation: 0,
            history: None,
            history_error: None,
        };
        workspace.queue_history();
        workspace
    }

    pub fn specification(&self) -> &ChartSpecification {
        &self.specification
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

    fn render_price_chart(&self, frame: &mut Frame, area: Rect, chart: &PreparedChart) {
        let columns = if area.width >= 110 {
            Layout::horizontal([Constraint::Percentage(78), Constraint::Percentage(22)]).split(area)
        } else {
            Layout::horizontal([Constraint::Percentage(100), Constraint::Length(0)]).split(area)
        };
        let selected_index = chart.primary_values.len().saturating_sub(1).saturating_sub(
            self.cursor_offset
                .min(chart.primary_values.len().saturating_sub(1)),
        );
        if self.effective_display_mode() == ChartDisplayMode::Candlesticks {
            render_candlesticks(frame, columns[0], chart, selected_index);
        } else {
            let selected_x = selected_index as f64;
            let cursor = [
                (selected_x, chart.y_bounds[0]),
                (selected_x, chart.y_bounds[1]),
            ];
            let zero_baseline = [(0.0, 0.0), (chart.x_max, 0.0)];
            let mut datasets = chart
                .lines
                .iter()
                .map(|line| {
                    Dataset::default()
                        .name(line.name.clone())
                        .marker(self.line_mode.marker())
                        .graph_type(GraphType::Line)
                        .style(line.color)
                        .data(&line.points)
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
            let middle = (chart.y_bounds[0] + chart.y_bounds[1]) / 2.0;
            let lower_middle = (chart.y_bounds[0] + middle) / 2.0;
            let upper_middle = (middle + chart.y_bounds[1]) / 2.0;
            let y_labels = [
                format!("{:.2}", chart.y_bounds[0]),
                format!("{lower_middle:.2}"),
                format!("{middle:.2}"),
                format!("{upper_middle:.2}"),
                format!("{:.2}", chart.y_bounds[1]),
            ];
            let axis_title = match self.specification.normalization {
                Normalization::Price => "PRICE",
                Normalization::PercentChange => "% CHANGE",
            };
            let price_chart = Chart::new(datasets)
                .block(terminal_block("GRAPH", "PRICE AND COMPARISON"))
                .x_axis(
                    Axis::default()
                        .bounds([0.0, chart.x_max])
                        .labels(["START", self.specification.period.label(), "LATEST"])
                        .style(MUTED),
                )
                .y_axis(
                    Axis::default()
                        .title(axis_title)
                        .bounds(chart.y_bounds)
                        .labels(y_labels)
                        .style(AMBER),
                );
            frame.render_widget(price_chart, columns[0]);
        }

        if columns[1].width > 0 {
            self.render_statistics(frame, columns[1], chart, selected_index);
        }
    }

    fn render_volume_chart(&self, frame: &mut Frame, area: Rect, chart: &PreparedChart) {
        let volume = Sparkline::default()
            .data(&chart.volume_bars)
            .max(chart.volume_max)
            .style(AMBER)
            .block(terminal_block("VOL", "VOLUME HISTOGRAM"));
        frame.render_widget(volume, area);
    }

    fn render_rsi_chart(&self, frame: &mut Frame, area: Rect, chart: &PreparedChart) {
        let lower_threshold = [(0.0, 30.0), (chart.x_max, 30.0)];
        let upper_threshold = [(0.0, 70.0), (chart.x_max, 70.0)];
        let selected_index = chart.primary_values.len().saturating_sub(1).saturating_sub(
            self.cursor_offset
                .min(chart.primary_values.len().saturating_sub(1)),
        );
        let selected_x = selected_index as f64;
        let cursor = [(selected_x, 0.0), (selected_x, 100.0)];
        let mut datasets = chart
            .rsi_lines
            .iter()
            .map(|line| {
                Dataset::default()
                    .name(line.name.clone())
                    .marker(self.line_mode.marker())
                    .graph_type(GraphType::Line)
                    .style(line.color)
                    .data(&line.points)
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
                .x_axis(Axis::default().bounds([0.0, chart.x_max]).style(MUTED))
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
        selected_index: usize,
    ) {
        let selected_value = chart
            .primary_values
            .get(selected_index)
            .copied()
            .unwrap_or_default();
        let selected_close = chart
            .primary_closes
            .get(selected_index)
            .copied()
            .unwrap_or_default();
        let observation = selected_index + 1;
        let total = chart.primary_values.len();
        let mut lines = vec![
            Line::from(Span::styled("INSPECTION", AMBER)),
            Line::from(Span::styled(format!("OBS  {observation}/{total}"), MUTED)),
            Line::from(Span::styled(format!("PX   {selected_close:.2}"), CYAN)),
            Line::from(Span::styled(format!("PLOT {selected_value:+.2}"), INK)),
            Line::from(""),
            Line::from(Span::styled("RANGE", AMBER)),
            Line::from(Span::styled(
                format!("HIGH {:.2}", chart.session_high),
                GREEN,
            )),
            Line::from(Span::styled(format!("LOW  {:.2}", chart.session_low), RED)),
            Line::from(Span::styled(
                format!("SPAN {:.2}", chart.session_high - chart.session_low),
                MUTED,
            )),
            Line::from(Span::styled(
                format!(
                    "VOL  {}",
                    compact_volume(*chart.volume_bars.get(selected_index).unwrap_or(&0))
                ),
                AMBER,
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
        self.cursor_offset = 0;
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
            KeyCode::Right | KeyCode::Char(']') | KeyCode::Char('t') => {
                self.specification.period = self.specification.period.next();
                self.cursor_offset = 0;
                self.queue_history();
                true
            }
            KeyCode::Left | KeyCode::Char('[') | KeyCode::Char('T') => {
                self.specification.period = self.specification.period.previous();
                self.cursor_offset = 0;
                self.queue_history();
                true
            }
            KeyCode::Char('n') => {
                self.specification.normalization = self.specification.normalization.toggled();
                self.status = format!("MODE · {}", self.specification.normalization.label());
                true
            }
            KeyCode::Char('v') | KeyCode::Char('b') => {
                let _ = self.specification.toggle_study(Study::Volume);
                self.status = "VOLUME TOGGLED".to_owned();
                true
            }
            KeyCode::Char('s') | KeyCode::Char('m') => {
                self.toggle_default_moving_averages();
                true
            }
            KeyCode::Char('e') => {
                self.toggle_moving_average_kind();
                true
            }
            KeyCode::Char('i') => {
                let _ = self
                    .specification
                    .toggle_study(Study::RelativeStrengthIndex { period: RSI_PERIOD });
                self.status = "RSI 14 TOGGLED".to_owned();
                true
            }
            KeyCode::Char('c') => {
                self.toggle_default_comparison();
                self.queue_history();
                true
            }
            KeyCode::Char('x') => {
                self.specification.comparisons.clear();
                self.queue_history();
                true
            }
            KeyCode::Char(',') => {
                self.cursor_offset = self.cursor_offset.saturating_add(1).min(10_000);
                self.status = format!("INSPECT · {} OBSERVATION(S) BACK", self.cursor_offset);
                true
            }
            KeyCode::Char('.') => {
                self.cursor_offset = self.cursor_offset.saturating_sub(1);
                self.status = format!("INSPECT · {} OBSERVATION(S) BACK", self.cursor_offset);
                true
            }
            KeyCode::Home | KeyCode::Char('E') => {
                self.cursor_offset = 0;
                self.status = "INSPECT · LATEST OBSERVATION".to_owned();
                true
            }
            KeyCode::Char('l') => {
                self.line_mode = self.line_mode.toggled();
                self.status = format!("LINE MODE · {}", self.line_mode.label());
                true
            }
            KeyCode::Char('k') => {
                self.toggle_display_mode();
                true
            }
            KeyCode::F(9) => {
                self.queue_history();
                true
            }
            _ => false,
        }
    }

    fn handle_mouse(&mut self, event: MouseEvent, area: Rect) -> bool {
        let sections = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(3),
        ])
        .split(area);
        if is_primary_click(event, sections[0]) {
            return self.handle_key(KeyEvent::new(KeyCode::F(9), KeyModifiers::NONE));
        }
        if contains(sections[1], event.column, event.row) {
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
        if is_primary_click(event, sections[2]) {
            let controls = [
                (" [/] PERIOD  ", KeyCode::Right),
                (" N NORMALIZE  ", KeyCode::Char('n')),
                (" M MA  ", KeyCode::Char('m')),
                (" E SMA/EMA  ", KeyCode::Char('e')),
                (" I RSI  ", KeyCode::Char('i')),
                (" B/V VOLUME  ", KeyCode::Char('v')),
                (" C COMPARE SPY  ", KeyCode::Char('c')),
                (" ,/. INSPECT  ", KeyCode::Char(',')),
                (" HOME LATEST  ", KeyCode::Home),
                (" K CANDLES/LINE  ", KeyCode::Char('k')),
                (" L LINE MODE", KeyCode::Char('l')),
            ];
            let mut x = sections[2].x;
            for (label, key) in controls {
                let width = label.chars().count() as u16;
                if event.column >= x && event.column < x.saturating_add(width) {
                    return self.handle_key(KeyEvent::new(key, KeyModifiers::NONE));
                }
                x = x.saturating_add(width);
            }
            return true;
        }
        if !is_primary_click(event, sections[1]) {
            return false;
        }
        let Ok(chart) = self.prepared_chart() else {
            return true;
        };
        if chart.primary_values.is_empty() {
            return true;
        }
        let areas = plot_areas(
            sections[1],
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
        let last = chart.primary_values.len() - 1;
        let selected = usize::from(relative) * last / usize::from(plot_width);
        self.cursor_offset = last.saturating_sub(selected);
        self.status = format!("INSPECT · {} OBSERVATION(S) BACK", self.cursor_offset);
        true
    }

    fn poll_intents(&mut self) -> Vec<AppIntent> {
        self.poll_history();
        Vec::new()
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let sections = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(3),
        ])
        .split(area);

        match self.prepared_chart() {
            Ok(chart) => {
                self.render_header(frame, sections[0], &chart);
                let areas = plot_areas(
                    sections[1],
                    !chart.rsi_lines.is_empty(),
                    self.specification.has_study(Study::Volume),
                );
                self.render_price_chart(frame, areas.price, &chart);
                if let Some(rsi_area) = areas.rsi {
                    self.render_rsi_chart(frame, rsi_area, &chart);
                }
                if let Some(volume_area) = areas.volume {
                    self.render_volume_chart(frame, volume_area, &chart);
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
                    sections[0],
                );
                self.render_error(frame, sections[1], &error);
            }
        }

        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" [/] ", AMBER),
                Span::styled("PERIOD  ", MUTED),
                Span::styled(" N ", AMBER),
                Span::styled("NORMALIZE  ", MUTED),
                Span::styled(" M ", AMBER),
                Span::styled("MA  ", MUTED),
                Span::styled(" E ", AMBER),
                Span::styled("SMA/EMA  ", MUTED),
                Span::styled(" I ", AMBER),
                Span::styled("RSI  ", MUTED),
                Span::styled(" B/V ", AMBER),
                Span::styled("VOLUME  ", MUTED),
                Span::styled(" C ", AMBER),
                Span::styled("COMPARE SPY  ", MUTED),
                Span::styled(" ,/. ", AMBER),
                Span::styled("INSPECT  ", MUTED),
                Span::styled(" HOME ", AMBER),
                Span::styled("LATEST  ", MUTED),
                Span::styled(" K ", AMBER),
                Span::styled("CANDLES/LINE  ", MUTED),
                Span::styled(" L ", AMBER),
                Span::styled("LINE MODE  ", MUTED),
                Span::styled(" F9/CLICK HEADER ", AMBER),
                Span::styled("REFRESH", MUTED),
            ]))
            .style(Style::new().fg(INK.into())),
            sections[2],
        );
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

    let x_max = lines
        .iter()
        .filter_map(|line| line.points.last().map(|point| point.0))
        .fold(1.0_f64, f64::max)
        .max(1.0);
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
    let volume_max = volume_bars.iter().copied().max().unwrap_or(1).max(1);
    let average_volume = if volume_bars.is_empty() {
        0
    } else {
        (volume_bars
            .iter()
            .map(|value| u128::from(*value))
            .sum::<u128>()
            / volume_bars.len() as u128) as u64
    };
    let session_high = primary
        .bars
        .iter()
        .map(|bar| bar.high)
        .fold(f64::NEG_INFINITY, f64::max);
    let session_low = primary
        .bars
        .iter()
        .map(|bar| bar.low)
        .fold(f64::INFINITY, f64::min);

    Ok(PreparedChart {
        lines,
        rsi_lines,
        primary_values,
        primary_closes,
        primary_bars,
        volume_bars,
        x_max,
        y_bounds,
        volume_max,
        session_high,
        session_low,
        average_volume,
        last,
        change_percent,
        quality: primary.quality.label(),
        source: primary.source.clone(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Candle {
    timestamp: i64,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
}

fn render_candlesticks(
    frame: &mut Frame,
    area: Rect,
    chart: &PreparedChart,
    selected_index: usize,
) {
    let block = terminal_block("OHLC", "CANDLESTICKS · LAST BAR MARKER");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if chart.primary_bars.is_empty() || inner.width < 12 || inner.height < 4 {
        return;
    }

    let candles = chart
        .primary_bars
        .iter()
        .map(|bar| Candle {
            timestamp: bar.timestamp,
            open: bar.open,
            high: bar.high,
            low: bar.low,
            close: bar.close,
        })
        .collect::<Vec<_>>();
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
        .position(|range| range.contains(&selected_index))
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
        let row = plot.y
            + (candle_scale(chart.last, y_low, y_high, usize::from(plot.height) * 2) / 2) as u16;
        for column in plot.x + usable..plot.x + plot.width {
            if let Some(cell) = buffer.cell_mut((column, row)) {
                cell.set_char('─').set_fg(CYAN.into());
            }
        }
        let tag = format!("{:.2}", chart.last);
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
    use crossterm::event::KeyModifiers;
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
        let mut terminal = Terminal::new(TestBackend::new(70, 14)).unwrap();

        terminal
            .draw(|frame| render_candlesticks(frame, frame.area(), &chart, 1))
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
        tiny.draw(|frame| render_candlesticks(frame, frame.area(), &chart, 1))
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
