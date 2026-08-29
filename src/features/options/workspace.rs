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
        AppIntent, CommandInvocation, ViewRestoreReport, ViewValue, Workspace, WorkspaceAction,
        WorkspaceDescriptor, WorkspaceViewState,
    },
    ui::{
        components::terminal_block,
        is_primary_click, table_row_at,
        theme::{AMBER, BG, CYAN, GREEN, INK, MUTED, RED, YELLOW},
    },
};

use super::{price_option, OptionAnalytics, OptionModelInput, OptionRight, ID};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OptionsView {
    Greeks,
    Scenarios,
}

impl OptionsView {
    const fn label(self) -> &'static str {
        match self {
            Self::Greeks => "MODEL + GREEKS",
            Self::Scenarios => "SCENARIOS",
        }
    }
}

pub struct OptionsWorkspace {
    input: OptionModelInput,
    analytics: OptionAnalytics,
    view: OptionsView,
    selected_scenario: usize,
    status: String,
    pending_intents: Vec<AppIntent>,
}

impl Default for OptionsWorkspace {
    fn default() -> Self {
        Self::new()
    }
}

impl OptionsWorkspace {
    pub fn new() -> Self {
        let input = OptionModelInput::default();
        let analytics = price_option(&input).expect("built-in option model input is valid");
        Self {
            input,
            analytics,
            view: OptionsView::Greeks,
            selected_scenario: 7,
            status: "REFERENCE MODEL READY".to_owned(),
            pending_intents: Vec::new(),
        }
    }

    fn apply(&mut self, input: OptionModelInput) -> bool {
        match price_option(&input) {
            Ok(analytics) => {
                self.input = input;
                self.analytics = analytics;
                self.selected_scenario = 7;
                self.status = "MODEL RECOMPUTED FROM EXPLICIT INPUTS".to_owned();
                true
            }
            Err(error) => {
                self.status = format!("REJECTED · {error}");
                false
            }
        }
    }

    fn parse_command(&self, args: &[String]) -> Result<OptionModelInput, String> {
        if args.is_empty() {
            return Ok(self.input.clone());
        }
        if !(args.len() == 8 || args.len() == 9) {
            return Err(
                "USE OPTIONS <SYMBOL> <CALL|PUT> <SPOT> <STRIKE> <DAYS> <VOL%> <RATE%> <DIV%> [MULT]"
                    .to_owned(),
            );
        }
        let right = match args[1].to_ascii_uppercase().as_str() {
            "CALL" | "C" => OptionRight::Call,
            "PUT" | "P" => OptionRight::Put,
            _ => return Err("OPTION RIGHT MUST BE CALL OR PUT".to_owned()),
        };
        let input = OptionModelInput {
            symbol: args[0].trim().to_ascii_uppercase(),
            right,
            spot_micros: parse_scaled(&args[2], 1_000_000.0, "SPOT")?,
            strike_micros: parse_scaled(&args[3], 1_000_000.0, "STRIKE")?,
            days_to_expiry: parse_u32(&args[4], "DAYS")?,
            volatility_bps: parse_scaled(&args[5], 100.0, "VOL")?
                .try_into()
                .map_err(|_| "VOL MUST BE POSITIVE".to_owned())?,
            risk_free_rate_bps: parse_scaled(&args[6], 100.0, "RATE")?
                .try_into()
                .map_err(|_| "RATE IS OUT OF RANGE".to_owned())?,
            dividend_yield_bps: parse_scaled(&args[7], 100.0, "DIV")?
                .try_into()
                .map_err(|_| "DIV IS OUT OF RANGE".to_owned())?,
            contract_multiplier: args
                .get(8)
                .map_or(Ok(100), |value| parse_u32(value, "MULT"))?,
        };
        input.validate().map_err(|error| error.to_string())?;
        Ok(input)
    }

    fn set_right(&mut self, right: OptionRight) {
        let mut input = self.input.clone();
        input.right = right;
        self.apply(input);
    }

    fn open_chart(&mut self) -> bool {
        self.pending_intents.push(AppIntent::DispatchCommand {
            command: format!("CHART {}", self.input.symbol),
            origin: ID,
        });
        true
    }
}

impl Workspace for OptionsWorkspace {
    fn descriptor(&self) -> WorkspaceDescriptor {
        WorkspaceDescriptor {
            id: ID,
            label: "OPTIONS",
            hotkey: '\0',
            commands: &["OPTIONS", "OPT"],
        }
    }

