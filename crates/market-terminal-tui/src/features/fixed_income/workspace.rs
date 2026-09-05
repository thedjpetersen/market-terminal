use crossterm::event::{KeyCode, KeyEvent, MouseEvent};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Cell, Paragraph, Row, Table, Wrap},
    Frame,
};

use crate::{
    app::{
        CommandInvocation, ViewRestoreReport, ViewValue, Workspace, WorkspaceAction,
        WorkspaceDescriptor, WorkspaceViewState,
    },
    ui::{
        components::terminal_block,
        is_primary_click, scroll_key, table_row_at, table_viewport,
        theme::{AMBER, BG, CYAN, GREEN, INK, MUTED, RED, YELLOW},
    },
};

use super::{analyze_bond, BondAnalytics, BondModelInput, CouponFrequency, ID};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FixedIncomeView {
    Analytics,
    CashFlows,
    Shocks,
}

impl FixedIncomeView {
    const fn label(self) -> &'static str {
        match self {
            Self::Analytics => "PRICE + RISK",
            Self::CashFlows => "CASH FLOWS",
            Self::Shocks => "CURVE SHOCKS",
        }
    }
}

pub struct FixedIncomeWorkspace {
    input: BondModelInput,
    analytics: BondAnalytics,
    view: FixedIncomeView,
    selected_row: usize,
    status: String,
}

impl Default for FixedIncomeWorkspace {
    fn default() -> Self {
        Self::new()
    }
}

impl FixedIncomeWorkspace {
    pub fn new() -> Self {
        let input = BondModelInput::default();
        let analytics = analyze_bond(&input).expect("built-in bond model input is valid");
        Self {
            input,
            analytics,
            view: FixedIncomeView::Analytics,
            selected_row: 0,
            status: "REFERENCE MODEL READY".to_owned(),
        }
    }

    fn apply(&mut self, input: BondModelInput) -> bool {
        match analyze_bond(&input) {
            Ok(analytics) => {
                self.input = input;
                self.analytics = analytics;
                self.selected_row = 0;
                self.status = "MODEL RECOMPUTED FROM EXPLICIT INPUTS".to_owned();
                true
            }
            Err(error) => {
                self.status = format!("REJECTED · {error}");
                false
            }
        }
    }

    fn parse_command(&self, args: &[String]) -> Result<BondModelInput, String> {
        if args.is_empty() {
            return Ok(self.input.clone());
        }
        if args.len() != 8 {
            return Err(
                "USE BOND <ID> <CCY> <FACE> <COUPON%> <YIELD%> <YEARS> <ANNUAL|SEMI|QUARTER> <ACCRUED%>"
                    .to_owned(),
            );
        }
        let frequency = parse_frequency(&args[6])?;
        let input = BondModelInput {
            instrument_id: args[0].trim().to_ascii_uppercase(),
            currency: args[1].trim().to_ascii_uppercase(),
            face_micros: parse_scaled(&args[2], 1_000_000.0, "FACE")?,
            coupon_bps: parse_scaled(&args[3], 100.0, "COUPON")?
                .try_into()
                .map_err(|_| "COUPON IS OUT OF RANGE".to_owned())?,
            yield_bps: parse_scaled(&args[4], 100.0, "YIELD")?
                .try_into()
                .map_err(|_| "YIELD IS OUT OF RANGE".to_owned())?,
            years_to_maturity: parse_u32(&args[5], "YEARS")?,
            frequency,
            accrued_period_bps: parse_scaled(&args[7], 100.0, "ACCRUED")?
                .try_into()
                .map_err(|_| "ACCRUED MUST BE POSITIVE".to_owned())?,
        };
        input.validate().map_err(|error| error.to_string())?;
        Ok(input)
    }

    fn set_frequency(&mut self, frequency: CouponFrequency) {
        let mut input = self.input.clone();
        input.frequency = frequency;
        self.apply(input);
    }

    fn row_count(&self) -> usize {
        match self.view {
            FixedIncomeView::Analytics => 0,
            FixedIncomeView::CashFlows => self.analytics.cash_flows.len(),
            FixedIncomeView::Shocks => self.analytics.scenarios.len(),
        }
    }

    fn select_view(&mut self, view: FixedIncomeView) {
        self.view = view;
        self.selected_row = self.selected_row.min(self.row_count().saturating_sub(1));
    }
}

