use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, MouseEvent};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Cell, Paragraph, Row, Table, Wrap},
    Frame,
};

use crate::{
    app::{AppIntent, CommandInvocation, Workspace, WorkspaceDescriptor},
    ui::{
        is_primary_click, scroll_key, table_row_at, table_viewport,
        theme::{AMBER, BG, CYAN, GREEN, INK, MUTED, RED, YELLOW},
    },
};

use super::{domain, historical, HistoricalRiskSnapshot, RiskQuery, RiskSnapshot, ID};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RiskView {
    Concentration,
    Historical,
}

impl RiskView {
    fn label(self) -> &'static str {
        match self {
            Self::Concentration => "CONCENTRATION",
            Self::Historical => "HISTORICAL",
        }
    }
}

pub struct RiskWorkspace {
    query: Arc<dyn RiskQuery>,
    view: RiskView,
    selected: usize,
    status: String,
    pending_intents: Vec<AppIntent>,
}

impl RiskWorkspace {
    pub fn new(query: Arc<dyn RiskQuery>) -> Self {
        Self {
            query,
            view: RiskView::Concentration,
            selected: 0,
            status: "VERSIONED PORTFOLIO RISK".to_owned(),
            pending_intents: Vec::new(),
        }
    }

    fn move_selection(&mut self, delta: isize) {
        let count = self
            .query
            .load_risk()
            .map(|snapshot| snapshot.positions.len())
            .unwrap_or_default();
        if count == 0 {
            self.selected = 0;
        } else {
            self.selected = self.selected.saturating_add_signed(delta).min(count - 1);
        }
    }

    fn open_selected(&mut self) -> bool {
        let Ok(snapshot) = self.query.load_risk() else {
            return false;
        };
        let Some(position) = snapshot.positions.get(self.selected) else {
            return false;
        };
        if position.cash {
            self.status = format!("{} IS CASH · NO SECURITY RESEARCH", position.currency);
            return true;
        }
        self.pending_intents.push(AppIntent::DispatchCommand {
            command: format!("SEC {}", position.symbol),
            origin: ID,
        });
        self.status = format!("OPENING {} SECURITY RESEARCH", position.symbol);
        true
    }

    fn refresh_status(&mut self) {
        self.status = match self.query.load_risk() {
            Ok(snapshot) => format!(
                "RECOMPUTED {} POSITIONS · {}",
                snapshot.positions.len(),
                snapshot.input_version
            ),
            Err(error) => error.to_string(),
        };
    }
}

impl Workspace for RiskWorkspace {
    fn descriptor(&self) -> WorkspaceDescriptor {
        WorkspaceDescriptor {
            id: ID,
            label: "RISK",
            hotkey: '\0',
            commands: &["RISK"],
        }
    }

    fn is_favorite(&self) -> bool {
        true
    }

