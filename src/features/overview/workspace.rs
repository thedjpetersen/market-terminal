use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, MouseEvent};
use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{Axis, Block, Borders, Cell, Chart, Dataset, GraphType, Paragraph, Row, Table},
    Frame,
};

use crate::{
    app::{ShellChrome, Workspace, WorkspaceDescriptor},
    ui::{
        is_primary_click,
        theme::{self, AMBER, BG, CYAN, GREEN, INK, MUTED, NAV_BG, YELLOW},
    },
};

use super::{OverviewQuery, ID};

const METRIC_HIGHLIGHT: Color = Color::Rgb(77, 58, 10);

const COUNTRIES: [(&str, &str); 5] = [
    ("United States", "63.2%"),
    ("Japan", "5.2%"),
    ("Taiwan", "3.3%"),
    ("South Korea", "3.1%"),
    ("United Kingdom", "3.0%"),
];

const HOLDINGS: [(&str, &str); 10] = [
    ("NVIDIA Corporation", "4.8%"),
    ("Apple Inc.", "4.3%"),
    ("Microsoft Corporation", "2.6%"),
    ("Amazon.com Inc.", "2.3%"),
    ("Alphabet Inc. Class A", "2.0%"),
    ("Taiwan Semiconductor Manufacturing Co. Ltd.", "1.8%"),
    ("Broadcom Inc.", "1.8%"),
    ("Alphabet Inc. Class C", "1.7%"),
    ("Micron Technology Inc.", "1.3%"),
    ("Meta Platforms Inc Class A", "1.2%"),
];

const WINNERS: [(&str, &str); 5] = [
    ("Advantest Corp.", "+15.06%"),
    ("Kioxia Holdings Corporation", "+12.27%"),
    ("SoftBank Group Corp.", "+7.90%"),
    ("Disco Corporation", "+7.81%"),
    ("Tokyo Electron Ltd.", "+7.78%"),
];

const LOSERS: [(&str, &str); 5] = [
    ("MS&AD Insurance Group Holdings", "−4.45%"),
    ("Komatsu Ltd.", "−3.56%"),
    ("National Australia Bank Ltd.", "−3.35%"),
    ("Mitsubishi Heavy Industries", "−3.34%"),
    ("Mitsui & Co. Ltd.", "−3.26%"),
];

const HEADLINES: [&str; 5] = [
    "European stocks edge higher on oil, tech relief",
    "Moonpig shares jump 10% as FY26 profit beats estimates under new CEO",
    "Japan stocks higher at close of trade; Nikkei 225 up 4.69%",
    "Asia stocks rally as Micron outlook revives AI trade; Korea, Japan lead",
    "Why is Halfords stock surging today?",
];

pub struct OverviewWorkspace {
    query: Arc<dyn OverviewQuery>,
    selected_period: usize,
}

impl OverviewWorkspace {
    pub fn new(query: Arc<dyn OverviewQuery>) -> Self {
        Self { query, selected_period: 3 }
    }

    fn render_context_header(&self, frame: &mut Frame, area: Rect, periods: &[&str]) {
        let rows = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).split(area);
        let mut period_spans = vec![Span::raw(" ")];
        for (index, period) in periods.iter().enumerate() {
            let style = if index == self.selected_period {
                Style::new().bg(CYAN).fg(BG).bold()
            } else {
                Style::new().fg(CYAN)
            };
            period_spans.push(Span::styled(format!("  {period}  "), style));
        }
        frame.render_widget(Paragraph::new(Line::from(period_spans)), rows[0]);