impl Workspace for FixedIncomeWorkspace {
    fn descriptor(&self) -> WorkspaceDescriptor {
        WorkspaceDescriptor {
            id: ID,
            label: "FIXED INCOME",
            hotkey: '\0',
            commands: &["BOND", "FI", "FIXEDINCOME"],
        }
    }

    fn handle_command(&mut self, invocation: &CommandInvocation) -> bool {
        match self.parse_command(&invocation.args) {
            Ok(input) => {
                if !invocation.args.is_empty() {
                    self.apply(input);
                }
            }
            Err(error) => self.status = format!("REJECTED · {error}"),
        }
        true
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char('1') => self.select_view(FixedIncomeView::Analytics),
            KeyCode::Char('2') => self.select_view(FixedIncomeView::CashFlows),
            KeyCode::Char('3') => self.select_view(FixedIncomeView::Shocks),
            KeyCode::Tab => self.select_view(match self.view {
                FixedIncomeView::Analytics => FixedIncomeView::CashFlows,
                FixedIncomeView::CashFlows => FixedIncomeView::Shocks,
                FixedIncomeView::Shocks => FixedIncomeView::Analytics,
            }),
            KeyCode::Char('a' | 'A') => self.set_frequency(CouponFrequency::Annual),
            KeyCode::Char('s' | 'S') => self.set_frequency(CouponFrequency::SemiAnnual),
            KeyCode::Char('q' | 'Q') => self.set_frequency(CouponFrequency::Quarterly),
            KeyCode::Down | KeyCode::Char('j' | 'J') if self.row_count() > 0 => {
                self.selected_row = self
                    .selected_row
                    .saturating_add(1)
                    .min(self.row_count().saturating_sub(1));
            }
            KeyCode::Up | KeyCode::Char('k' | 'K') if self.row_count() > 0 => {
                self.selected_row = self.selected_row.saturating_sub(1);
            }
            _ => return false,
        }
        true
    }

    fn handle_mouse(&mut self, event: MouseEvent, area: Rect) -> bool {
        let areas = fixed_income_areas(area);
        if is_primary_click(event, areas.tabs) {
            let width = areas.tabs.width / 3;
            let index =
                usize::from(event.column.saturating_sub(areas.tabs.x) / width.max(1)).min(2);
            self.select_view(match index {
                0 => FixedIncomeView::Analytics,
                1 => FixedIncomeView::CashFlows,
                _ => FixedIncomeView::Shocks,
            });
            return true;
        }
        let visible = table_viewport(areas.body, self.row_count(), self.selected_row);
        if let Some(index) = table_row_at(event, areas.body, visible.len()) {
            self.selected_row = visible.start + index;
            return true;
        }
        scroll_key(event, area).is_some_and(|key| self.handle_key(key))
    }

    fn actions(&self, area: Rect) -> Vec<WorkspaceAction> {
        let areas = fixed_income_areas(area);
        let third = areas.tabs.width / 3;
        let views = [
            (
                FixedIncomeView::Analytics,
                "view:analytics",
                "Show price and risk",
            ),
            (
                FixedIncomeView::CashFlows,
                "view:cashflows",
                "Show coupon cash flows",
            ),
            (
                FixedIncomeView::Shocks,
                "view:shocks",
                "Show parallel yield shocks",
            ),
        ];
        let mut actions = views
            .into_iter()
            .enumerate()
            .map(|(index, (view, id, label))| {
                let x = areas
                    .tabs
                    .x
                    .saturating_add(third.saturating_mul(index as u16));
                let width = if index == 2 {
                    areas.tabs.right().saturating_sub(x)
                } else {
                    third
                };
                let action = WorkspaceAction::new(
                    id,
                    label,
                    Rect::new(x, areas.tabs.y, width, areas.tabs.height),
                );
                if view == self.view {
                    action.preferred()
                } else {
                    action
                }
            })
            .collect::<Vec<_>>();
        let thirds = header_thirds(areas.header);
        actions.extend([
            WorkspaceAction::new("frequency:annual", "Use annual coupons", thirds[0]),
            WorkspaceAction::new("frequency:semi", "Use semiannual coupons", thirds[1]),
            WorkspaceAction::new("frequency:quarter", "Use quarterly coupons", thirds[2]),
        ]);
        actions
    }

    fn activate_action(&mut self, id: &str) -> bool {
        match id {
            "view:analytics" => self.select_view(FixedIncomeView::Analytics),
            "view:cashflows" => self.select_view(FixedIncomeView::CashFlows),
            "view:shocks" => self.select_view(FixedIncomeView::Shocks),
            "frequency:annual" => self.set_frequency(CouponFrequency::Annual),
            "frequency:semi" => self.set_frequency(CouponFrequency::SemiAnnual),
            "frequency:quarter" => self.set_frequency(CouponFrequency::Quarterly),
            _ => return false,
        }
        true
    }

    fn capture_view(&self) -> WorkspaceViewState {
        WorkspaceViewState::new(ID.as_str())
            .with_field(
                "instrument_id",
                ViewValue::Text(self.input.instrument_id.clone()),
            )
            .with_field("currency", ViewValue::Text(self.input.currency.clone()))
            .with_field(
                "face_micros",
                ViewValue::Unsigned(self.input.face_micros as u64),
            )
            .with_field(
                "coupon_bps",
                ViewValue::Text(self.input.coupon_bps.to_string()),
            )
            .with_field(
                "yield_bps",
                ViewValue::Text(self.input.yield_bps.to_string()),
            )
            .with_field(
                "years",
                ViewValue::Unsigned(u64::from(self.input.years_to_maturity)),
            )
            .with_field(
                "frequency",
                ViewValue::Text(self.input.frequency.label().to_owned()),
            )
            .with_field(
                "accrued_bps",
                ViewValue::Unsigned(u64::from(self.input.accrued_period_bps)),
            )
            .with_field("view", ViewValue::Text(self.view.label().to_owned()))
            .with_field(
                "selected_row",
                ViewValue::Unsigned(self.selected_row as u64),
            )
    }

    fn restore_view(&mut self, state: &WorkspaceViewState) -> ViewRestoreReport {
        if !state.workspace.eq_ignore_ascii_case(ID.as_str()) {
            return ViewRestoreReport::warning(format!(
                "saved state belongs to {}, not fixed_income",
                state.workspace
            ));
        }
        let mut input = self.input.clone();
        let mut present = 0;
        if let Some(value) = state
            .fields
            .get("instrument_id")
            .and_then(ViewValue::as_text)
        {
            input.instrument_id = value.to_owned();
            present += 1;
        }
        if let Some(value) = state.fields.get("currency").and_then(ViewValue::as_text) {
            input.currency = value.to_owned();
            present += 1;
        }
        if let Some(value) = state
            .fields
            .get("face_micros")
            .and_then(ViewValue::as_unsigned)
        {
            input.face_micros = i64::try_from(value).unwrap_or(i64::MAX);
            present += 1;
        }
        if let Some(value) = state
            .fields
            .get("coupon_bps")
            .and_then(ViewValue::as_text)
            .and_then(|value| value.parse().ok())
        {
            input.coupon_bps = value;
            present += 1;
        }
        if let Some(value) = state
            .fields
            .get("yield_bps")
            .and_then(ViewValue::as_text)
            .and_then(|value| value.parse().ok())
        {
            input.yield_bps = value;
            present += 1;
        }
        if let Some(value) = state.fields.get("years").and_then(ViewValue::as_unsigned) {
            input.years_to_maturity = u32::try_from(value).unwrap_or(u32::MAX);
            present += 1;
        }
        if let Some(value) = state.fields.get("frequency").and_then(ViewValue::as_text) {
            if let Ok(frequency) = parse_frequency(value) {
                input.frequency = frequency;
                present += 1;
            }
        }
        if let Some(value) = state
            .fields
            .get("accrued_bps")
            .and_then(ViewValue::as_unsigned)
        {
            input.accrued_period_bps = u32::try_from(value).unwrap_or(u32::MAX);
            present += 1;
        }
        let mut report = ViewRestoreReport::default();
        match analyze_bond(&input) {
            Ok(analytics) => {
                self.input = input;
                self.analytics = analytics;
                report.restored_fields = present;
            }
            Err(error) => {
                report.skipped_fields = present;
                report
                    .warnings
                    .push(format!("ignored invalid fixed-income model state: {error}"));
            }
        }
        if let Some(value) = state.fields.get("view").and_then(ViewValue::as_text) {
            match value {
                "PRICE + RISK" => self.view = FixedIncomeView::Analytics,
                "CASH FLOWS" => self.view = FixedIncomeView::CashFlows,
                "CURVE SHOCKS" => self.view = FixedIncomeView::Shocks,
                _ => report.skipped_fields += 1,
            }
        }
        if let Some(value) = state
            .fields
            .get("selected_row")
            .and_then(ViewValue::as_unsigned)
        {
            self.selected_row = usize::try_from(value)
                .unwrap_or(0)
                .min(self.row_count().saturating_sub(1));
        }
        report
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let areas = fixed_income_areas(area);
        render_header(frame, areas.header, &self.analytics, &self.status);
        render_tabs(frame, areas.tabs, self.view);
        match self.view {
            FixedIncomeView::Analytics => render_analytics(frame, areas.body, &self.analytics),
            FixedIncomeView::CashFlows => {
                render_cash_flows(frame, areas.body, &self.analytics, self.selected_row)
            }
            FixedIncomeView::Shocks => {
                render_shocks(frame, areas.body, &self.analytics, self.selected_row)
            }
        }
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" 1/2/3/TAB VIEW  ", CYAN),
                Span::styled("↑↓/JK ROW  ", INK),
                Span::styled("A/S/Q FREQUENCY  ", YELLOW),
                Span::styled("REFERENCE MODEL · NO LIVE CURVE", RED),
            ]))
            .style(Style::new().bg(BG.into())),
            areas.footer,
        );
    }
}