    fn handle_command(&mut self, invocation: &CommandInvocation) -> bool {
        match self.parse_command(&invocation.args) {
            Ok(input) => {
                if !invocation.args.is_empty() {
                    self.apply(input);
                }
                true
            }
            Err(message) => {
                self.status = format!("REJECTED · {message}");
                true
            }
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char('1') => self.view = OptionsView::Greeks,
            KeyCode::Char('2') => self.view = OptionsView::Scenarios,
            KeyCode::Tab => {
                self.view = match self.view {
                    OptionsView::Greeks => OptionsView::Scenarios,
                    OptionsView::Scenarios => OptionsView::Greeks,
                }
            }
            KeyCode::Char('c' | 'C') => self.set_right(OptionRight::Call),
            KeyCode::Char('p' | 'P') => self.set_right(OptionRight::Put),
            KeyCode::Char('g' | 'G') => return self.open_chart(),
            KeyCode::Down | KeyCode::Char('j' | 'J') if self.view == OptionsView::Scenarios => {
                self.selected_scenario = self
                    .selected_scenario
                    .saturating_add(1)
                    .min(self.analytics.scenarios.len().saturating_sub(1));
            }
            KeyCode::Up | KeyCode::Char('k' | 'K') if self.view == OptionsView::Scenarios => {
                self.selected_scenario = self.selected_scenario.saturating_sub(1);
            }
            _ => return false,
        }
        true
    }

    fn handle_mouse(&mut self, event: MouseEvent, area: Rect) -> bool {
        let areas = options_areas(area);
        if is_primary_click(event, areas.tabs) {
            self.view = if event.column < areas.tabs.x.saturating_add(areas.tabs.width / 2) {
                OptionsView::Greeks
            } else {
                OptionsView::Scenarios
            };
            return true;
        }
        if self.view == OptionsView::Scenarios {
            if let Some(index) = table_row_at(event, areas.body, self.analytics.scenarios.len()) {
                self.selected_scenario = index;
                return true;
            }
        }
        if is_primary_click(event, areas.footer) {
            return self.open_chart();
        }
        Workspace::handle_mouse(self, event, area)
    }

    fn actions(&self, area: Rect) -> Vec<WorkspaceAction> {
        let areas = options_areas(area);
        let half = areas.tabs.width / 2;
        let mut greeks = WorkspaceAction::new(
            "view:greeks",
            "Show model price and Greeks",
            Rect::new(areas.tabs.x, areas.tabs.y, half, areas.tabs.height),
        );
        let mut scenarios = WorkspaceAction::new(
            "view:scenarios",
            "Show spot and volatility scenarios",
            Rect::new(
                areas.tabs.x.saturating_add(half),
                areas.tabs.y,
                areas.tabs.width.saturating_sub(half),
                areas.tabs.height,
            ),
        );
        if self.view == OptionsView::Greeks {
            greeks = greeks.preferred();
        } else {
            scenarios = scenarios.preferred();
        }
        vec![
            greeks,
            scenarios,
            WorkspaceAction::new("right:call", "Price as a call", left_half(areas.header)),
            WorkspaceAction::new("right:put", "Price as a put", right_half(areas.header)),
            WorkspaceAction::new("open:chart", "Open underlying chart", areas.footer),
        ]
    }

    fn activate_action(&mut self, id: &str) -> bool {
        match id {
            "view:greeks" => self.view = OptionsView::Greeks,
            "view:scenarios" => self.view = OptionsView::Scenarios,
            "right:call" => self.set_right(OptionRight::Call),
            "right:put" => self.set_right(OptionRight::Put),
            "open:chart" => return self.open_chart(),
            _ => return false,
        }
        true
    }

    fn poll_intents(&mut self) -> Vec<AppIntent> {
        std::mem::take(&mut self.pending_intents)
    }

    fn capture_view(&self) -> WorkspaceViewState {
        WorkspaceViewState::new(ID.as_str())
            .with_field("symbol", ViewValue::Text(self.input.symbol.clone()))
            .with_field(
                "right",
                ViewValue::Text(self.input.right.label().to_owned()),
            )
            .with_field(
                "spot_micros",
                ViewValue::Unsigned(self.input.spot_micros as u64),
            )
            .with_field(
                "strike_micros",
                ViewValue::Unsigned(self.input.strike_micros as u64),
            )
            .with_field(
                "days",
                ViewValue::Unsigned(u64::from(self.input.days_to_expiry)),
            )
            .with_field(
                "volatility_bps",
                ViewValue::Unsigned(u64::from(self.input.volatility_bps)),
            )
            .with_field(
                "rate_bps",
                ViewValue::Text(self.input.risk_free_rate_bps.to_string()),
            )
            .with_field(
                "dividend_bps",
                ViewValue::Text(self.input.dividend_yield_bps.to_string()),
            )
            .with_field(
                "multiplier",
                ViewValue::Unsigned(u64::from(self.input.contract_multiplier)),
            )
            .with_field("view", ViewValue::Text(self.view.label().to_owned()))
            .with_field(
                "scenario",
                ViewValue::Unsigned(self.selected_scenario as u64),
            )
    }

