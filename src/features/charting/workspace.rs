use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Style},
    symbols,
    text::{Line, Span},
    widgets::{Axis, Block, Borders, Chart, Dataset, GraphType, Paragraph, Sparkline, Wrap},
    Frame,
};

use crate::{
    app::{CommandInvocation, Workspace, WorkspaceDescriptor},
    ui::{
        components::terminal_block,
        theme::{AMBER, BG, CYAN, GREEN, INK, MUTED, RED, YELLOW},
    },
};

use super::{
    domain::{percent_change, simple_moving_average}, ChartHistoryQuery, ChartInstrument,
    ChartPeriod, ChartSpecification, HistoryError, HistoryRequest, HistorySeries, Normalization,
    Study, ID,
};

const SERIES_COLORS: [Color; 4] = [CYAN, YELLOW, GREEN, RED];
const DEFAULT_MOVING_AVERAGE: Study = Study::SimpleMovingAverage { window: 20 };

struct PreparedLine {
    name: String,
    points: Vec<(f64, f64)>,
    color: Color,
}

struct PreparedChart {
    lines: Vec<PreparedLine>,
    primary_values: Vec<f64>,
    primary_closes: Vec<f64>,
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

pub struct ChartingWorkspace {
    query: Arc<dyn ChartHistoryQuery>,
    specification: ChartSpecification,
    status: String,
    cursor_offset: usize,
}

impl ChartingWorkspace {
    pub fn new(query: Arc<dyn ChartHistoryQuery>) -> Self {
        Self {
            query,
            specification: ChartSpecification::new(ChartInstrument::from_terminal_subject("AAPL")),
            status: "READY · [/] PERIOD · N NORMALIZE · V VOLUME · S SMA".to_owned(),
            cursor_offset: 0,
        }
    }

    pub fn specification(&self) -> &ChartSpecification { &self.specification }

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

    fn load_chart(&self) -> Result<PreparedChart, HistoryError> {
        let mut instruments = Vec::with_capacity(1 + self.specification.comparisons.len());
        instruments.push(self.specification.primary.clone());
        instruments.extend(self.specification.comparisons.iter().cloned());

        let series = instruments
            .into_iter()
            .map(|instrument| {
                self.query.load_history(&HistoryRequest::new(
                    instrument,
                    self.specification.period,
                ))
            })
            .collect::<Result<Vec<_>, _>>()?;
        prepare_chart(&self.specification, series)
    }