struct FixedIncomeAreas {
    header: Rect,
    tabs: Rect,
    body: Rect,
    footer: Rect,
}
fn fixed_income_areas(area: Rect) -> FixedIncomeAreas {
    let rows = Layout::vertical([
        Constraint::Length(4),
        Constraint::Length(3),
        Constraint::Min(6),
        Constraint::Length(2),
    ])
    .split(area);
    FixedIncomeAreas {
        header: rows[0],
        tabs: rows[1],
        body: rows[2],
        footer: rows[3],
    }
}

fn header_thirds(area: Rect) -> [Rect; 3] {
    let width = area.width / 3;
    [
        Rect::new(area.x, area.y, width, area.height),
        Rect::new(area.x.saturating_add(width), area.y, width, area.height),
        Rect::new(
            area.x.saturating_add(width.saturating_mul(2)),
            area.y,
            area.width.saturating_sub(width.saturating_mul(2)),
            area.height,
        ),
    ]
}

fn render_header(frame: &mut Frame, area: Rect, analytics: &BondAnalytics, status: &str) {
    let input = &analytics.input;
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled(
                    format!(" {}  ", input.instrument_id),
                    Style::new().bg(AMBER.into()).fg(BG.into()).bold(),
                ),
                Span::styled(
                    format!(
                        "{} {:.2} FACE  {:.2}% CPN  {:.2}% YLD  {}Y  {}  {:.2}% ACCRUED",
                        input.currency,
                        money(input.face_micros),
                        f64::from(input.coupon_bps) / 100.0,
                        f64::from(input.yield_bps) / 100.0,
                        input.years_to_maturity,
                        input.frequency.label(),
                        f64::from(input.accrued_period_bps) / 100.0
                    ),
                    INK,
                ),
            ]),
            Line::from(vec![
                Span::styled(analytics.model_version, CYAN),
                Span::styled(format!(" · {} · {status}", analytics.input_digest), MUTED),
            ]),
        ])
        .block(terminal_block("FI", "FIXED-RATE BULLET REFERENCE MODEL"))
        .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_tabs(frame: &mut Frame, area: Rect, active: FixedIncomeView) {
    let line = [
        FixedIncomeView::Analytics,
        FixedIncomeView::CashFlows,
        FixedIncomeView::Shocks,
    ]
    .into_iter()
    .flat_map(|view| {
        let style = if view == active {
            Style::new().bg(CYAN.into()).fg(BG.into()).bold()
        } else {
            Style::new().fg(MUTED.into())
        };
        [
            Span::styled(format!("  {}  ", view.label()), style),
            Span::raw("  "),
        ]
    })
    .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(Line::from(line)).block(terminal_block("VIEW", "VALUATION EVIDENCE")),
        area,
    );
}

