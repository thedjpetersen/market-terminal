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
    app::{CommandInvocation, Workspace, WorkspaceDescriptor},
    ui::{
        components::terminal_block,
        scroll_key, table_row_at,
        theme::{AMBER, BG, CYAN, GREEN, INK, MUTED, RED, YELLOW},
    },
};

use super::{
    AlertCondition, AlertEvaluation, AlertLifecycle, AlertRule, AlertRuleId, AlertSnapshot,
    AlertStatus, AlertsQuery, DebouncePolicy, InstrumentRef, ID,
};

pub struct AlertsWorkspace {
    query: Arc<dyn AlertsQuery>,
    rules: Vec<AlertRule>,
    selected: usize,
    status: String,
    snapshot_as_of: String,
    snapshot_source: String,
    local_rule_sequence: u64,
}

impl AlertsWorkspace {
    pub fn new(query: Arc<dyn AlertsQuery>) -> Self {
        let mut workspace = Self {
            query,
            rules: Vec::new(),
            selected: 0,
            status: "LOADING LOCAL ALERTS".to_owned(),
            snapshot_as_of: "--".to_owned(),
            snapshot_source: "SIMULATED LOCAL".to_owned(),
            local_rule_sequence: 0,
        };
        workspace.replay();
        workspace
    }

    pub fn rules(&self) -> &[AlertRule] { &self.rules }

    fn replay(&mut self) {
        let snapshot = self.query.load_snapshot();
        self.apply_snapshot(snapshot);
    }

    fn apply_snapshot(&mut self, snapshot: AlertSnapshot) {
        if self.rules.is_empty() {
            self.rules = snapshot.rules;
        } else {
            for rule in snapshot.rules {
                if !self.rules.iter().any(|existing| existing.id == rule.id) {
                    self.rules.push(rule);
                }
            }
        }

        let mut triggered = 0;
        let mut duplicates = 0;
        for observation in &snapshot.observations {
            for rule in &mut self.rules {
                match rule.evaluate(observation) {
                    AlertEvaluation::Triggered(_) => triggered += 1,
                    AlertEvaluation::Duplicate => duplicates += 1,
                    _ => {}
                }
            }
        }
        self.selected = self.selected.min(self.rules.len().saturating_sub(1));
        self.snapshot_as_of = snapshot.as_of;
        self.snapshot_source = snapshot.source;
        self.status = if triggered > 0 {
            format!("REPLAY {} · {triggered} NEW TRIGGER(S)", snapshot.sequence)
        } else if duplicates > 0 {
            format!("REPLAY {} · IDEMPOTENT · NO NEW TRIGGERS", snapshot.sequence)
        } else {
            format!("REPLAY {} · EVALUATED", snapshot.sequence)
        };
    }

    fn move_selection(&mut self, delta: isize) {
        if self.rules.is_empty() {
            self.selected = 0;
            return;
        }
        self.selected = self.selected.saturating_add_signed(delta).min(self.rules.len() - 1);
    }

    fn toggle_selected(&mut self) {
        let Some(rule) = self.rules.get_mut(self.selected) else {
            return;
        };
        rule.toggle(self.snapshot_as_of.clone());
        self.status = format!("{} {}", rule.instrument.symbol, rule.lifecycle.label());
    }

    fn acknowledge_selected(&mut self) {
        let Some(rule) = self.rules.get_mut(self.selected) else {
            return;
        };
        self.status = if rule.acknowledge(self.snapshot_as_of.clone()) {
            format!("{} ACKNOWLEDGED LOCALLY", rule.instrument.symbol)
        } else {
            format!("{} HAS NO UNACKNOWLEDGED TRIGGER", rule.instrument.symbol)
        };
    }