    fn handle_command(&mut self, invocation: &CommandInvocation) -> bool {
        if invocation.args.len() > 1
            || invocation.args.first().is_some_and(|argument| {
                !argument.eq_ignore_ascii_case("HISTORY")
                    && !argument.eq_ignore_ascii_case("CONCENTRATION")
            })
        {
            self.status = "USAGE: RISK [HISTORY|CONCENTRATION] · UNSUPPORTED ARGUMENTS".to_owned();
            return true;
        }
        if invocation
            .args
            .first()
            .is_some_and(|argument| argument.eq_ignore_ascii_case("HISTORY"))
        {
            self.view = RiskView::Historical;
        } else if invocation
            .args
            .first()
            .is_some_and(|argument| argument.eq_ignore_ascii_case("CONCENTRATION"))
        {
            self.view = RiskView::Concentration;
        }
        self.refresh_status();
        true
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Tab => {
                self.view = match self.view {
                    RiskView::Concentration => RiskView::Historical,
                    RiskView::Historical => RiskView::Concentration,
                };
                true
            }
            KeyCode::Char('1') => {
                self.view = RiskView::Concentration;
                true
            }
            KeyCode::Char('2') => {
                self.view = RiskView::Historical;
                true
            }
            KeyCode::Up | KeyCode::Char('k') if self.view == RiskView::Concentration => {
                self.move_selection(-1);
                true
            }
            KeyCode::Down | KeyCode::Char('j') if self.view == RiskView::Concentration => {
                self.move_selection(1);
                true
            }
            KeyCode::Enter | KeyCode::Char('s') if self.view == RiskView::Concentration => {
                self.open_selected()
            }
            KeyCode::Char('r') => {
                self.refresh_status();
                true
            }
            _ => false,
        }
    }

    fn handle_mouse(&mut self, event: MouseEvent, area: Rect) -> bool {
        let areas = risk_layout(area);
        if is_primary_click(event, areas.tabs) {
            self.view = if event.column < areas.tabs.x.saturating_add(20) {
                RiskView::Concentration
            } else {
                RiskView::Historical
            };
            return true;
        }
        if is_primary_click(event, areas.header) {
            self.refresh_status();
            return true;
        }
        if self.view == RiskView::Concentration {
            let Ok(snapshot) = self.query.load_risk() else {
                return false;
            };
            let visible = table_viewport(areas.table, snapshot.positions.len(), self.selected);
            if let Some(index) = table_row_at(event, areas.table, visible.len()) {
                self.selected = visible.start + index;
                return self.open_selected();
            }
        }
        if is_primary_click(event, areas.side) {
            self.status = "RISK RESULTS ARE PER-CURRENCY · NO INVENTED FX CONVERSION".to_owned();
            return true;
        }
        if is_primary_click(event, areas.footer) {
            let controls = if self.view == RiskView::Concentration {
                [
                    (" ↑↓/JK SELECT  ", None),
                    ("ENTER/S SECURITY  ", Some(KeyCode::Enter)),
                    ("R RECOMPUTE  ", Some(KeyCode::Char('r'))),
                ]
            } else {
                [
                    (" 1/2/TAB VIEW  ", None),
                    ("R RECOMPUTE  ", Some(KeyCode::Char('r'))),
                    (" ", None),
                ]
            };
            let mut x = areas.footer.x;
            for (label, key) in controls {
                let width = label.chars().count() as u16;
                if event.column >= x && event.column < x.saturating_add(width) {
                    return key.is_none_or(|key| {
                        self.handle_key(KeyEvent::new(key, crossterm::event::KeyModifiers::NONE))
                    });
                }
                x = x.saturating_add(width);
            }
            return true;
        }
        if let Some(key) = scroll_key(event, areas.table) {
            return self.handle_key(key);
        }
        false
    }

    fn poll_intents(&mut self) -> Vec<AppIntent> {
        std::mem::take(&mut self.pending_intents)
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        match self.query.load_risk() {
            Ok(snapshot) => self.render_snapshot(frame, area, &snapshot),
            Err(error) => frame.render_widget(
                Paragraph::new(vec![
                    Line::styled("RISK UNAVAILABLE", RED),
                    Line::raw(""),
                    Line::styled(error.to_string(), YELLOW),
                    Line::raw(""),
                    Line::styled("IMPORT A PORTFOLIO WITH PORT IMPORT <FILE.CSV>", CYAN),
                ])
                .wrap(Wrap { trim: true })
                .block(crate::ui::components::terminal_block(
                    "RISK",
                    "VERSIONED PORTFOLIO INPUT REQUIRED",
                )),
                area,
            ),
        }
    }
}