fn render_analytics(frame: &mut Frame, area: Rect, analytics: &BondAnalytics) {
    let columns =
        Layout::horizontal([Constraint::Percentage(48), Constraint::Percentage(52)]).split(area);
    let lines = vec![
        metric(
            "CLEAN PRICE",
            format!(
                "{} {:.6}",
                analytics.input.currency,
                money(analytics.clean_price_micros)
            ),
        ),
        metric(
            "DIRTY PRICE",
            format!(
                "{} {:.6}",
                analytics.input.currency,
                money(analytics.dirty_price_micros)
            ),
        ),
        metric(
            "ACCRUED INTEREST",
            format!(
                "{} {:.6}",
                analytics.input.currency,
                money(analytics.accrued_interest_micros)
            ),
        ),
        metric(
            "COUPON / PERIOD",
            format!(
                "{} {:.6}",
                analytics.input.currency,
                money(analytics.coupon_payment_micros)
            ),
        ),
        metric(
            "CURRENT YIELD",
            format!("{:.2}%", f64::from(analytics.current_yield_bps) / 100.0),
        ),
        metric(
            "MACAULAY DURATION",
            format!(
                "{:.6} Y",
                metric_value(analytics.macaulay_duration_years_millionths)
            ),
        ),
        metric(
            "MODIFIED DURATION",
            format!(
                "{:.6} Y",
                metric_value(analytics.modified_duration_years_millionths)
            ),
        ),
        metric(
            "CONVEXITY",
            format!(
                "{:.6} Y²",
                metric_value(analytics.convexity_years2_millionths)
            ),
        ),
        metric(
            "DV01",
            format!(
                "{} {:.6}",
                analytics.input.currency,
                money(analytics.dv01_micros)
            ),
        ),
    ];
    frame.render_widget(
        Paragraph::new(lines).block(terminal_block("MODEL", "PRICE / YIELD / SENSITIVITY")),
        columns[0],
    );
    let mut evidence = vec![Line::styled(analytics.methodology, AMBER), Line::raw("")];
    evidence.extend(
        analytics
            .disclosures
            .iter()
            .map(|value| Line::styled(format!("• {value}"), MUTED)),
    );
    evidence.push(Line::raw(""));
    evidence.push(Line::styled(
        "Provider price, curve, spread, calendar, and credit state: NOT LOADED",
        RED,
    ));
    frame.render_widget(
        Paragraph::new(evidence)
            .block(terminal_block("BOUND", "CONVENTIONS + LIMITS"))
            .wrap(Wrap { trim: true }),
        columns[1],
    );
}