    fn restore_view(&mut self, state: &WorkspaceViewState) -> ViewRestoreReport {
        if !state.workspace.eq_ignore_ascii_case(ID.as_str()) {
            return ViewRestoreReport::warning(format!(
                "saved state belongs to {}, not options",
                state.workspace
            ));
        }
        let mut restored = self.input.clone();
        if let Some(value) = state.fields.get("symbol").and_then(ViewValue::as_text) {
            restored.symbol = value.to_owned();
        }
        if let Some(value) = state.fields.get("right").and_then(ViewValue::as_text) {
            restored.right = if value == "PUT" {
                OptionRight::Put
            } else if value == "CALL" {
                OptionRight::Call
            } else {
                return ViewRestoreReport::warning("ignored invalid option right");
            };
        }
        for (name, target) in [
            ("spot_micros", &mut restored.spot_micros),
            ("strike_micros", &mut restored.strike_micros),
        ] {
            if let Some(value) = state.fields.get(name).and_then(ViewValue::as_unsigned) {
                *target = i64::try_from(value).unwrap_or(i64::MAX);
            }
        }
        if let Some(value) = state.fields.get("days").and_then(ViewValue::as_unsigned) {
            restored.days_to_expiry = u32::try_from(value).unwrap_or(u32::MAX);
        }
        if let Some(value) = state
            .fields
            .get("volatility_bps")
            .and_then(ViewValue::as_unsigned)
        {
            restored.volatility_bps = u32::try_from(value).unwrap_or(u32::MAX);
        }
        if let Some(value) = state
            .fields
            .get("rate_bps")
            .and_then(ViewValue::as_text)
            .and_then(|value| value.parse().ok())
        {
            restored.risk_free_rate_bps = value;
        }
        if let Some(value) = state
            .fields
            .get("dividend_bps")
            .and_then(ViewValue::as_text)
            .and_then(|value| value.parse().ok())
        {
            restored.dividend_yield_bps = value;
        }
        if let Some(value) = state
            .fields
            .get("multiplier")
            .and_then(ViewValue::as_unsigned)
        {
            restored.contract_multiplier = u32::try_from(value).unwrap_or(u32::MAX);
        }
        let mut report = ViewRestoreReport::default();
        match price_option(&restored) {
            Ok(analytics) => {
                self.input = restored;
                self.analytics = analytics;
                report.restored_fields = 9;
            }
            Err(error) => {
                report.skipped_fields = 9;
                report
                    .warnings
                    .push(format!("ignored invalid option model state: {error}"));
            }
        }
        if let Some(value) = state.fields.get("view").and_then(ViewValue::as_text) {
            match value {
                "MODEL + GREEKS" => self.view = OptionsView::Greeks,
                "SCENARIOS" => self.view = OptionsView::Scenarios,
                _ => report.skipped_fields += 1,
            }
        }
        if let Some(value) = state
            .fields
            .get("scenario")
            .and_then(ViewValue::as_unsigned)
        {
            self.selected_scenario = usize::try_from(value).unwrap_or(0).min(14);
        }
        report
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let areas = options_areas(area);
        render_header(frame, areas.header, &self.analytics, &self.status);
        render_tabs(frame, areas.tabs, self.view);
        match self.view {
            OptionsView::Greeks => render_greeks(frame, areas.body, &self.analytics),
            OptionsView::Scenarios => {
                render_scenarios(frame, areas.body, &self.analytics, self.selected_scenario)
            }
        }
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" 1/2/TAB VIEW  ", CYAN),
                Span::styled("C/P RIGHT  ", INK),
                Span::styled("↑↓/JK SCENARIO  ", YELLOW),
                Span::styled("G CHART  ", GREEN),
                Span::styled("MODEL ONLY · NO CHAIN", RED),
            ]))
            .style(Style::new().bg(BG.into())),
            areas.footer,
        );
    }
}

struct OptionsAreas {
    header: Rect,
    tabs: Rect,
    body: Rect,
    footer: Rect,
}
fn options_areas(area: Rect) -> OptionsAreas {
    let rows = Layout::vertical([
        Constraint::Length(4),
        Constraint::Length(3),
        Constraint::Min(6),
        Constraint::Length(2),
    ])
    .split(area);
    OptionsAreas {
        header: rows[0],
        tabs: rows[1],
        body: rows[2],
        footer: rows[3],
    }
}
fn left_half(area: Rect) -> Rect {
    Rect::new(area.x, area.y, area.width / 2, area.height)
}
fn right_half(area: Rect) -> Rect {
    Rect::new(
        area.x.saturating_add(area.width / 2),
        area.y,
        area.width.saturating_sub(area.width / 2),
        area.height,
    )
}