impl RiskWorkspace {
    fn render_snapshot(&self, frame: &mut Frame, area: Rect, snapshot: &RiskSnapshot) {
        let areas = risk_layout(area);
        render_tabs(frame, areas.tabs, self.view);
        match self.view {
            RiskView::Concentration => {
                render_header(frame, areas.header, snapshot, &self.status);
                render_positions(frame, areas.table, snapshot, self.selected);
                render_provenance(frame, areas.side, snapshot);
            }
            RiskView::Historical => {
                render_historical_header(
                    frame,
                    areas.header,
                    snapshot.historical.as_ref(),
                    &self.status,
                );
                render_historical(frame, areas.table, snapshot.historical.as_ref());
                render_historical_provenance(frame, areas.side, snapshot.historical.as_ref());
            }
        }
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(vec![
                    Span::styled(" 1/2/TAB ", AMBER),
                    Span::styled("VIEW  ", MUTED),
                    Span::styled(
                        if self.view == RiskView::Concentration {
                            " ↑↓/JK "
                        } else {
                            " "
                        },
                        AMBER,
                    ),
                    Span::styled(
                        if self.view == RiskView::Concentration {
                            "SELECT  ENTER/S SECURITY  "
                        } else {
                            ""
                        },
                        MUTED,
                    ),
                    Span::styled("R ", AMBER),
                    Span::styled("RECOMPUTE  ", MUTED),
                    Span::styled("PER-CURRENCY · INPUTS AND METHODS EXPLICIT", YELLOW),
                ]),
                Line::styled(self.status.clone(), YELLOW),
            ]),
            areas.footer,
        );
    }
}

#[derive(Debug, Clone, Copy)]
struct RiskLayout {
    header: Rect,
    tabs: Rect,
    table: Rect,
    side: Rect,
    footer: Rect,
}

fn risk_layout(area: Rect) -> RiskLayout {
    let rows = Layout::vertical([
        Constraint::Length(4),
        Constraint::Length(1),
        Constraint::Min(10),
        Constraint::Length(2),
    ])
    .split(area);
    let body =
        Layout::horizontal([Constraint::Percentage(74), Constraint::Percentage(26)]).split(rows[2]);
    RiskLayout {
        header: rows[0],
        tabs: rows[1],
        table: body[0],
        side: body[1],
        footer: rows[3],
    }
}