fn render_cash_flows(frame: &mut Frame, area: Rect, analytics: &BondAnalytics, selected: usize) {
    let rows = analytics
        .cash_flows
        .iter()
        .enumerate()
        .skip(table_viewport(area, analytics.cash_flows.len(), selected).start)
        .take(table_viewport(area, analytics.cash_flows.len(), selected).len())
        .map(|(index, flow)| {
            let style = if index == selected {
                Style::new().bg(CYAN.into()).fg(BG.into()).bold()
            } else {
                Style::new().fg(INK.into())
            };
            Row::new(vec![
                Cell::from(flow.ordinal.to_string()),
                Cell::from(format!("{:.6}", metric_value(flow.time_years_millionths))),
                Cell::from(format!("{:.6}", money(flow.coupon_micros))),
                Cell::from(format!("{:.6}", money(flow.principal_micros))),
                Cell::from(format!("{:.6}", money(flow.total_micros))),
                Cell::from(format!("{:.6}", money(flow.present_value_micros))),
            ])
            .style(style)
        });
    frame.render_widget(
        Table::new(
            rows,
            [
                Constraint::Length(7),
                Constraint::Length(12),
                Constraint::Length(15),
                Constraint::Length(15),
                Constraint::Length(15),
                Constraint::Length(16),
            ],
        )
        .header(
            Row::new([
                "PERIOD",
                "TIME (Y)",
                "COUPON",
                "PRINCIPAL",
                "TOTAL",
                "PRESENT VALUE",
            ])
            .style(AMBER)
            .bottom_margin(1),
        )
        .block(terminal_block(
            "SCHEDULE",
            "CONTRACTUAL CASH FLOWS · MODEL TIME",
        )),
        area,
    );
}