        let context = Layout::horizontal([
            Constraint::Percentage(29),
            Constraint::Percentage(46),
            Constraint::Percentage(25),
        ])
        .split(rows[1]);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" ● MARKET OPEN ", GREEN),
                Span::styled("(Regular session)", MUTED),
            ])),
            context[0],
        );
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("Inflation (YTD): ", INK),
                Span::styled("+0.0% (HICP, as of 2025-12)", AMBER),
            ])),
            context[1],
        );
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("S&P Futures ", INK),
                Span::styled("+0.81%", GREEN),
            ]))
            .alignment(Alignment::Right),
            context[2],
        );
    }

    fn render_returns_chart(
        &self,
        frame: &mut Frame,
        area: Rect,
        primary: &[(f64, f64)],
        comparison: &[(f64, f64)],
    ) {
        let primary_steps = stepped_points(primary);
        let comparison_steps = stepped_points(comparison);
        let zero_baseline = [(0.0, 0.0), (100.0, 0.0)];
        let datasets = vec![
            Dataset::default()
                .marker(symbols::Marker::Braille)
                .graph_type(GraphType::Line)
                .style(YELLOW)
                .data(&primary_steps),
            Dataset::default()
                .marker(symbols::Marker::Braille)
                .graph_type(GraphType::Line)
                .style(CYAN)
                .data(&comparison_steps),
            Dataset::default()
                .marker(symbols::Marker::Dot)
                .graph_type(GraphType::Line)
                .style(MUTED)
                .data(&zero_baseline),
        ];
        let chart = Chart::new(datasets)
            .block(reference_block("Returns — YTD (%)"))
            .x_axis(
                Axis::default()
                    .bounds([0.0, 100.0])
                    .labels(["02 Jan 26", "30 Mar 26", "25 Jun 26"])
                    .style(MUTED),
            )
            .y_axis(
                Axis::default()
                    .bounds([-3.0, 18.0])
                    .labels([
                        "−3.0", "−1.7", "−0.3", "1.0", "2.4", "3.7", "5.0", "6.4",
                        "7.7", "9.1", "10.4", "11.7", "13.1", "14.4", "15.8", "17.1",
                    ])
                    .style(AMBER),
            );
        frame.render_widget(chart, area);

        let legend = Line::from(vec![
            Span::styled(" ━ 001 ", Style::new().fg(YELLOW).bold()),
            Span::styled("H +17.1% L −1.6%    ", MUTED),
            Span::styled("━ 002 ", Style::new().fg(CYAN).bold()),
            Span::styled("H +14.3% L −3.0%", MUTED),
        ]);
        frame.render_widget(
            Paragraph::new(legend),
            Rect::new(
                area.x.saturating_add(2),
                area.y.saturating_add(1),
                area.width.saturating_sub(4),
                1,
            ),
        );
    }

    fn render_risk_band(&self, frame: &mut Frame, area: Rect) {
        let columns = Layout::horizontal([
            Constraint::Percentage(50),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
        ])
        .split(area);
        render_risk_table(frame, columns[0]);
        render_metric_pairs(
            frame,
            columns[1],
            "Asset returns — YTD",
            &[("SPYY", "+13.97%"), ("IS3R", "+30.31%"), ("AVWS", "+22.05%")],
            Some(1),
        );
        render_metric_pairs(
            frame,
            columns[2],
            "Watchlist — YTD",
            &[("AVWC", "+16.72%"), ("DEGC", "+13.15%"), ("DEGT", "+12.75%")],
            Some(0),
        );
    }

    fn render_composition(&self, frame: &mut Frame, area: Rect) {
        let block = reference_block("Current composition · MSCI ACWI (WEBN/SPYY)");
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let columns = Layout::horizontal([
            Constraint::Percentage(26),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(inner);
        render_countries(frame, columns[0]);
        render_holdings(frame, columns[2]);
    }

    fn render_news(&self, frame: &mut Frame, area: Rect) {
        let block = reference_block("News & movers — today");
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let columns = Layout::horizontal([
            Constraint::Percentage(23),
            Constraint::Length(1),
            Constraint::Percentage(23),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(inner);
        render_movers(frame, columns[0], "▲ Winners", &WINNERS);
        render_movers(frame, columns[2], "▼ Losers", &LOSERS);
        render_headlines(frame, columns[4]);
    }

    fn render_function_strip(&self, frame: &mut Frame, area: Rect) {
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" 1 ", AMBER), Span::styled("D  ", INK),
                Span::styled("2 ", AMBER), Span::styled("M  ", INK),
                Span::styled("3 ", AMBER), Span::styled("6M  ", INK),
                Span::styled("4 ", AMBER), Span::styled("YTD  ", INK),
                Span::styled("5 ", AMBER), Span::styled("1Y  ", INK),
                Span::styled("6 ", AMBER), Span::styled("2Y  ", INK),
                Span::styled("7 ", AMBER), Span::styled("5Y  ", INK),
                Span::styled("8 ", AMBER), Span::styled("10Y   ", INK),
                Span::styled("← ", AMBER), Span::styled("◀ Period   ", INK),
                Span::styled("→ ", AMBER), Span::styled("Period ▶   ", INK),
                Span::styled("c ", AMBER), Span::styled("Compare   ", INK),
                Span::styled("r ", AMBER), Span::styled("Refresh   ", INK),
                Span::styled("/ ", AMBER), Span::styled("Command   ", INK),
                Span::styled("q ", AMBER), Span::styled("Quit", INK),
            ]))
            .style(Style::new().bg(NAV_BG)),
            area,
        );
    }
}