fn render_tabs(frame: &mut Frame, area: Rect, active: RiskView) {
    let spans = [RiskView::Concentration, RiskView::Historical]
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

fn render_header(frame: &mut Frame, area: Rect, snapshot: &RiskSnapshot, status: &str) {
    let kpis = Layout::horizontal([Constraint::Ratio(1, 4); 4]).split(area);
    let values = [
        (
            "PRICED / UNPRICED",
            format!(
                "{} / {}",
                snapshot.priced_position_count(),
                snapshot.unpriced_position_count()
            ),
        ),
        ("CURRENCIES", snapshot.currencies.len().to_string()),
        ("LARGEST NON-CASH", snapshot.largest_position_label()),
        ("NON-CASH -10%", snapshot.scenario_label()),
    ];
    for (index, (label, value)) in values.into_iter().enumerate() {
        frame.render_widget(
            Paragraph::new(vec![
                Line::styled(label, MUTED),
                Line::styled(value, if index == 3 { RED } else { INK }),
                Line::styled(status, YELLOW),
            ])
            .block(crate::ui::components::terminal_block("RISK", label)),
            kpis[index],
        );
    }
}

fn render_positions(frame: &mut Frame, area: Rect, snapshot: &RiskSnapshot, selected: usize) {
    let header = Row::new([
        "ACCOUNT · SYMBOL · CCY",
        "MARKET VALUE",
        "CCY WEIGHT",
        "-10% CHANGE",
        "QUALITY",
    ])
    .style(Style::new().fg(AMBER.into()).bold())
    .bottom_margin(1);
    let rows = snapshot
        .positions
        .iter()
        .enumerate()
        .skip(table_viewport(area, snapshot.positions.len(), selected).start)
        .take(table_viewport(area, snapshot.positions.len(), selected).len())
        .map(|(index, position)| {
            let values = [
                format!(
                    "{} · {} · {}",
                    position.account, position.symbol, position.currency
                ),
                position
                    .market_value
                    .map(domain::format_money)
                    .unwrap_or_else(|| "UNPRICED".to_owned()),
                position
                    .currency_weight_bps
                    .map(domain::format_bps)
                    .unwrap_or_else(|| "—".to_owned()),
                position
                    .scenario_change
                    .map(domain::format_money)
                    .unwrap_or_else(|| "—".to_owned()),
                if position.market_value.is_some() {
                    "PRICED"
                } else {
                    "MISSING"
                }
                .to_owned(),
            ];
            Row::new(values.into_iter().map(|value| {
                let style = crate::ui::theme::value(&value);
                Cell::from(value).style(style)
            }))
            .style(if index == selected {
                Style::new().bg(CYAN.into()).fg(BG.into()).bold()
            } else {
                Style::new()
            })
        });
    frame.render_widget(
        Table::new(
            rows,
            [
                Constraint::Percentage(31),
                Constraint::Percentage(21),
                Constraint::Percentage(16),
                Constraint::Percentage(19),
                Constraint::Percentage(13),
            ],
        )
        .header(header)
        .column_spacing(1)
        .block(crate::ui::components::terminal_block(
            "EXPOSURE",
            "POSITION CONCENTRATION",
        )),
        area,
    );
}

fn render_historical_header(
    frame: &mut Frame,
    area: Rect,
    snapshot: Option<&HistoricalRiskSnapshot>,
    status: &str,
) {
    let kpis = Layout::horizontal([Constraint::Ratio(1, 4); 4]).split(area);
    let values = match snapshot {
        Some(snapshot) => [
            ("ANNUALIZED VOL", snapshot.annualized_volatility_label()),
            ("MAX DRAWDOWN", snapshot.max_drawdown_label()),
            ("HISTORICAL VAR", snapshot.historical_var_label()),
            ("SHARPE", snapshot.sharpe_label()),
        ],
        None => [
            ("ANNUALIZED VOL", "N/A".to_owned()),
            ("MAX DRAWDOWN", "N/A".to_owned()),
            ("HISTORICAL VAR", "N/A".to_owned()),
            ("SHARPE", "N/A".to_owned()),
        ],
    };
    for (index, (label, value)) in values.into_iter().enumerate() {
        frame.render_widget(
            Paragraph::new(vec![
                Line::styled(label, MUTED),
                Line::styled(value, if index == 1 || index == 2 { RED } else { INK }),
                Line::styled(status, YELLOW),
            ])
            .block(crate::ui::components::terminal_block("RISK HISTORY", label)),
            kpis[index],
        );
    }
}

fn render_historical(frame: &mut Frame, area: Rect, snapshot: Option<&HistoricalRiskSnapshot>) {
    let Some(snapshot) = snapshot else {
        frame.render_widget(
            Paragraph::new(vec![
                Line::styled("NO DATED VALUATION HISTORY", RED),
                Line::raw(""),
                Line::styled("PORT IMPORT PERFORMANCE <FILE.CSV>", CYAN),
                Line::raw(""),
                Line::styled(
                    "RISK WILL NOT INFER RETURNS FROM A POSITION SNAPSHOT.",
                    MUTED,
                ),
            ])
            .wrap(Wrap { trim: true })
            .block(crate::ui::components::terminal_block(
                "HISTORICAL",
                "VERSIONED VALUATIONS REQUIRED",
            )),
            area,
        );
        return;
    };

    let header = Row::new(["SERIES", "ABSOLUTE RISK", "TAIL · BENCHMARK-RELATIVE"])
        .style(Style::new().fg(AMBER.into()).bold())
        .bottom_margin(1);
    let rows = snapshot.series.iter().map(|series| {
        let drawdown_detail = if series.max_drawdown_bps == 0 {
            "NO DRAWDOWN OBSERVED".to_owned()
        } else {
            format!(
                "{} → {} · REC {}",
                series.drawdown_peak_date,
                series.drawdown_trough_date,
                series.recovery_date.as_deref().unwrap_or("NOT RECOVERED")
            )
        };
        let value_or_na = |value: Option<i32>| {
            value
                .map(historical::format_hundredths)
                .unwrap_or_else(|| "N/A".to_owned())
        };
        let bps_or_na = |value: Option<i32>| {
            value
                .map(historical::format_bps)
                .unwrap_or_else(|| "N/A".to_owned())
        };
        Row::new([
            Cell::from(vec![
                Line::styled(
                    format!("{} · N {}", series.currency, series.observations),
                    INK,
                ),
                Line::styled(series.period_start.clone(), MUTED),
                Line::styled(format!("→ {}", series.period_end), MUTED),
                Line::styled(
                    format!(
                        "MEDIAN {}D · {:.2}/YR",
                        series.median_interval_days,
                        series.annualization_periods_hundredths as f64 / 100.0
                    ),
                    CYAN,
                ),
            ]),
            Cell::from(vec![
                Line::styled(
                    format!(
                        "VOL {} · EWMA {}",
                        historical::format_bps(series.annualized_volatility_bps),
                        historical::format_bps(series.ewma_volatility_bps)
                    ),
                    INK,
                ),
                Line::styled(
                    format!(
                        "SHARPE {} · SORTINO {}",
                        value_or_na(series.sharpe_hundredths),
                        value_or_na(series.sortino_hundredths)
                    ),
                    INK,
                ),
                Line::styled(
                    format!(
                        "MAX DRAWDOWN {}",
                        historical::format_bps(series.max_drawdown_bps)
                    ),
                    RED,
                ),
                Line::styled(drawdown_detail, MUTED),
            ]),
            Cell::from(vec![
                Line::styled(
                    format!(
                        "HIST VAR {} · CVAR {}",
                        historical::format_bps(series.historical_var_bps),
                        historical::format_bps(series.historical_cvar_bps)
                    ),
                    RED,
                ),
                Line::styled(
                    format!(
                        "GAUSS VAR {} · CVAR {}",
                        historical::format_bps(series.parametric_var_bps),
                        historical::format_bps(series.parametric_cvar_bps)
                    ),
                    RED,
                ),
                Line::styled(
                    format!(
                        "BETA {} · CORR {}",
                        value_or_na(series.beta_hundredths),
                        value_or_na(series.correlation_hundredths)
                    ),
                    INK,
                ),
                Line::styled(
                    format!(
                        "TRACK ERR {} · INFO {}",
                        bps_or_na(series.tracking_error_bps),
                        value_or_na(series.information_ratio_hundredths)
                    ),
                    INK,
                ),
            ]),
        ])
        .height(4)
    });
    frame.render_widget(
        Table::new(
            rows,
            [
                Constraint::Percentage(24),
                Constraint::Percentage(38),
                Constraint::Percentage(38),
            ],
        )
        .header(header)
        .column_spacing(1)
        .block(crate::ui::components::terminal_block(
            "HISTORICAL",
            "ABSOLUTE · DOWNSIDE · BENCHMARK-RELATIVE",
        )),
        area,
    );
}

fn render_historical_provenance(
    frame: &mut Frame,
    area: Rect,
    snapshot: Option<&HistoricalRiskSnapshot>,
) {
    let Some(snapshot) = snapshot else {
        frame.render_widget(
            Paragraph::new(vec![
                Line::styled("INPUT", AMBER),
                Line::styled("NO PERFORMANCE HISTORY", RED),
                Line::raw(""),
                Line::styled("REQUIRED", AMBER),
                Line::styled("DATE · CCY · VALUE · FLOW", MUTED),
                Line::styled("BENCHMARK VALUE OPTIONAL", MUTED),
            ])
            .block(crate::ui::components::terminal_block("METHOD", "NO INPUT")),
            area,
        );
        return;
    };
    let confidence = if snapshot.confidence_bps.is_multiple_of(100) {
        format!("{}%", snapshot.confidence_bps / 100)
    } else {
        format!(
            "{}.{:02}%",
            snapshot.confidence_bps / 100,
            snapshot.confidence_bps % 100
        )
    };
    let mut lines = vec![
        Line::styled("INPUT", AMBER),
        Line::styled(snapshot.source.clone(), INK),
        Line::styled(snapshot.period.clone(), MUTED),
        Line::styled(snapshot.input_version.clone(), CYAN),
        Line::raw(""),
        Line::styled("PARAMETERS", AMBER),
        Line::styled(format!("CONFIDENCE {confidence}"), INK),
        Line::styled(
            format!(
                "EWMA λ {:.6}",
                snapshot.ewma_lambda_millionths as f64 / 1_000_000.0
            ),
            INK,
        ),
        Line::styled(
            format!(
                "ANNUAL RISK-FREE {}",
                historical::format_bps(snapshot.annual_risk_free_rate_bps)
            ),
            INK,
        ),
        Line::raw(""),
        Line::styled("METHODOLOGY", AMBER),
        Line::styled(snapshot.methodology.clone(), MUTED),
        Line::raw(""),
        Line::styled("DISCLOSURES", AMBER),
    ];
    for disclosure in snapshot.disclosures.iter().take(8) {
        lines.push(Line::styled(format!("• {disclosure}"), MUTED));
    }
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: true }).block(
            crate::ui::components::terminal_block("METHOD", "VERSIONED RETURN SERIES"),
        ),
        area,
    );
}