fn render_shocks(frame: &mut Frame, area: Rect, analytics: &BondAnalytics, selected: usize) {
    let rows = analytics
        .scenarios
        .iter()
        .enumerate()
        .skip(table_viewport(area, analytics.scenarios.len(), selected).start)
        .take(table_viewport(area, analytics.scenarios.len(), selected).len())
        .map(|(index, scenario)| {
            let style = if index == selected {
                Style::new().bg(CYAN.into()).fg(BG.into()).bold()
            } else if scenario.clean_change_micros >= 0 {
                Style::new().fg(GREEN.into())
            } else {
                Style::new().fg(RED.into())
            };
            Row::new(vec![
                Cell::from(format!("{:+} BP", scenario.shock_bps)),
                Cell::from(format!("{:.2}%", f64::from(scenario.yield_bps) / 100.0)),
                Cell::from(format!("{:.6}", money(scenario.clean_price_micros))),
                Cell::from(format!("{:.6}", money(scenario.dirty_price_micros))),
                Cell::from(format!("{:+.6}", money(scenario.clean_change_micros))),
                Cell::from(format!(
                    "{:+.2}%",
                    f64::from(scenario.clean_change_bps) / 100.0
                )),
            ])
            .style(style)
        });
    frame.render_widget(
        Table::new(
            rows,
            [
                Constraint::Length(12),
                Constraint::Length(12),
                Constraint::Length(16),
                Constraint::Length(16),
                Constraint::Length(16),
                Constraint::Length(13),
            ],
        )
        .header(
            Row::new(["PARALLEL", "YIELD", "CLEAN", "DIRTY", "CLEAN Δ", "CLEAN Δ%"])
                .style(AMBER)
                .bottom_margin(1),
        )
        .block(terminal_block(
            "SHOCK",
            "7 DETERMINISTIC PARALLEL YIELD SCENARIOS",
        )),
        area,
    );
}

