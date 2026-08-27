use std::sync::{
    mpsc::{channel, sync_channel, Receiver, SyncSender, TrySendError},
    Arc,
};
use std::thread::JoinHandle;

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
        components::terminal_block,
        scroll_key, table_row_at,
        theme::{AMBER, BG, CYAN, GREEN, INK, MUTED, RED, YELLOW},
    },
};

use super::{
    AlertCondition, AlertEvaluation, AlertLifecycle, AlertRule, AlertRuleId, AlertRulesState,
    AlertSnapshot, AlertStateError, AlertStateStore, AlertStatus, AlertsError, AlertsQuery,
    DebouncePolicy, InstrumentRef, ID, MAX_ALERT_RULES,
};

struct AlertsRefresh {
    generation: u64,
    instruments: Vec<InstrumentRef>,
}

struct AlertsRefreshResult {
    generation: u64,
    result: Result<AlertSnapshot, AlertsError>,
}

struct AlertPersistResult {
    revision: u64,
    result: Result<(), AlertStateError>,
}

pub struct AlertsWorkspace {
    rules: Vec<AlertRule>,
    selected: usize,
    status: String,
    snapshot_as_of: String,
    snapshot_source: String,
    local_rule_sequence: u64,
    refresh_sender: SyncSender<AlertsRefresh>,
    refresh_receiver: Receiver<AlertsRefreshResult>,
    pending_refresh: Option<AlertsRefresh>,
    desired_generation: u64,
    state_revision: u64,
    persisted_revision: u64,
    persistence_status: String,
    persist_sender: Option<SyncSender<AlertRulesState>>,
    persist_receiver: Option<Receiver<AlertPersistResult>>,
    pending_persist: Option<AlertRulesState>,
    persist_worker: Option<JoinHandle<()>>,
}

impl AlertsWorkspace {
    pub fn new(query: Arc<dyn AlertsQuery>) -> Self {
        Self::configured(query, None)
    }

    pub fn persistent(query: Arc<dyn AlertsQuery>, state_store: Arc<dyn AlertStateStore>) -> Self {
        Self::configured(query, Some(state_store))
    }