fn render_provenance(frame: &mut Frame, area: Rect, snapshot: &RiskSnapshot) {
    let mut lines = vec![
        Line::styled("INPUT", AMBER),
        Line::styled(snapshot.source.clone(), INK),
        Line::styled(snapshot.as_of.clone(), MUTED),
        Line::styled(snapshot.input_version.clone(), CYAN),
        Line::raw(""),
        Line::styled("CURRENCY SCENARIOS", AMBER),
    ];
    for currency in &snapshot.currencies {
        lines.push(Line::styled(
            format!(
                "{} NAV {} · CASH {}",
                currency.currency,
                domain::format_money(currency.priced_nav),
                domain::format_money(currency.available_cash)
            ),
            INK,
        ));
        lines.push(Line::styled(
            format!(
                "  -10% {} · MAX {} · {} UNPRICED",
                domain::format_money(currency.scenario_change),
                currency
                    .largest_non_cash_weight_bps
                    .map(domain::format_bps)
                    .unwrap_or_else(|| "—".to_owned()),
                currency.unpriced_positions
            ),
            if currency.scenario_change.minor_units() < 0 {
                RED
            } else {
                GREEN
            },
        ));
    }
    lines.extend([
        Line::raw(""),
        Line::styled("METHODOLOGY", AMBER),
        Line::styled(snapshot.methodology.clone(), MUTED),
        Line::raw(""),
        Line::styled("DISCLOSURES", AMBER),
    ]);
    for disclosure in snapshot.disclosures.iter().take(6) {
        lines.push(Line::styled(format!("• {disclosure}"), MUTED));
    }
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: true }).block(
            crate::ui::components::terminal_block("METHOD", "VERSIONED INPUT"),
        ),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::risk::{
        calculate_risk, RiskCurrencyInput, RiskError, RiskInput, RiskPositionInput,
        SCENARIO_SHOCK_BPS,
    };
    use crate::foundation::{Currency, InstrumentId, Money};
    use crate::{bootstrap, runtime};
    use crossterm::event::{KeyModifiers, MouseButton, MouseEventKind};
    use ratatui::{backend::TestBackend, Terminal};
    use std::sync::Arc;

    struct TestRisk(RiskSnapshot);

    impl RiskQuery for TestRisk {
        fn load_risk(&self) -> Result<RiskSnapshot, RiskError> {
            Ok(self.0.clone())
        }
    }

    fn query() -> Arc<dyn RiskQuery> {
        let usd = Currency::new("USD").unwrap();
        let snapshot = calculate_risk(RiskInput {
            positions: vec![RiskPositionInput {
                instrument_id: InstrumentId::new("us:xnas:aapl"),
                account: "ACCOUNT 1".to_owned(),
                symbol: "AAPL".to_owned(),
                currency: usd,
                market_value: Some(Money::from_minor_units(100_000, usd)),
                cash: false,
            }],
            currencies: vec![RiskCurrencyInput {
                currency: usd,
                priced_nav: Money::from_minor_units(100_000, usd),
                available_cash: Money::from_minor_units(0, usd),
                priced_positions: 1,
                unpriced_positions: 0,
            }],
            source: "TEST".to_owned(),
            as_of: "2026-08-27T20:00:00Z".to_owned(),
            input_version: "INPUT-1".to_owned(),
            disclosures: Vec::new(),
        })
        .unwrap();
        Arc::new(TestRisk(snapshot))
    }

    #[test]
    fn risk_command_is_exact_and_selected_security_opens() {
        let mut workspace = RiskWorkspace::new(query());

        assert_eq!(workspace.descriptor().commands, &["RISK"]);
        assert!(workspace.is_favorite());
        assert!(workspace.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));
        assert_eq!(
            workspace.poll_intents(),
            vec![AppIntent::DispatchCommand {
                command: "SEC AAPL".to_owned(),
                origin: ID,
            }]
        );
    }

    #[test]
    fn unsupported_risk_arguments_preserve_the_current_view() {
        let mut workspace = RiskWorkspace::new(query());
        for command in ["RISK AAPL", "RISK HISTORY extra"] {
            workspace.handle_command(&CommandInvocation::parse(command).unwrap());
            assert_eq!(workspace.view, RiskView::Concentration);
            assert!(workspace.status.contains("UNSUPPORTED ARGUMENTS"));
            let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
            terminal
                .draw(|frame| workspace.render(frame, frame.area()))
                .unwrap();
            let text = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>();
            assert!(text.contains("UNSUPPORTED ARGUMENTS"));
        }
    }

    #[test]
    fn long_position_tables_render_and_click_the_visible_position() {
        let mut snapshot = query().load_risk().unwrap();
        snapshot.positions = (0..40)
            .map(|index| {
                let mut position = snapshot.positions[0].clone();
                position.symbol = format!("ROW{index:03}");
                position
            })
            .collect();
        let mut workspace = RiskWorkspace::new(Arc::new(TestRisk(snapshot)));
        workspace.move_selection(39);
        let area = Rect::new(0, 0, 120, 36);
        let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
        terminal
            .draw(|frame| workspace.render(frame, area))
            .unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("ROW039"));
        assert!(!text.contains("ROW000"));
        let table = risk_layout(area).table;
        let visible = table_viewport(table, 40, 39);
        workspace.handle_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: table.x + 2,
                row: table.y + 3,
                modifiers: KeyModifiers::NONE,
            },
            area,
        );
        assert_eq!(workspace.selected, visible.start);
        assert_eq!(
            workspace.poll_intents(),
            vec![AppIntent::DispatchCommand {
                command: format!("SEC ROW{:03}", visible.start),
                origin: ID,
            }]
        );
    }

    #[test]
    fn table_header_sidebar_and_footer_are_clickable() {
        let mut workspace = RiskWorkspace::new(query());
        let area = Rect::new(0, 0, 120, 36);
        let layout = risk_layout(area);
        let click = |x, y| MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        };

        assert!(workspace.handle_mouse(click(layout.header.x + 1, layout.header.y + 1), area));
        assert!(workspace.handle_mouse(click(layout.table.x + 2, layout.table.y + 3), area));
        assert!(!workspace.poll_intents().is_empty());
        assert!(workspace.handle_mouse(click(layout.side.x + 1, layout.side.y + 1), area));
        assert!(workspace.status.contains("PER-CURRENCY"));
        assert!(workspace.handle_mouse(click(layout.footer.x + 45, layout.footer.y), area));
    }

    #[test]
    fn scenario_constant_is_explicitly_negative_ten_percent() {
        assert_eq!(SCENARIO_SHOCK_BPS, -1_000);
    }

    #[test]
    fn application_command_opens_risk_with_standard_clickable_chrome() {
        let mut app = bootstrap::demo_app();
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for character in "RISK".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(app.active_workspace(), ID);
        assert_eq!(
            app.workspaces.shell_chrome(ID),
            crate::app::ShellChrome::Standard
        );
        let mut terminal = Terminal::new(TestBackend::new(160, 48)).unwrap();
        terminal.draw(|frame| runtime::render(frame, &app)).unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("MARKET TERMINAL"));
        assert!(rendered.contains("POSITION CONCENTRATION"));

        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for character in "RISK HISTORY".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        terminal.draw(|frame| runtime::render(frame, &app)).unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("ABSOLUTE · DOWNSIDE · BENCHMARK-RELATIVE"));
        assert!(rendered.contains("EWMA λ 0.940000"));
        assert!(rendered.contains("DEMO-PERFORMANCE-V1"));
    }
}