fn metric(label: &str, value: String) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label:<21}"), MUTED),
        Span::styled(value, INK),
    ])
}
fn money(value: i64) -> f64 {
    value as f64 / 1_000_000.0
}
fn metric_value(value: i64) -> f64 {
    value as f64 / 1_000_000.0
}
fn parse_frequency(value: &str) -> Result<CouponFrequency, String> {
    match value.trim().to_ascii_uppercase().as_str() {
        "ANNUAL" | "A" | "1" => Ok(CouponFrequency::Annual),
        "SEMI" | "SEMIANNUAL" | "S" | "2" => Ok(CouponFrequency::SemiAnnual),
        "QUARTER" | "QUARTERLY" | "Q" | "4" => Ok(CouponFrequency::Quarterly),
        _ => Err("FREQUENCY MUST BE ANNUAL, SEMI, OR QUARTER".to_owned()),
    }
}
fn parse_u32(value: &str, label: &str) -> Result<u32, String> {
    value
        .parse()
        .map_err(|_| format!("{label} MUST BE AN UNSIGNED INTEGER"))
}
fn parse_scaled(value: &str, scale: f64, label: &str) -> Result<i64, String> {
    let parsed = value
        .parse::<f64>()
        .map_err(|_| format!("{label} MUST BE NUMERIC"))?;
    let scaled = (parsed * scale).round();
    if !scaled.is_finite() || scaled < i64::MIN as f64 || scaled > i64::MAX as f64 {
        Err(format!("{label} IS OUT OF RANGE"))
    } else {
        Ok(scaled as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyModifiers, MouseButton, MouseEventKind};
    use ratatui::{backend::TestBackend, Terminal};

    fn render(workspace: &FixedIncomeWorkspace, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| workspace.render(frame, frame.area()))
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[test]
    fn renders_all_views_at_three_terminal_sizes() {
        for (width, height) in [(80, 24), (120, 36), (160, 48)] {
            let mut workspace = FixedIncomeWorkspace::new();
            assert!(render(&workspace, width, height).contains("CLEAN PRICE"));
            workspace.select_view(FixedIncomeView::CashFlows);
            assert!(render(&workspace, width, height).contains("PRESENT VALUE"));
            workspace.select_view(FixedIncomeView::Shocks);
            assert!(render(&workspace, width, height).contains("PARALLEL"));
        }
    }

    #[test]
    fn cash_flow_navigation_reaches_maturity_and_mouse_wheel_returns() {
        let mut workspace = FixedIncomeWorkspace::new();
        workspace.handle_command(
            &CommandInvocation::parse("BOND LONG USD 1000 5 4 100 QUARTER 0").unwrap(),
        );
        workspace.select_view(FixedIncomeView::CashFlows);
        for _ in 0..400 {
            workspace.handle_key(KeyEvent::new(
                KeyCode::Down,
                crossterm::event::KeyModifiers::NONE,
            ));
        }
        assert_eq!(workspace.selected_row, 399);
        assert!(render(&workspace, 120, 36).contains("100.000000"));
        let area = Rect::new(0, 0, 120, 36);
        let body = fixed_income_areas(area).body;
        let visible = table_viewport(body, 400, 399);
        let event = MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: body.x + 2,
            row: body.y + 3,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };
        assert!(workspace.handle_mouse(event, area));
        assert_eq!(workspace.selected_row, visible.start);
        assert!(workspace.handle_mouse(
            MouseEvent {
                kind: crossterm::event::MouseEventKind::ScrollUp,
                ..event
            },
            area
        ));
        assert_eq!(workspace.selected_row, visible.start - 1);
        assert!(!workspace.handle_mouse(
            MouseEvent {
                kind: crossterm::event::MouseEventKind::Moved,
                ..event
            },
            area
        ));
    }

    #[test]
    fn command_application_is_atomic() {
        let mut workspace = FixedIncomeWorkspace::new();
        workspace.handle_command(&CommandInvocation {
            function: "BOND".to_owned(),
            args: "CORP-7Y EUR 1000 6.25 5.5 7 QUARTER 25"
                .split_whitespace()
                .map(str::to_owned)
                .collect(),
        });
        assert_eq!(workspace.input.instrument_id, "CORP-7Y");
        assert_eq!(workspace.input.currency, "EUR");
        assert_eq!(workspace.input.frequency, CouponFrequency::Quarterly);
        let valid = workspace.input.clone();
        workspace.handle_command(&CommandInvocation {
            function: "BOND".to_owned(),
            args: "BAD USD 0 5 5 5 SEMI 0"
                .split_whitespace()
                .map(str::to_owned)
                .collect(),
        });
        assert_eq!(workspace.input, valid);
        assert!(workspace.status.starts_with("REJECTED"));
    }

    #[test]
    fn saved_view_round_trips_and_invalid_state_degrades_without_mutation() {
        let mut source = FixedIncomeWorkspace::new();
        source.handle_command(&CommandInvocation {
            function: "FI".to_owned(),
            args: "MUNI-10Y USD 500 3.5 4.1 10 SEMI 30"
                .split_whitespace()
                .map(str::to_owned)
                .collect(),
        });
        source.select_view(FixedIncomeView::Shocks);
        source.selected_row = 4;
        let state = source.capture_view();
        let mut restored = FixedIncomeWorkspace::new();
        let report = restored.restore_view(&state);
        assert!(report.warnings.is_empty());
        assert_eq!(restored.capture_view(), state);
        let before = restored.input.clone();
        let mut invalid = state;
        invalid
            .fields
            .insert("face_micros".to_owned(), ViewValue::Unsigned(0));
        let report = restored.restore_view(&invalid);
        assert!(!report.warnings.is_empty());
        assert_eq!(restored.input, before);
    }

    #[test]
    fn actions_mouse_and_keyboard_share_geometry() {
        let mut workspace = FixedIncomeWorkspace::new();
        let area = Rect::new(0, 0, 120, 36);
        assert!(workspace.activate_action("view:cashflows"));
        assert!(workspace.activate_action("frequency:quarter"));
        assert_eq!(workspace.input.frequency, CouponFrequency::Quarterly);
        let body = fixed_income_areas(area).body;
        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: body.x.saturating_add(1),
            row: body.y.saturating_add(4),
            modifiers: KeyModifiers::NONE,
        };
        assert!(workspace.handle_mouse(click, area));
        assert_eq!(workspace.selected_row, 1);
        assert!(workspace
            .actions(area)
            .iter()
            .all(|action| crate::ui::contains(area, action.area.x, action.area.y)));
    }
}