impl Workspace for OverviewWorkspace {
    fn descriptor(&self) -> WorkspaceDescriptor {
        WorkspaceDescriptor {
            id: ID,
            label: "OVERVIEW",
            hotkey: 'g',
            commands: &["OVERVIEW", "HOME", "PERF"],
        }
    }

    fn shell_chrome(&self) -> ShellChrome { ShellChrome::Immersive }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        if let KeyCode::Char(character @ '1'..='8') = key.code {
            self.selected_period = character as usize - '1' as usize;
            true
        } else {
            false
        }
    }

    fn handle_mouse(&mut self, event: MouseEvent, area: Rect) -> bool {
        let snapshot = self.query.load_overview();
        let periods_area = Layout::vertical([
            Constraint::Length(2),
            Constraint::Percentage(52),
            Constraint::Length(7),
            Constraint::Min(8),
        ])
        .split(area)[0];
        if !is_primary_click(event, periods_area) {
            return false;
        }
        let mut x = periods_area.x.saturating_add(1);
        for (index, period) in snapshot.periods.iter().enumerate() {
            let width = format!(" {} {} ", index + 1, period).chars().count() as u16;
            if event.column >= x && event.column < x.saturating_add(width) {
                self.selected_period = index;
                return true;
            }
            x = x.saturating_add(width);
        }
        false
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let snapshot = self.query.load_overview();
        let rows = Layout::vertical([
            Constraint::Length(2),
            Constraint::Length(21),
            Constraint::Length(7),
            Constraint::Length(8),
            Constraint::Min(9),
            Constraint::Length(1),
        ])
        .split(area);
        self.render_context_header(frame, rows[0], snapshot.periods);
        self.render_returns_chart(
            frame,
            rows[1],
            snapshot.primary_returns,
            snapshot.comparison_returns,
        );
        self.render_risk_band(frame, rows[2]);
        self.render_composition(frame, rows[3]);
        self.render_news(frame, rows[4]);
        self.render_function_strip(frame, rows[5]);
    }
}

fn reference_block(title: &'static str) -> Block<'static> {
    Block::new()
        .borders(Borders::ALL)
        .border_style(AMBER)
        .title(Span::styled(format!(" {title} "), Style::new().fg(AMBER).bold()))
}

fn stepped_points(points: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let Some(first) = points.first().copied() else { return Vec::new() };
    let mut stepped = Vec::with_capacity(points.len().saturating_mul(2).saturating_sub(1));
    stepped.push(first);
    for window in points.windows(2) {
        let previous = window[0];
        let current = window[1];
        stepped.push((current.0, previous.1));
        stepped.push(current);
    }
    stepped
}

fn render_risk_table(frame: &mut Frame, area: Rect) {
    let header = Row::new(["Portfolio", "Return", "Max DD", "Std dev (ann.)", "Sharpe"])
        .style(Style::new().fg(INK))
        .bottom_margin(1);
    let rows = vec![
        Row::new([
            Cell::from("001").style(Style::new().fg(INK)),
            metric_cell("+17.02%", true),
            metric_cell("−6.3%", true),
            metric_cell("13.2%", false),
            metric_cell("2.79", true),
        ]),
        Row::new([
            Cell::from("002").style(Style::new().fg(INK)),
            metric_cell("+13.87%", false),
            metric_cell("−6.6%", false),
            metric_cell("12.8%", true),
            metric_cell("2.28", false),
        ]),
    ];
    let table = Table::new(
        rows,
        [
            Constraint::Percentage(19),
            Constraint::Percentage(19),
            Constraint::Percentage(18),
            Constraint::Percentage(26),
            Constraint::Percentage(18),
        ],
    )
    .header(header)
    .column_spacing(1)
    .block(reference_block("Risk & return — YTD"));
    frame.render_widget(table, area);
}

fn metric_cell(value: &'static str, highlighted: bool) -> Cell<'static> {
    let mut style = theme::value(value);
    if highlighted {
        style = style.bg(METRIC_HIGHLIGHT);
    }
    Cell::from(Line::from(value).alignment(Alignment::Right)).style(style)
}

fn render_metric_pairs(
    frame: &mut Frame,
    area: Rect,
    title: &'static str,
    values: &[(&'static str, &'static str)],
    highlighted: Option<usize>,
) {
    let rows = values.iter().enumerate().map(|(index, (label, value))| {
        let mut value_style = theme::value(value);
        if highlighted == Some(index) {
            value_style = value_style.bg(METRIC_HIGHLIGHT);
        }
        Row::new([
            Cell::from(*label).style(Style::new().fg(INK)),
            Cell::from(Line::from(*value).alignment(Alignment::Right)).style(value_style),
        ])
    });
    frame.render_widget(
        Table::new(rows, [Constraint::Percentage(62), Constraint::Percentage(38)])
            .block(reference_block(title)),
        area,
    );
}

fn render_countries(frame: &mut Frame, area: Rect) {
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(5)]).split(area);
    frame.render_widget(
        Paragraph::new("Largest country exposure")
            .alignment(Alignment::Center)
            .style(Style::new().fg(INK).add_modifier(Modifier::ITALIC)),
        rows[0],
    );
    frame.render_widget(
        Table::new(
            COUNTRIES.iter().map(|(name, weight)| {
                Row::new([
                    Cell::from(*name),
                    Cell::from(Line::from(*weight).alignment(Alignment::Right)),
                ])
            }),
            [Constraint::Percentage(72), Constraint::Percentage(28)],
        ),
        rows[1],
    );
}