    fn configured(
        query: Arc<dyn AlertsQuery>,
        state_store: Option<Arc<dyn AlertStateStore>>,
    ) -> Self {
        let (rules, state_revision, persisted_revision, persistence_status) =
            match state_store.as_ref().map(|store| store.load_alert_rules()) {
                None => (Vec::new(), 0, 0, "PROCESS-LOCAL STATE".to_owned()),
                Some(Ok(Some(state))) => {
                    let revision = state.revision;
                    let count = state.rules.len();
                    (
                        state.rules,
                        revision,
                        revision,
                        format!("DURABLE R{revision} · RESTORED {count} RULE(S)"),
                    )
                }
                Some(Ok(None)) => (Vec::new(), 0, 0, "DURABLE STATE READY".to_owned()),
                Some(Err(error)) => (
                    Vec::new(),
                    0,
                    0,
                    format!("DURABLE STATE LOAD ERROR · {error}"),
                ),
            };
        let local_rule_sequence = next_local_rule_sequence(&rules);
        let (refresh_sender, worker_receiver) = sync_channel::<AlertsRefresh>(1);
        let (worker_sender, refresh_receiver) = sync_channel::<AlertsRefreshResult>(1);
        std::thread::Builder::new()
            .name("alert-observations".to_owned())
            .spawn(move || {
                while let Ok(mut refresh) = worker_receiver.recv() {
                    while let Ok(newer) = worker_receiver.try_recv() {
                        refresh = newer;
                    }
                    let result = query.load_snapshot(&refresh.instruments);
                    if worker_sender
                        .send(AlertsRefreshResult {
                            generation: refresh.generation,
                            result,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .expect("alert observation worker should start");
        let (persist_sender, persist_receiver, persist_worker) = if let Some(store) = state_store {
            let (persist_sender, worker_receiver) = sync_channel::<AlertRulesState>(1);
            let (worker_sender, persist_receiver) = channel::<AlertPersistResult>();
            let persist_worker = std::thread::Builder::new()
                .name("alert-state-writer".to_owned())
                .spawn(move || {
                    while let Ok(mut state) = worker_receiver.recv() {
                        while let Ok(newer) = worker_receiver.try_recv() {
                            state = newer;
                        }
                        let revision = state.revision;
                        let result = store.save_alert_rules(&state);
                        if worker_sender
                            .send(AlertPersistResult { revision, result })
                            .is_err()
                        {
                            break;
                        }
                    }
                })
                .expect("alert state writer should start");
            (
                Some(persist_sender),
                Some(persist_receiver),
                Some(persist_worker),
            )
        } else {
            (None, None, None)
        };
        let mut workspace = Self {
            rules,
            selected: 0,
            status: "LOADING LOCAL ALERTS".to_owned(),
            snapshot_as_of: "--".to_owned(),
            snapshot_source: "SIMULATED LOCAL".to_owned(),
            local_rule_sequence,
            refresh_sender,
            refresh_receiver,
            pending_refresh: None,
            desired_generation: 0,
            state_revision,
            persisted_revision,
            persistence_status,
            persist_sender,
            persist_receiver,
            pending_persist: None,
            persist_worker,
        };
        workspace.refresh();
        workspace
    }

    pub fn rules(&self) -> &[AlertRule] {
        &self.rules
    }

    fn refresh(&mut self) {
        self.desired_generation = self.desired_generation.wrapping_add(1);
        let mut instruments = Vec::<InstrumentRef>::new();
        for rule in &self.rules {
            if !instruments
                .iter()
                .any(|instrument| instrument.canonical_id == rule.instrument.canonical_id)
            {
                instruments.push(rule.instrument.clone());
            }
        }
        self.pending_refresh = Some(AlertsRefresh {
            generation: self.desired_generation,
            instruments,
        });
        self.status = "LOADING LIVE ALERT OBSERVATIONS…".to_owned();
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
                self.status = "ALERT OBSERVATION WORKER STOPPED".to_owned();
            }
        }
    }

    fn poll_refresh(&mut self) {
        while let Ok(refresh) = self.refresh_receiver.try_recv() {
            if refresh.generation != self.desired_generation {
                continue;
            }
            match refresh.result {
                Ok(snapshot) => self.apply_snapshot(snapshot),
                Err(error) => self.status = error.to_string(),
            }
        }
        self.dispatch_pending_refresh();
    }

    fn apply_snapshot(&mut self, snapshot: AlertSnapshot) {
        let mut state_changed = false;
        if self.rules.is_empty() {
            let available = MAX_ALERT_RULES.saturating_sub(self.rules.len());
            let additions = snapshot
                .rules
                .into_iter()
                .take(available)
                .collect::<Vec<_>>();
            state_changed |= !additions.is_empty();
            self.rules = additions;
        } else {
            for rule in snapshot.rules {
                if self.rules.len() < MAX_ALERT_RULES
                    && !self.rules.iter().any(|existing| existing.id == rule.id)
                {
                    self.rules.push(rule);
                    state_changed = true;
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
                    AlertEvaluation::NotApplicable => {}
                    _ => state_changed = true,
                }
            }
        }
        if state_changed {
            self.queue_persist();
        }
        self.selected = self.selected.min(self.rules.len().saturating_sub(1));
        self.snapshot_as_of = snapshot.as_of;
        self.snapshot_source = snapshot.source;
        self.status = if triggered > 0 {
            format!(
                "SNAPSHOT {} · {triggered} NEW TRIGGER(S)",
                snapshot.sequence
            )
        } else if duplicates > 0 {
            format!(
                "SNAPSHOT {} · IDEMPOTENT · NO NEW TRIGGERS",
                snapshot.sequence
            )
        } else {
            format!("SNAPSHOT {} · EVALUATED", snapshot.sequence)
        };
    }

    fn move_selection(&mut self, delta: isize) {
        if self.rules.is_empty() {
            self.selected = 0;
            return;
        }
        self.selected = self
            .selected
            .saturating_add_signed(delta)
            .min(self.rules.len() - 1);
    }

    fn toggle_selected(&mut self) {
        let Some(rule) = self.rules.get_mut(self.selected) else {
            return;
        };
        rule.toggle(self.snapshot_as_of.clone());
        let status = format!("{} {}", rule.instrument.symbol, rule.lifecycle.label());
        self.status = status;
        self.queue_persist();
    }

    fn acknowledge_selected(&mut self) {
        let Some(rule) = self.rules.get_mut(self.selected) else {
            return;
        };
        let changed = rule.acknowledge(self.snapshot_as_of.clone());
        self.status = if changed {
            format!("{} ACKNOWLEDGED LOCALLY", rule.instrument.symbol)
        } else {
            format!("{} HAS NO UNACKNOWLEDGED TRIGGER", rule.instrument.symbol)
        };
        if changed {
            self.queue_persist();
        }
    }

    fn handle_alert_command(&mut self, invocation: &CommandInvocation) {
        if invocation.args.is_empty() {
            self.status =
                "USE ALERT <SYMBOL> <|> <PRICE> OR ALERT <SYMBOL> MOVE <|> <%>".to_owned();
            return;
        }

        if let Some(condition) = parse_condition(&invocation.args) {
            let symbol = invocation.args[0].trim().to_ascii_uppercase();
            if !valid_alert_symbol(&symbol) {
                self.status =
                    "INVALID ALERT SYMBOL · USE 1-32 LETTERS, DIGITS, . - / ^ OR _".to_owned();
                return;
            }
            if let Some(index) = self.rules.iter().position(|rule| {
                rule.instrument.symbol.eq_ignore_ascii_case(&symbol) && rule.condition == condition
            }) {
                self.selected = index;
                self.status = format!("EXISTING {symbol} RULE SELECTED");
                return;
            }

            if self.rules.len() >= MAX_ALERT_RULES {
                self.status = format!("ALERT RULE LIMIT REACHED · MAXIMUM {MAX_ALERT_RULES}");
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
            self.status =
                "INVALID ALERT · EXAMPLES: ALERT AAPL > 206 · ALERT NVDA MOVE < -3".to_owned();
        }
    }

    fn queue_persist(&mut self) {
        if self.persist_sender.is_none() {
            return;
        }
        self.state_revision = self.state_revision.saturating_add(1);
        match AlertRulesState::new(self.state_revision, self.rules.clone()) {
            Ok(state) => {
                self.pending_persist = Some(state);
                self.dispatch_pending_persist();
            }
            Err(error) => {
                self.persistence_status = format!("DURABLE STATE ERROR · {error}");
            }
        }
    }

    fn dispatch_pending_persist(&mut self) {
        let Some(state) = self.pending_persist.take() else {
            return;
        };
        let Some(sender) = &self.persist_sender else {
            return;
        };
        match sender.try_send(state) {
            Ok(()) => {}
            Err(TrySendError::Full(state)) => self.pending_persist = Some(state),
            Err(TrySendError::Disconnected(_)) => {
                self.persistence_status = "DURABLE STATE WRITER STOPPED".to_owned();
            }
        }
    }

    fn poll_persistence(&mut self) {
        if let Some(receiver) = &self.persist_receiver {
            while let Ok(result) = receiver.try_recv() {
                match result.result {
                    Ok(()) => {
                        self.persisted_revision = self.persisted_revision.max(result.revision);
                        self.persistence_status =
                            format!("DURABLE R{} · SAVED", self.persisted_revision);
                    }
                    Err(error) => {
                        self.persistence_status = format!("DURABLE STATE SAVE ERROR · {error}");
                    }
                }
            }
        }
        self.dispatch_pending_persist();
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

    fn is_favorite(&self) -> bool {
        true
    }

    fn handle_command(&mut self, invocation: &CommandInvocation) -> bool {
        if invocation.function.eq_ignore_ascii_case("ALERT") {
            let rule_count = self.rules.len();
            self.handle_alert_command(invocation);
            if self.rules.len() > rule_count {
                self.queue_persist();
                self.refresh();
            }
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
                self.refresh();
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
        if crate::ui::is_primary_click(event, areas[0]) {
            return self.handle_key(KeyEvent::new(
                KeyCode::Char('r'),
                crossterm::event::KeyModifiers::NONE,
            ));
        }
        if let Some(index) = table_row_at(event, areas[1], self.rules.len()) {
            self.selected = index;
            return true;
        }
        if crate::ui::is_primary_click(event, areas[3]) {
            let controls = [
                (" ↑↓/JK SELECT  ", None),
                ("SPACE/E ENABLE/DISABLE  ", Some(KeyCode::Char(' '))),
                ("A ACKNOWLEDGE  ", Some(KeyCode::Char('a'))),
                ("R REFRESH LIVE EVALUATION  ", Some(KeyCode::Char('r'))),
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

    fn poll_intents(&mut self) -> Vec<AppIntent> {
        self.poll_refresh();
        self.poll_persistence();
        Vec::new()
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
                Span::styled(
                    " ALERTS ",
                    Style::new().bg(AMBER.into()).fg(BG.into()).bold(),
                ),
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
            "STATE",
            "SYMBOL",
            "CONDITION",
            "LAST",
            "MOVE",
            "DEBOUNCE",
            "DELIVERY",
        ])
        .style(Style::new().fg(AMBER.into()).bold())
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
            .style(if selected {
                Style::new().bg(CYAN.into()).fg(BG.into()).bold()
            } else {
                Style::new()
            })
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
            Paragraph::new(selected_detail(
                &self.rules,
                self.selected,
                &self.snapshot_as_of,
                &self.snapshot_source,
                &self.persistence_status,
            ))
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
                Span::styled("REFRESH LIVE EVALUATION  ", MUTED),
                Span::styled("SIMULATED · LOCAL ONLY · NO EXTERNAL NOTIFICATION", YELLOW),
            ])),
            areas[3],
        );
    }
}

impl Drop for AlertsWorkspace {
    fn drop(&mut self) {
        if let (Some(sender), Some(state)) = (&self.persist_sender, self.pending_persist.take()) {
            let _ = sender.send(state);
        }
        self.persist_sender.take();
        if let Some(worker) = self.persist_worker.take() {
            let _ = worker.join();
        }
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
            if kind.eq_ignore_ascii_case("MOVE")
                || kind == "%"
                || kind.eq_ignore_ascii_case("PCT") =>
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
        "SPX" => "index:spx".to_owned(),
        _ => format!("us:listed:{}", symbol.to_ascii_lowercase()),
    }
}

fn valid_alert_symbol(symbol: &str) -> bool {
    !symbol.is_empty()
        && symbol.len() <= 32
        && symbol
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphanumeric() || character == '^')
        && !symbol.contains("..")
        && symbol.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '/' | '^' | '_')
        })
}