    fn handle_alert_command(&mut self, invocation: &CommandInvocation) {
        if invocation.args.is_empty() {
            self.status = "USE ALERT <SYMBOL> <|> <PRICE> OR ALERT <SYMBOL> MOVE <|> <%>"
                .to_owned();
            return;
        }

        if let Some(condition) = parse_condition(&invocation.args) {
            let symbol = invocation.args[0].trim().to_ascii_uppercase();
            if let Some(index) = self.rules.iter().position(|rule| {
                rule.instrument.symbol.eq_ignore_ascii_case(&symbol) && rule.condition == condition
            }) {
                self.selected = index;
                self.status = format!("EXISTING {symbol} RULE SELECTED");
                return;
            }

            self.local_rule_sequence += 1;
            let canonical_id = canonical_id_for_symbol(&symbol);
            let id = AlertRuleId::new(format!(
                "local:{}:{}",
                symbol.to_ascii_lowercase(),
                self.local_rule_sequence
            ));
            self.rules.push(AlertRule::new(
                id,
                InstrumentRef::new(canonical_id, symbol.clone()),
                condition,
                DebouncePolicy::consecutive(2),
            ));
            self.selected = self.rules.len() - 1;
            self.status = format!("{symbol} RULE CREATED · SIMULATED LOCAL");
            return;
        }

        let symbol = invocation.args.join(" ");
        if let Some(index) = self
            .rules
            .iter()
            .position(|rule| rule.instrument.symbol.eq_ignore_ascii_case(&symbol))
        {
            self.selected = index;
            self.status = format!("{} RULE SELECTED", symbol.to_ascii_uppercase());
        } else {
            self.status = "INVALID ALERT · EXAMPLES: ALERT AAPL > 206 · ALERT NVDA MOVE < -3"
                .to_owned();
        }
    }
}

impl Workspace for AlertsWorkspace {
    fn descriptor(&self) -> WorkspaceDescriptor {
        WorkspaceDescriptor {
            id: ID,
            label: "ALERTS",
            hotkey: '\0',
            commands: &["ALERT", "ALERTS"],
        }
    }

    fn is_favorite(&self) -> bool { true }

    fn handle_command(&mut self, invocation: &CommandInvocation) -> bool {
        if invocation.function.eq_ignore_ascii_case("ALERT") {
            self.handle_alert_command(invocation);
        } else {
            self.status = "ALERT REGISTER · SIMULATED LOCAL DELIVERY".to_owned();
        }
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
            KeyCode::Char(' ') | KeyCode::Char('e') => {
                self.toggle_selected();
                true
            }
            KeyCode::Char('a') => {
                self.acknowledge_selected();
                true
            }
            KeyCode::Char('r') => {
                self.replay();
                true
            }
            _ => false,
        }
    }

    fn handle_mouse(&mut self, event: MouseEvent, area: Rect) -> bool {
        let areas = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(7),
            Constraint::Length(2),
        ])
        .split(area);
        if let Some(index) = table_row_at(event, areas[1], self.rules.len()) {
            self.selected = index;
            return true;
        }
        if crate::ui::is_primary_click(event, areas[3]) {
            let controls = [
                (" ↑↓/JK SELECT  ", None),
                ("SPACE/E ENABLE/DISABLE  ", Some(KeyCode::Char(' '))),
                ("A ACKNOWLEDGE  ", Some(KeyCode::Char('a'))),
                ("R REPLAY EVALUATION  ", Some(KeyCode::Char('r'))),
            ];
            let mut x = areas[3].x;
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
        if let Some(key) = scroll_key(event, areas[1]) {
            return self.handle_key(key);
        }
        false
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let areas = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(7),
            Constraint::Length(2),
        ])
        .split(area);
        let (triggered, acknowledged, disabled) = status_counts(&self.rules);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" ALERTS ", Style::new().bg(AMBER).fg(BG).bold()),
                Span::styled(format!(" {} RULES  ", self.rules.len()), INK),
                Span::styled(format!("TRIGGERED {triggered}  "), RED),
                Span::styled(format!("ACK {acknowledged}  "), GREEN),
                Span::styled(format!("DISABLED {disabled}  "), MUTED),
                Span::styled(&self.status, YELLOW),
            ]))
            .block(terminal_block("ALERTS", "RULE REGISTER")),
            areas[0],
        );

        let header = Row::new([
            "STATE", "SYMBOL", "CONDITION", "LAST", "MOVE", "DEBOUNCE", "DELIVERY",
        ])
        .style(Style::new().fg(AMBER).bold())
        .bottom_margin(1);
        let rows = self.rules.iter().enumerate().map(|(index, rule)| {
            let selected = index == self.selected;
            Row::new(vec![
                Cell::from(display_status(rule)).style(rule_status_style(rule, selected)),
                Cell::from(rule.instrument.symbol.clone()),
                Cell::from(rule.condition.label()),
                Cell::from(
                    rule.last_observation
                        .as_ref()
                        .map(|observation| format!("{:.2}", observation.price))
                        .unwrap_or_else(|| "--".to_owned()),
                ),
                Cell::from(
                    rule.last_observation
                        .as_ref()
                        .map(|observation| format!("{:+.2}%", observation.percent_move))
                        .unwrap_or_else(|| "--".to_owned()),
                ),
                Cell::from(debounce_label(rule)),
                Cell::from(rule.delivery.label()),
            ])
            .style(if selected { Style::new().bg(CYAN).fg(BG).bold() } else { Style::new() })
        });
        frame.render_widget(
            Table::new(
                rows,
                [
                    Constraint::Length(13),
                    Constraint::Length(10),
                    Constraint::Length(20),
                    Constraint::Length(11),
                    Constraint::Length(10),
                    Constraint::Length(11),
                    Constraint::Min(20),
                ],
            )
            .header(header)
            .column_spacing(1)
            .block(terminal_block("RULES", "LOCAL EVALUATION STATE")),
            areas[1],
        );

        frame.render_widget(
            Paragraph::new(selected_detail(&self.rules, self.selected, &self.snapshot_as_of, &self.snapshot_source))
                .wrap(Wrap { trim: true })
                .block(terminal_block("AUDIT", "SELECTED RULE")),
            areas[2],
        );

        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" ↑↓/JK ", AMBER),
                Span::styled("SELECT  ", MUTED),
                Span::styled("SPACE/E ", AMBER),
                Span::styled("ENABLE/DISABLE  ", MUTED),
                Span::styled("A ", AMBER),
                Span::styled("ACKNOWLEDGE  ", MUTED),
                Span::styled("R ", AMBER),
                Span::styled("REPLAY EVALUATION  ", MUTED),
                Span::styled("SIMULATED · LOCAL ONLY · NO EXTERNAL NOTIFICATION", YELLOW),
            ])),
            areas[3],
        );
    }
}