fn render_holdings(frame: &mut Frame, area: Rect) {
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(5)]).split(area);
    frame.render_widget(
        Paragraph::new("Top holdings")
            .alignment(Alignment::Center)
            .style(Style::new().fg(INK).add_modifier(Modifier::ITALIC)),
        rows[0],
    );
    let columns = Layout::horizontal([
        Constraint::Percentage(50),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .split(rows[1]);
    render_holding_half(frame, columns[0], 0);
    render_holding_half(frame, columns[2], 5);
}

fn render_holding_half(frame: &mut Frame, area: Rect, offset: usize) {
    frame.render_widget(
        Table::new(
            HOLDINGS.iter().skip(offset).take(5).enumerate().map(|(index, (name, weight))| {
                Row::new([
                    Cell::from(format!("{}", index + offset + 1)).style(MUTED),
                    Cell::from(*name),
                    Cell::from(Line::from(*weight).alignment(Alignment::Right)),
                ])
            }),
            [Constraint::Length(3), Constraint::Min(16), Constraint::Length(6)],
        )
        .column_spacing(1),
        area,
    );
}

fn render_movers(
    frame: &mut Frame,
    area: Rect,
    title: &'static str,
    values: &[(&'static str, &'static str)],
) {
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(5)]).split(area);
    frame.render_widget(
        Paragraph::new(title)
            .alignment(Alignment::Center)
            .style(Style::new().fg(INK).add_modifier(Modifier::ITALIC)),
        rows[0],
    );
    frame.render_widget(
        Table::new(
            values.iter().map(|(name, change)| {
                Row::new([
                    Cell::from(*name),
                    Cell::from(Line::from(*change).alignment(Alignment::Right))
                        .style(theme::value(change)),
                ])
            }),
            [Constraint::Min(14), Constraint::Length(8)],
        )
        .column_spacing(1),
        rows[1],
    );
}

fn render_headlines(frame: &mut Frame, area: Rect) {
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(5)]).split(area);
    frame.render_widget(
        Paragraph::new("Market headlines")
            .alignment(Alignment::Center)
            .style(Style::new().fg(INK).add_modifier(Modifier::ITALIC)),
        rows[0],
    );
    frame.render_widget(
        Paragraph::new(
            HEADLINES
                .iter()
                .map(|headline| {
                    Line::from(vec![Span::styled(" • ", MUTED), Span::styled(*headline, MUTED)])
                })
                .collect::<Vec<_>>(),
        ),
        rows[1],
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stepped_series_preserves_horizontal_plateaus_and_vertical_moves() {
        assert_eq!(
            stepped_points(&[(0.0, 1.0), (2.0, 3.0), (5.0, 2.0)]),
            [(0.0, 1.0), (2.0, 1.0), (2.0, 3.0), (5.0, 3.0), (5.0, 2.0)]
        );
    }
}