    fn render_header(&self, frame: &mut Frame, area: Rect, chart: &PreparedChart) {
        let change_style = if chart.change_percent >= 0.0 { GREEN } else { RED };
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
                    Style::new().bg(AMBER).fg(BG).bold(),
                ),
                Span::styled(format!(" {:.2}  ", chart.last), Style::new().fg(CYAN).bold()),
                Span::styled(format!("{:+.2}%  ", chart.change_percent), change_style),
                Span::styled(
                    format!(
                        "{} · {}  |  COMPARE {}  |  {}  |  {} · {}",
                        self.specification.period.label(),
                        self.specification.normalization.label(),
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
        let selected_index = chart
            .primary_values
            .len()
            .saturating_sub(1)
            .saturating_sub(self.cursor_offset.min(chart.primary_values.len().saturating_sub(1)));
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
                    // Half-blocks retain twice the vertical resolution of a
                    // character grid without depending on Braille font coverage.
                    .marker(symbols::Marker::HalfBlock)
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

    fn render_statistics(
        &self,
        frame: &mut Frame,
        area: Rect,
        chart: &PreparedChart,
        selected_index: usize,
    ) {
        let selected_value = chart.primary_values.get(selected_index).copied().unwrap_or_default();
        let selected_close = chart.primary_closes.get(selected_index).copied().unwrap_or_default();
        let observation = selected_index + 1;
        let total = chart.primary_values.len();
        let mut lines = vec![
            Line::from(Span::styled("INSPECTION", AMBER)),
            Line::from(Span::styled(format!("OBS  {observation}/{total}"), MUTED)),
            Line::from(Span::styled(format!("PX   {selected_close:.2}"), CYAN)),
            Line::from(Span::styled(format!("PLOT {selected_value:+.2}"), INK)),
            Line::from(""),
            Line::from(Span::styled("RANGE", AMBER)),
            Line::from(Span::styled(format!("HIGH {:.2}", chart.session_high), GREEN)),
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
                    value.map(|value| format!("{value:+.2}"))
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

    fn is_favorite(&self) -> bool { true }

    fn handle_command(&mut self, invocation: &CommandInvocation) -> bool {
        apply_chart_command(&mut self.specification, &invocation.args, &mut self.status);
        self.cursor_offset = 0;
        true
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Right | KeyCode::Char(']') => {
                self.specification.period = self.specification.period.next();
                self.cursor_offset = 0;
                self.status = format!("PERIOD · {}", self.specification.period.label());
                true
            }
            KeyCode::Left | KeyCode::Char('[') => {
                self.specification.period = self.specification.period.previous();
                self.cursor_offset = 0;
                self.status = format!("PERIOD · {}", self.specification.period.label());
                true
            }
            KeyCode::Char('n') => {
                self.specification.normalization = self.specification.normalization.toggled();
                self.status = format!("MODE · {}", self.specification.normalization.label());
                true
            }
            KeyCode::Char('v') => {
                let _ = self.specification.toggle_study(Study::Volume);
                self.status = "VOLUME TOGGLED".to_owned();
                true
            }
            KeyCode::Char('s') => {
                let _ = self.specification.toggle_study(DEFAULT_MOVING_AVERAGE);
                self.status = "SMA 20 TOGGLED".to_owned();
                true
            }
            KeyCode::Char('c') => {
                self.toggle_default_comparison();
                true
            }
            KeyCode::Char('x') => {
                self.specification.comparisons.clear();
                self.status = "COMPARISONS CLEARED".to_owned();
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
            KeyCode::Char('e') => {
                self.cursor_offset = 0;
                self.status = "INSPECT · LATEST OBSERVATION".to_owned();
                true
            }
            _ => false,
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let sections = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(3),
        ])
        .split(area);

        match self.load_chart() {
            Ok(chart) => {
                self.render_header(frame, sections[0], &chart);
                if self.specification.has_study(Study::Volume) {
                    let plots = Layout::vertical([
                        Constraint::Percentage(72),
                        Constraint::Percentage(28),
                    ])
                    .split(sections[1]);
                    self.render_price_chart(frame, plots[0], &chart);
                    self.render_volume_chart(frame, plots[1], &chart);
                } else {
                    self.render_price_chart(frame, sections[1], &chart);
                }
            }
            Err(error) => {
                frame.render_widget(
                    Paragraph::new(Line::from(vec![
                        Span::styled(
                            format!(" {}  ", self.specification.primary.symbol),
                            Style::new().bg(AMBER).fg(BG).bold(),
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
                Span::styled(" V ", AMBER),
                Span::styled("VOLUME  ", MUTED),
                Span::styled(" S ", AMBER),
                Span::styled("SMA  ", MUTED),
                Span::styled(" C ", AMBER),
                Span::styled("COMPARE SPY  ", MUTED),
                Span::styled(" ,/. ", AMBER),
                Span::styled("INSPECT  ", MUTED),
                Span::styled(" E ", AMBER),
                Span::styled("LATEST", MUTED),
            ]))
            .style(Style::new().fg(INK)),
            sections[2],
        );
    }
}

fn prepare_chart(
    specification: &ChartSpecification,
    series: Vec<HistorySeries>,
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

    let first_close = primary.bars.first().map(|bar| bar.close).unwrap_or_default();
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
                color: SERIES_COLORS[index % SERIES_COLORS.len()],
            }
        })
        .collect::<Vec<_>>();

    for study in &specification.studies {
        let Study::SimpleMovingAverage { window } = *study else { continue };
        let closes = primary.bars.iter().map(|bar| bar.close).collect::<Vec<_>>();
        let moving_average = simple_moving_average(&closes, window);
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
        lines.push(PreparedLine {
            name: format!("SMA {window}"),
            points,
            color: AMBER,
        });
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
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(minimum, maximum), value| {
            (minimum.min(value), maximum.max(value))
        });
    let y_bounds = padded_bounds(minimum, maximum);
    let primary_values = lines
        .first()
        .map(|line| line.points.iter().map(|point| point.1).collect())
        .unwrap_or_default();
    let volume_max = volume_bars
        .iter()
        .copied()
        .max()
        .unwrap_or(1)
        .max(1);
    let average_volume = if volume_bars.is_empty() {
        0
    } else {
        (volume_bars.iter().map(|value| u128::from(*value)).sum::<u128>()
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
        primary_values,
        primary_closes,
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
                    for symbol in args[index].split(',').filter(|symbol| !symbol.trim().is_empty()) {
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
                if let Some(period) = args.get(index + 1).and_then(|value| ChartPeriod::parse(value)) {
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
                if let Err(error) = specification.toggle_study(Study::SimpleMovingAverage { window }) {
                    *status = format!("CHART ERROR · {error}");
                }
                index += usize::from(parsed_window.is_some()) + 1;
            }
            _ => {
                if let Some(period) = ChartPeriod::parse(&token) {
                    specification.period = period;
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
            | "VOLUME"
            | "SMA"
            | "STUDY"
    ) || ChartPeriod::parse(&upper).is_some()
        || parse_sma_window(&upper).is_some()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::charting::{HistoryQuality, PriceBar};
    use crossterm::event::KeyModifiers;

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
            args: ["MSFT", "COMPARE", "SPY,QQQ", "6M", "SMA50"]
                .into_iter()
                .map(ToOwned::to_owned)
                .collect(),
        });

        assert_eq!(workspace.specification.primary.symbol, "MSFT");
        assert_eq!(workspace.specification.period, ChartPeriod::SixMonths);
        assert_eq!(workspace.specification.normalization, Normalization::PercentChange);
        assert_eq!(workspace.specification.comparisons.len(), 2);
        assert!(workspace
            .specification
            .has_study(Study::SimpleMovingAverage { window: 50 }));
    }

    #[test]
    fn keyboard_controls_change_only_chart_specification() {
        let mut workspace = ChartingWorkspace::new(Arc::new(StubHistory));
        let initial_period = workspace.specification.period;

        assert!(workspace.handle_key(KeyEvent::new(KeyCode::Char(']'), KeyModifiers::NONE)));
        assert_eq!(workspace.specification.period, initial_period.next());
        assert!(workspace.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE)));
        assert_eq!(workspace.specification.normalization, Normalization::PercentChange);
        assert!(workspace.handle_key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE)));
        assert!(!workspace.specification.has_study(Study::Volume));
    }