fn parse_condition(args: &[String]) -> Option<AlertCondition> {
    match args {
        [_, operator, value] => {
            let value = value.parse::<f64>().ok()?;
            if !value.is_finite() || value < 0.0 {
                return None;
            }
            match operator.as_str() {
                ">" => Some(AlertCondition::price_above(value)),
                "<" => Some(AlertCondition::price_below(value)),
                _ => None,
            }
        }
        [_, kind, operator, value]
            if kind.eq_ignore_ascii_case("MOVE") || kind == "%" || kind.eq_ignore_ascii_case("PCT") =>
        {
            let value = value.trim_end_matches('%').parse::<f64>().ok()?;
            if !value.is_finite() {
                return None;
            }
            match operator.as_str() {
                ">" => Some(AlertCondition::percent_move_above(value)),
                "<" => Some(AlertCondition::percent_move_below(value)),
                _ => None,
            }
        }
        _ => None,
    }
}

fn canonical_id_for_symbol(symbol: &str) -> String {
    match symbol {
        "SPY" => "us:arcx:spy".to_owned(),
        "SPX" => "index:spx".to_owned(),
        _ => format!("us:xnas:{}", symbol.to_ascii_lowercase()),
    }
}

fn debounce_label(rule: &AlertRule) -> String {
    match &rule.status {
        AlertStatus::Pending { matched, required } => format!("{matched}/{required}"),
        _ => format!("{} TICKS", rule.debounce.confirmations()),
    }
}

fn display_status(rule: &AlertRule) -> &'static str {
    if rule.lifecycle == AlertLifecycle::Disabled {
        "DISABLED"
    } else {
        rule.status.label()
    }
}

fn rule_status_style(rule: &AlertRule, selected: bool) -> Style {
    if rule.lifecycle == AlertLifecycle::Disabled && !selected {
        Style::new().fg(MUTED)
    } else {
        status_style(&rule.status, selected)
    }
}

fn status_style(status: &AlertStatus, selected: bool) -> Style {
    if selected {
        return Style::new().bg(CYAN).fg(BG).bold();
    }
    match status {
        AlertStatus::Armed => Style::new().fg(INK),
        AlertStatus::Pending { .. } => Style::new().fg(YELLOW),
        AlertStatus::Triggered { .. } => Style::new().fg(RED).bold(),
        AlertStatus::Acknowledged { .. } => Style::new().fg(GREEN),
    }
}

fn status_counts(rules: &[AlertRule]) -> (usize, usize, usize) {
    let triggered = rules
        .iter()
        .filter(|rule| matches!(&rule.status, AlertStatus::Triggered { .. }))
        .count();
    let acknowledged = rules
        .iter()
        .filter(|rule| matches!(&rule.status, AlertStatus::Acknowledged { .. }))
        .count();
    let disabled = rules.iter().filter(|rule| rule.lifecycle == AlertLifecycle::Disabled).count();
    (triggered, acknowledged, disabled)
}