fn render_header(frame: &mut Frame, area: Rect, analytics: &OptionAnalytics, status: &str) {
    let input = &analytics.input;
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled(
                    format!(" {} {}  ", input.symbol, input.right.label()),
                    Style::new().bg(AMBER.into()).fg(BG.into()).bold(),
                ),
                Span::styled(
                    format!(
                        "S {:.2}  K {:.2}  {}D  IV {:.2}%  R {:.2}%  Q {:.2}%  ×{}",
                        money(input.spot_micros),
                        money(input.strike_micros),
                        input.days_to_expiry,
                        f64::from(input.volatility_bps) / 100.0,
                        f64::from(input.risk_free_rate_bps) / 100.0,
                        f64::from(input.dividend_yield_bps) / 100.0,
                        input.contract_multiplier
                    ),
                    INK,
                ),
            ]),
            Line::from(vec![
                Span::styled(analytics.model_version, CYAN),
                Span::styled(format!(" · {} · {status}", analytics.input_digest), MUTED),
            ]),
        ])
        .block(terminal_block(
            "OPT",
            "TRANSPARENT EUROPEAN REFERENCE MODEL",
        ))
        .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_tabs(frame: &mut Frame, area: Rect, active: OptionsView) {
    let line = [OptionsView::Greeks, OptionsView::Scenarios]
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
        Paragraph::new(Line::from(line)).block(terminal_block("VIEW", "MODEL EVIDENCE")),
        area,
    );
}

fn render_greeks(frame: &mut Frame, area: Rect, analytics: &OptionAnalytics) {
    let columns =
        Layout::horizontal([Constraint::Percentage(48), Constraint::Percentage(52)]).split(area);
    let contract = analytics
        .price_micros
        .saturating_mul(i64::from(analytics.input.contract_multiplier));
    let lines = vec![
        metric(
            "MODEL PRICE",
            format!("{:.4}", money(analytics.price_micros)),
        ),
        metric("CONTRACT VALUE", format!("{:.2}", money(contract))),
        metric(
            "INTRINSIC",
            format!("{:.4}", money(analytics.intrinsic_micros)),
        ),
        metric(
            "TIME VALUE",
            format!("{:.4}", money(analytics.time_value_micros)),
        ),
        metric(
            "DELTA",
            format!("{:.6}", analytics.delta_millionths as f64 / 1_000_000.0),
        ),
        metric(
            "GAMMA",
            format!("{:.9}", analytics.gamma_billionths as f64 / 1_000_000_000.0),
        ),
        metric(
            "VEGA / 1 VOL PT",
            format!("{:.6}", money(analytics.vega_micros_per_point)),
        ),
        metric(
            "THETA / DAY",
            format!("{:.6}", money(analytics.theta_micros_per_day)),
        ),
        metric(
            "RHO / 1 RATE PT",
            format!("{:.6}", money(analytics.rho_micros_per_point)),
        ),
    ];
    frame.render_widget(
        Paragraph::new(lines).block(terminal_block("MODEL", "PRICE + ANALYTIC GREEKS")),
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
        "Provider price, chain quote, OI, volume, IV, and Greeks: NOT LOADED",
        RED,
    ));
    frame.render_widget(
        Paragraph::new(evidence)
            .block(terminal_block("BOUND", "CONVENTIONS + LIMITS"))
            .wrap(Wrap { trim: true }),
        columns[1],
    );
}