    #[test]
    fn inspection_cursor_moves_back_and_returns_to_latest() {
        let mut workspace = ChartingWorkspace::new(Arc::new(StubHistory));
        assert!(workspace.handle_key(KeyEvent::new(KeyCode::Char(','), KeyModifiers::NONE)));
        assert!(workspace.handle_key(KeyEvent::new(KeyCode::Char(','), KeyModifiers::NONE)));
        assert_eq!(workspace.cursor_offset, 2);
        assert!(workspace.handle_key(KeyEvent::new(KeyCode::Char('.'), KeyModifiers::NONE)));
        assert_eq!(workspace.cursor_offset, 1);
        assert!(workspace.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE)));
        assert_eq!(workspace.cursor_offset, 0);
    }

    #[test]
    fn normalized_comparisons_share_a_zero_origin() {
        let mut spec = ChartSpecification::new(ChartInstrument::from_terminal_subject("AAPL"));
        spec.normalization = Normalization::PercentChange;
        spec.add_comparison(ChartInstrument::from_terminal_subject("SPY"))
            .unwrap();
        let query = StubHistory;
        let series = [&spec.primary, &spec.comparisons[0]]
            .into_iter()
            .map(|instrument| {
                query
                    .load_history(&HistoryRequest::new(instrument.clone(), spec.period))
                    .unwrap()
            })
            .collect();

        let chart = prepare_chart(&spec, series).unwrap();
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
}