fn selected_detail(
    rules: &[AlertRule],
    selected: usize,
    snapshot_as_of: &str,
    snapshot_source: &str,
) -> Vec<Line<'static>> {
    let Some(rule) = rules.get(selected) else {
        return vec![Line::styled("NO ALERT RULES", MUTED)];
    };
    let last_evaluation = rule
        .last_observation
        .as_ref()
        .map(|observation| observation.evaluation_id.clone())
        .unwrap_or_else(|| "NONE".to_owned());
    let audit = rule.audit.last().map_or_else(
        || "NO AUDIT EVENT".to_owned(),
        |entry| format!("{:?} · {} · {}", entry.kind, entry.at, entry.detail),
    );
    vec![
        Line::from(vec![
            Span::styled(format!("{}  ", rule.id), AMBER),
            Span::styled(format!("{}  ", rule.lifecycle.label()), INK),
            Span::styled(format!("{}  ", rule.status.label()), status_style(&rule.status, false)),
            Span::styled(rule.delivery.label(), YELLOW),
        ]),
        Line::styled(
            format!("SNAPSHOT {snapshot_as_of} · {snapshot_source} · EVALUATION {last_evaluation}"),
            MUTED,
        ),
        Line::styled(audit, INK),
        Line::styled("ACKNOWLEDGEMENT CHANGES LOCAL DISPLAY STATE ONLY.", YELLOW),
    ]
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, sync::Mutex};

    use crossterm::event::KeyModifiers;

    use super::*;
    use crate::features::alerts::{AlertObservation, AlertSnapshot};

    struct StubAlerts {
        snapshots: Mutex<VecDeque<AlertSnapshot>>,
    }

    impl AlertsQuery for StubAlerts {
        fn load_snapshot(&self) -> AlertSnapshot {
            self.snapshots.lock().expect("stub snapshots poisoned").pop_front().unwrap_or_else(|| {
                AlertSnapshot::new(99, "2026-08-25T20:00:99Z", Vec::new(), Vec::new(), "STUB")
            })
        }
    }

    fn stub_query(triggered: bool) -> Arc<dyn AlertsQuery> {
        let rule = AlertRule::new(
            AlertRuleId::new("stub:aapl"),
            InstrumentRef::new("us:xnas:aapl", "AAPL"),
            AlertCondition::price_above(205.0),
            DebouncePolicy::consecutive(1),
        );
        let price = if triggered { 206.0 } else { 204.0 };
        Arc::new(StubAlerts {
            snapshots: Mutex::new(VecDeque::from([AlertSnapshot::new(
                0,
                "2026-08-25T20:00:00Z",
                vec![rule],
                vec![AlertObservation::new(
                    "stub-0",
                    "us:xnas:aapl",
                    price,
                    0.5,
                    "2026-08-25T20:00:00Z",
                )],
                "STUB · SIMULATED LOCAL",
            )])),
        })
    }

    #[test]
    fn exposes_exact_alert_command_vocabulary_without_hotkey_collision() {
        let workspace = AlertsWorkspace::new(stub_query(false));

        assert_eq!(workspace.descriptor().commands, &["ALERT", "ALERTS"]);
        assert_eq!(workspace.hotkey(), None);
        assert!(workspace.is_favorite());
    }

    #[test]
    fn keyboard_toggles_and_acknowledges_selected_rule() {
        let mut workspace = AlertsWorkspace::new(stub_query(true));
        assert!(matches!(&workspace.rules[0].status, AlertStatus::Triggered { .. }));

        workspace.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        assert!(matches!(&workspace.rules[0].status, AlertStatus::Acknowledged { .. }));
        workspace.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        assert_eq!(workspace.rules[0].lifecycle, AlertLifecycle::Disabled);
    }

    #[test]
    fn alert_command_creates_price_and_percent_rules_locally() {
        let mut workspace = AlertsWorkspace::new(stub_query(false));
        workspace.handle_command(&CommandInvocation {
            function: "ALERT".to_owned(),
            args: vec!["MSFT".to_owned(), ">".to_owned(), "510".to_owned()],
        });
        workspace.handle_command(&CommandInvocation {
            function: "ALERT".to_owned(),
            args: vec!["NVDA".to_owned(), "MOVE".to_owned(), "<".to_owned(), "-3%".to_owned()],
        });

        assert_eq!(workspace.rules.len(), 3);
        assert_eq!(workspace.rules[1].condition, AlertCondition::price_above(510.0));
        assert_eq!(workspace.rules[2].condition, AlertCondition::percent_move_below(-3.0));
        assert_eq!(workspace.rules[2].delivery.label(), "SIMULATED · LOCAL ONLY");
    }
}