fn next_local_rule_sequence(rules: &[AlertRule]) -> u64 {
    rules
        .iter()
        .filter_map(|rule| {
            rule.id
                .as_str()
                .strip_prefix("local:")?
                .rsplit(':')
                .next()?
                .parse::<u64>()
                .ok()
        })
        .max()
        .unwrap_or(0)
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
        Style::new().fg(MUTED.into())
    } else {
        status_style(&rule.status, selected)
    }
}

fn status_style(status: &AlertStatus, selected: bool) -> Style {
    if selected {
        return Style::new().bg(CYAN.into()).fg(BG.into()).bold();
    }
    match status {
        AlertStatus::Armed => Style::new().fg(INK.into()),
        AlertStatus::Pending { .. } => Style::new().fg(YELLOW.into()),
        AlertStatus::Triggered { .. } => Style::new().fg(RED.into()).bold(),
        AlertStatus::Acknowledged { .. } => Style::new().fg(GREEN.into()),
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
    let disabled = rules
        .iter()
        .filter(|rule| rule.lifecycle == AlertLifecycle::Disabled)
        .count();
    (triggered, acknowledged, disabled)
}

fn selected_detail(
    rules: &[AlertRule],
    selected: usize,
    snapshot_as_of: &str,
    snapshot_source: &str,
    persistence_status: &str,
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
            Span::styled(
                format!("{}  ", rule.status.label()),
                status_style(&rule.status, false),
            ),
            Span::styled(rule.delivery.label(), YELLOW),
        ]),
        Line::styled(
            format!("SNAPSHOT {snapshot_as_of} · {snapshot_source} · EVALUATION {last_evaluation}"),
            MUTED,
        ),
        Line::styled(audit, INK),
        Line::styled(persistence_status.to_owned(), CYAN),
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

    #[derive(Default)]
    struct MemoryAlertState {
        state: Mutex<Option<AlertRulesState>>,
    }

    impl AlertStateStore for MemoryAlertState {
        fn load_alert_rules(&self) -> Result<Option<AlertRulesState>, AlertStateError> {
            Ok(self
                .state
                .lock()
                .expect("memory alert state poisoned")
                .clone())
        }

        fn save_alert_rules(&self, state: &AlertRulesState) -> Result<(), AlertStateError> {
            *self.state.lock().expect("memory alert state poisoned") = Some(state.clone());
            Ok(())
        }
    }

    impl AlertsQuery for StubAlerts {
        fn load_snapshot(
            &self,
            _instruments: &[InstrumentRef],
        ) -> Result<AlertSnapshot, AlertsError> {
            Ok(self
                .snapshots
                .lock()
                .expect("stub snapshots poisoned")
                .pop_front()
                .unwrap_or_else(|| {
                    AlertSnapshot::new(99, "2026-08-25T20:00:99Z", Vec::new(), Vec::new(), "STUB")
                }))
        }
    }

    fn settle(workspace: &mut AlertsWorkspace) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        while workspace.rules.is_empty() && std::time::Instant::now() < deadline {
            workspace.poll_refresh();
            std::thread::yield_now();
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
        let mut workspace = AlertsWorkspace::new(stub_query(false));
        settle(&mut workspace);

        assert_eq!(workspace.descriptor().commands, &["ALERT", "ALERTS"]);
        assert_eq!(workspace.hotkey(), None);
        assert!(workspace.is_favorite());
    }

    #[test]
    fn keyboard_toggles_and_acknowledges_selected_rule() {
        let mut workspace = AlertsWorkspace::new(stub_query(true));
        settle(&mut workspace);
        assert!(matches!(
            &workspace.rules[0].status,
            AlertStatus::Triggered { .. }
        ));

        workspace.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        assert!(matches!(
            &workspace.rules[0].status,
            AlertStatus::Acknowledged { .. }
        ));
        workspace.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        assert_eq!(workspace.rules[0].lifecycle, AlertLifecycle::Disabled);
    }

    #[test]
    fn alert_command_creates_price_and_percent_rules_locally() {
        let mut workspace = AlertsWorkspace::new(stub_query(false));
        settle(&mut workspace);
        workspace.handle_command(&CommandInvocation {
            function: "ALERT".to_owned(),
            args: vec!["MSFT".to_owned(), ">".to_owned(), "510".to_owned()],
        });
        workspace.handle_command(&CommandInvocation {
            function: "ALERT".to_owned(),
            args: vec![
                "NVDA".to_owned(),
                "MOVE".to_owned(),
                "<".to_owned(),
                "-3%".to_owned(),
            ],
        });

        assert_eq!(workspace.rules.len(), 3);
        assert_eq!(
            workspace.rules[1].condition,
            AlertCondition::price_above(510.0)
        );
        assert_eq!(
            workspace.rules[2].condition,
            AlertCondition::percent_move_below(-3.0)
        );
        assert_eq!(
            workspace.rules[2].delivery.label(),
            "SIMULATED · LOCAL ONLY"
        );

        let rule_count = workspace.rules.len();
        workspace.handle_command(&CommandInvocation {
            function: "ALERT".to_owned(),
            args: vec!["../IBM".to_owned(), ">".to_owned(), "1".to_owned()],
        });
        assert_eq!(workspace.rules.len(), rule_count);
        assert!(workspace.status.contains("INVALID ALERT SYMBOL"));
    }

    #[test]
    fn complete_rule_state_and_sequence_survive_workspace_restart() {
        let observation = AlertObservation::new(
            "provider:aapl:one",
            "us:xnas:aapl",
            206.0,
            0.5,
            "2026-08-25T20:00:00Z",
        );
        let mut rule = AlertRule::new(
            AlertRuleId::new("local:aapl:7"),
            InstrumentRef::new("us:xnas:aapl", "AAPL"),
            AlertCondition::price_above(205.0),
            DebouncePolicy::consecutive(2),
        );
        assert_eq!(
            rule.evaluate(&observation),
            AlertEvaluation::Pending {
                matched: 1,
                required: 2
            }
        );
        let store = Arc::new(MemoryAlertState {
            state: Mutex::new(Some(AlertRulesState::new(4, vec![rule]).unwrap())),
        });
        let query = Arc::new(StubAlerts {
            snapshots: Mutex::new(VecDeque::from([AlertSnapshot::new(
                1,
                "2026-08-25T20:00:00Z",
                Vec::new(),
                vec![observation],
                "REAL-SHAPED PROVIDER SNAPSHOT",
            )])),
        });
        let mut workspace = AlertsWorkspace::persistent(query, store.clone());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        while !workspace.status.contains("IDEMPOTENT") && std::time::Instant::now() < deadline {
            workspace.poll_refresh();
            std::thread::yield_now();
        }

        assert!(matches!(
            workspace.rules[0].status,
            AlertStatus::Pending {
                matched: 1,
                required: 2
            }
        ));
        assert!(workspace.rules[0].audit.is_empty());
        workspace.handle_command(&CommandInvocation {
            function: "ALERT".to_owned(),
            args: vec!["MSFT".to_owned(), ">".to_owned(), "500".to_owned()],
        });
        drop(workspace);

        let saved = store.load_alert_rules().unwrap().unwrap();
        assert_eq!(saved.rules.len(), 2);
        assert!(saved.revision > 4);
        assert_eq!(saved.rules[1].id.as_str(), "local:msft:8");
        assert_eq!(
            saved.rules[0].clone().evaluate(&AlertObservation::new(
                "provider:aapl:one",
                "us:xnas:aapl",
                206.0,
                0.5,
                "2026-08-25T20:00:00Z",
            )),
            AlertEvaluation::Duplicate
        );
    }

    #[test]
    fn slow_durable_writes_do_not_block_alert_commands() {
        struct SlowAlertState;

        impl AlertStateStore for SlowAlertState {
            fn load_alert_rules(&self) -> Result<Option<AlertRulesState>, AlertStateError> {
                Ok(None)
            }

            fn save_alert_rules(&self, _state: &AlertRulesState) -> Result<(), AlertStateError> {
                std::thread::sleep(std::time::Duration::from_millis(200));
                Ok(())
            }
        }

        let query = Arc::new(StubAlerts {
            snapshots: Mutex::new(VecDeque::from([AlertSnapshot::new(
                0,
                "2026-08-25T20:00:00Z",
                Vec::new(),
                Vec::new(),
                "TEST",
            )])),
        });
        let mut workspace = AlertsWorkspace::persistent(query, Arc::new(SlowAlertState));
        let started = std::time::Instant::now();
        workspace.handle_command(&CommandInvocation {
            function: "ALERT".to_owned(),
            args: vec!["IBM".to_owned(), ">".to_owned(), "250".to_owned()],
        });

        assert!(started.elapsed() < std::time::Duration::from_millis(100));
        assert_eq!(workspace.rules.len(), 1);
    }

    #[test]
    fn alert_provider_never_blocks_workspace_construction() {
        struct SlowAlerts;

        impl AlertsQuery for SlowAlerts {
            fn load_snapshot(
                &self,
                _instruments: &[InstrumentRef],
            ) -> Result<AlertSnapshot, AlertsError> {
                std::thread::sleep(std::time::Duration::from_millis(200));
                Ok(AlertSnapshot::new(
                    0,
                    "2026-08-25T20:00:00Z",
                    Vec::new(),
                    Vec::new(),
                    "TEST",
                ))
            }
        }

        let started = std::time::Instant::now();
        let workspace = AlertsWorkspace::new(Arc::new(SlowAlerts));
        assert!(started.elapsed() < std::time::Duration::from_millis(100));
        assert!(workspace.rules.is_empty());
    }
}