fn render_scenarios(frame: &mut Frame, area: Rect, analytics: &OptionAnalytics, selected: usize) {
    let rows = analytics
        .scenarios
        .iter()
        .enumerate()
        .map(|(index, scenario)| {
            let style = if index == selected {
                Style::new().bg(CYAN.into()).fg(BG.into()).bold()
            } else {
                Style::new().fg(INK.into())
            };
            Row::new(vec![
                Cell::from(format!("{:+.0}%", scenario.spot_shock_bps as f64 / 100.0)),
                Cell::from(format!(
                    "{:+.0} PT",
                    scenario.volatility_shift_bps as f64 / 100.0
                )),
                Cell::from(format!("{:.2}", money(scenario.spot_micros))),
                Cell::from(format!(
                    "{:.2}%",
                    f64::from(scenario.volatility_bps) / 100.0
                )),
                Cell::from(format!("{:.4}", money(scenario.price_micros))),
                Cell::from(format!("{:.2}", money(scenario.contract_value_micros))),
            ])
            .style(style)
        });
    frame.render_widget(
        Table::new(
            rows,
            [
                Constraint::Length(10),
                Constraint::Length(10),
                Constraint::Length(12),
                Constraint::Length(10),
                Constraint::Length(14),
                Constraint::Length(16),
            ],
        )
        .header(
            Row::new([
                "SPOT SHOCK",
                "VOL SHIFT",
                "SPOT",
                "VOL",
                "MODEL PRICE",
                "CONTRACT VALUE",
            ])
            .style(AMBER)
            .bottom_margin(1),
        )
        .block(terminal_block(
            "GRID",
            "15 DETERMINISTIC SPOT × VOL SCENARIOS",
        )),
        area,
    );
}

fn metric(label: &str, value: String) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label:<20}"), MUTED),
        Span::styled(value, INK),
    ])
}
fn money(micros: i64) -> f64 {
    micros as f64 / 1_000_000.0
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

    fn render(workspace: &OptionsWorkspace, width: u16, height: u16) -> String {
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
    fn renders_model_and_scenarios_at_three_sizes() {
        for (width, height) in [(80, 24), (120, 36), (160, 48)] {
            let mut workspace = OptionsWorkspace::new();
            let model = render(&workspace, width, height);
            assert!(model.contains("MODEL PRICE"));
            assert!(model.contains("NO CHAIN"));
            workspace.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
            let scenarios = render(&workspace, width, height);
            assert!(scenarios.contains("SPOT SHOCK"));
            assert!(scenarios.contains("CONTRACT VALUE"));
        }
    }

    #[test]
    fn command_is_atomic_and_rejects_invalid_inputs() {
        let mut workspace = OptionsWorkspace::new();
        let original = workspace.input.clone();
        workspace.handle_command(&CommandInvocation {
            function: "OPTIONS".to_owned(),
            args: "MSFT PUT 420 400 45 30 4.5 0.8 100"
                .split_whitespace()
                .map(str::to_owned)
                .collect(),
        });
        assert_eq!(workspace.input.symbol, "MSFT");
        assert_eq!(workspace.input.right, OptionRight::Put);
        let valid = workspace.input.clone();
        workspace.handle_command(&CommandInvocation {
            function: "OPTIONS".to_owned(),
            args: "BAD CALL 0 100 30 20 5 0"
                .split_whitespace()
                .map(str::to_owned)
                .collect(),
        });
        assert_eq!(workspace.input, valid);
        assert_ne!(workspace.input, original);
        assert!(workspace.status.starts_with("REJECTED"));
    }

    #[test]
    fn saved_view_round_trips_and_degrades_invalid_state() {
        let mut source = OptionsWorkspace::new();
        source.handle_command(&CommandInvocation {
            function: "OPTIONS".to_owned(),
            args: "NVDA PUT 180 170 60 55 4 0 100"
                .split_whitespace()
                .map(str::to_owned)
                .collect(),
        });
        source.view = OptionsView::Scenarios;
        source.selected_scenario = 10;
        let state = source.capture_view();
        let mut restored = OptionsWorkspace::new();
        let report = restored.restore_view(&state);
        assert!(report.warnings.is_empty());
        assert_eq!(restored.capture_view(), state);
        let mut invalid = state;
        invalid
            .fields
            .insert("spot_micros".to_owned(), ViewValue::Unsigned(0));
        let before = restored.input.clone();
        let report = restored.restore_view(&invalid);
        assert!(!report.warnings.is_empty());
        assert_eq!(restored.input, before);
    }

    #[test]
    fn actions_and_mouse_share_render_geometry() {
        let mut workspace = OptionsWorkspace::new();
        let area = Rect::new(0, 0, 120, 36);
        assert!(workspace.activate_action("view:scenarios"));
        assert_eq!(workspace.view, OptionsView::Scenarios);
        assert!(workspace.activate_action("right:put"));
        assert_eq!(workspace.input.right, OptionRight::Put);
        let footer = options_areas(area).footer;
        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: footer.x,
            row: footer.y,
            modifiers: KeyModifiers::NONE,
        };
        assert!(workspace.handle_mouse(click, area));
        assert!(
            matches!(workspace.poll_intents().as_slice(), [AppIntent::DispatchCommand { command, .. }] if command == "CHART AAPL")
        );
        assert!(workspace
            .actions(area)
            .iter()
            .all(|action| crate::ui::contains(area, action.area.x, action.area.y)));
    }
}
