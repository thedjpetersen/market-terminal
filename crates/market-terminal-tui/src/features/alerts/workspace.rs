use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use std::{
    cell::Cell as StateCell,
    sync::{
        mpsc::{channel, sync_channel, Receiver, SyncSender, TrySendError},
        Arc,
    },
};

use crossterm::event::{KeyCode, KeyEvent, MouseEvent};
use ratatui::{
    layout::{Constraint, Rect},
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
        contains, is_primary_click, scroll_key,
        theme::{AMBER, BG, CYAN, GREEN, INK, MUTED, RED, YELLOW},
    },
};

use super::{
    controls::{alert_areas, pack_control_areas, panel_header_area, table_row_area, AlertControl},
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
    viewport_top: StateCell<usize>,
    viewport_rows: StateCell<usize>,
    pending_selected_rule_id: Option<String>,
    pending_top_rule_id: Option<String>,
    status: String,
    snapshot_as_of: String,
    snapshot_source: String,
    local_rule_sequence: u64,
    refresh_sender: SyncSender<AlertsRefresh>,
    refresh_receiver: Receiver<AlertsRefreshResult>,
    pending_refresh: Option<AlertsRefresh>,
    refresh_in_flight: bool,
    refresh_interval: Duration,
    next_refresh: Instant,
    desired_generation: u64,
    state_revision: u64,
    persisted_revision: u64,
    persistence_status: String,
    persist_sender: Option<SyncSender<AlertRulesState>>,
    persist_receiver: Option<Receiver<AlertPersistResult>>,
    pending_persist: Option<AlertRulesState>,
    persist_worker: Option<JoinHandle<()>>,
    pending_intents: Vec<AppIntent>,
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
            viewport_top: StateCell::new(0),
            viewport_rows: StateCell::new(0),
            pending_selected_rule_id: None,
            pending_top_rule_id: None,
            status: "LOADING LOCAL ALERTS".to_owned(),
            snapshot_as_of: "--".to_owned(),
            snapshot_source: "SIMULATED LOCAL".to_owned(),
            local_rule_sequence,
            refresh_sender,
            refresh_receiver,
            pending_refresh: None,
            refresh_in_flight: false,
            refresh_interval: Duration::from_secs(60),
            next_refresh: Instant::now(),
            desired_generation: 0,
            state_revision,
            persisted_revision,
            persistence_status,
            persist_sender,
            persist_receiver,
            pending_persist: None,
            persist_worker,
            pending_intents: Vec::new(),
        };
        workspace.refresh();
        workspace
    }

    pub fn rules(&self) -> &[AlertRule] {
        &self.rules
    }

    pub fn with_refresh_interval(mut self, interval: Duration) -> Self {
        self.refresh_interval = interval.max(Duration::from_secs(1));
        self
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
        if self.refresh_in_flight {
            return;
        }
        let Some(refresh) = self.pending_refresh.take() else {
            return;
        };
        match self.refresh_sender.try_send(refresh) {
            Ok(()) => self.refresh_in_flight = true,
            Err(TrySendError::Full(refresh)) => self.pending_refresh = Some(refresh),
            Err(TrySendError::Disconnected(_)) => {
                self.status = "ALERT OBSERVATION WORKER STOPPED".to_owned();
            }
        }
    }

    fn poll_refresh(&mut self) {
        self.poll_refresh_at(Instant::now());
    }

    fn poll_refresh_at(&mut self, now: Instant) {
        while let Ok(refresh) = self.refresh_receiver.try_recv() {
            self.refresh_in_flight = false;
            self.next_refresh = now + self.refresh_interval;
            if refresh.generation != self.desired_generation {
                continue;
            }
            match refresh.result {
                Ok(snapshot) => self.apply_snapshot(snapshot),
                Err(error) => self.status = error.to_string(),
            }
        }
        self.dispatch_pending_refresh();
        if !self.refresh_in_flight && self.pending_refresh.is_none() && now >= self.next_refresh {
            self.next_refresh = now + self.refresh_interval;
            self.refresh();
        }
    }

    fn apply_snapshot(&mut self, snapshot: AlertSnapshot) {
        let selected_rule_id = self
            .selected_rule()
            .map(|rule| rule.id.as_str().to_owned())
            .or_else(|| self.pending_selected_rule_id.clone());
        let top_rule_id = self
            .rules
            .get(self.viewport_top.get())
            .map(|rule| rule.id.as_str().to_owned())
            .or_else(|| self.pending_top_rule_id.clone());
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
                    AlertEvaluation::Triggered(_) => {
                        triggered += 1;
                        state_changed = true;
                    }
                    AlertEvaluation::Duplicate => duplicates += 1,
                    AlertEvaluation::NotApplicable => {}
                    _ => state_changed = true,
                }
            }
        }
        if state_changed {
            self.queue_persist();
        }
        self.restore_rule_anchors(selected_rule_id.as_deref(), top_rule_id.as_deref());
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
        self.pending_selected_rule_id = None;
        if self.rules.is_empty() {
            self.selected = 0;
            return;
        }
        self.selected = self
            .selected
            .saturating_add_signed(delta)
            .min(self.rules.len() - 1);
        self.reveal_selection();
    }

    fn reveal_selection(&self) {
        let capacity = self.viewport_rows.get();
        if capacity == 0 {
            return;
        }
        let mut top = self.viewport_top.get();
        if self.selected < top {
            top = self.selected;
        } else if self.selected >= top.saturating_add(capacity) {
            top = self.selected.saturating_add(1).saturating_sub(capacity);
        }
        self.viewport_top
            .set(top.min(self.rules.len().saturating_sub(capacity.max(1))));
    }

    fn update_viewport(&self, area: Rect) -> (usize, usize) {
        let capacity = usize::from(area.height.saturating_sub(4));
        self.viewport_rows.set(capacity);
        self.reveal_selection();
        (self.viewport_top.get(), capacity)
    }

    fn rule_index(&self, id: &str) -> Option<usize> {
        self.rules.iter().position(|rule| rule.id.as_str() == id)
    }

    fn restore_rule_anchors(&mut self, selected_id: Option<&str>, top_id: Option<&str>) {
        self.selected = selected_id
            .and_then(|id| self.rule_index(id))
            .unwrap_or_default();
        self.viewport_top.set(
            top_id
                .and_then(|id| self.rule_index(id))
                .unwrap_or_default(),
        );
        self.pending_selected_rule_id = selected_id
            .filter(|id| self.rule_index(id).is_none() && self.rules.is_empty())
            .map(str::to_owned);
        self.pending_top_rule_id = top_id
            .filter(|id| self.rule_index(id).is_none() && self.rules.is_empty())
            .map(str::to_owned);
        self.reveal_selection();
    }

    fn selected_rule(&self) -> Option<&AlertRule> {
        self.rules.get(self.selected)
    }

    fn control_text(&self, control: AlertControl) -> String {
        match control {
            AlertControl::Toggle => match self.selected_rule().map(|rule| rule.lifecycle) {
                Some(AlertLifecycle::Enabled) => " SPACE/E DISABLE ".to_owned(),
                Some(AlertLifecycle::Disabled) => " SPACE/E ENABLE ".to_owned(),
                None => " SPACE/E ENABLE ".to_owned(),
            },
            AlertControl::Acknowledge => " A ACKNOWLEDGE ".to_owned(),
            AlertControl::Security => " S SECURITY ".to_owned(),
            AlertControl::Refresh => " R REFRESH ".to_owned(),
        }
    }

    fn control_action_label(&self, control: AlertControl) -> String {
        let selected = self.selected_rule();
        match control {
            AlertControl::Toggle => selected.map_or_else(
                || "Enable or disable the selected alert".to_owned(),
                |rule| {
                    let verb = if rule.lifecycle == AlertLifecycle::Enabled {
                        "Disable"
                    } else {
                        "Enable"
                    };
                    format!("{verb} {} alert", rule.instrument.symbol)
                },
            ),
            AlertControl::Acknowledge => selected.map_or_else(
                || "Acknowledge the selected alert".to_owned(),
                |rule| format!("Acknowledge {} trigger", rule.instrument.symbol),
            ),
            AlertControl::Security => selected.map_or_else(
                || "Open selected alert instrument in Security".to_owned(),
                |rule| format!("Open {} security research", rule.instrument.symbol),
            ),
            AlertControl::Refresh => "Refresh live alert evaluation".to_owned(),
        }
    }

    fn control_enabled(&self, control: AlertControl) -> bool {
        match control {
            AlertControl::Toggle | AlertControl::Security => self.selected_rule().is_some(),
            AlertControl::Acknowledge => self
                .selected_rule()
                .is_some_and(|rule| matches!(rule.status, AlertStatus::Triggered { .. })),
            AlertControl::Refresh => true,
        }
    }

    fn control_areas(&self, area: Rect) -> Vec<(AlertControl, Rect)> {
        pack_control_areas(
            area,
            AlertControl::ALL.into_iter().map(|control| {
                let width = self.control_text(control).chars().count() as u16;
                (control, width)
            }),
        )
    }

    fn activate_control(&mut self, control: AlertControl) -> bool {
        if !self.control_enabled(control) {
            return false;
        }
        match control {
            AlertControl::Toggle => self.toggle_selected(),
            AlertControl::Acknowledge => self.acknowledge_selected(),
            AlertControl::Security => {
                let Some(symbol) = self
                    .selected_rule()
                    .map(|rule| rule.instrument.symbol.clone())
                else {
                    return false;
                };
                self.pending_intents.push(AppIntent::DispatchCommand {
                    command: format!("SEC {symbol} US"),
                    origin: ID,
                });
            }
            AlertControl::Refresh => self.refresh(),
        }
        true
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
                self.pending_selected_rule_id = None;
                self.reveal_selection();
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
            self.pending_selected_rule_id = None;
            self.reveal_selection();
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
            self.pending_selected_rule_id = None;
            self.reveal_selection();
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
                let _ = self.activate_control(AlertControl::Toggle);
                true
            }
            KeyCode::Char('a') => {
                let _ = self.activate_control(AlertControl::Acknowledge);
                true
            }
            KeyCode::Char('s') => self.activate_control(AlertControl::Security),
            KeyCode::Char('r') => self.activate_control(AlertControl::Refresh),
            _ => false,
        }
    }

    fn handle_mouse(&mut self, event: MouseEvent, area: Rect) -> bool {
        let areas = alert_areas(area);
        if is_primary_click(event, area) {
            for action in self.actions(area) {
                if action.enabled && contains(action.area, event.column, event.row) {
                    return self.activate_action(&action.id);
                }
            }
        }
        if let Some(key) = scroll_key(event, areas.rules) {
            return self.handle_key(key);
        }
        false
    }

    fn actions(&self, area: Rect) -> Vec<WorkspaceAction> {
        let areas = alert_areas(area);
        let (viewport_top, viewport_rows) = self.update_viewport(areas.rules);
        let visible_rows = viewport_rows.min(self.rules.len().saturating_sub(viewport_top));
        let preferred_row = (self.selected >= viewport_top
            && self.selected < viewport_top.saturating_add(visible_rows))
        .then_some(self.selected)
        .or_else(|| (visible_rows > 0).then_some(viewport_top));
        let mut actions = self
            .rules
            .iter()
            .skip(viewport_top)
            .take(visible_rows)
            .enumerate()
            .filter_map(|(ordinal, rule)| {
                let index = viewport_top.saturating_add(ordinal);
                let area = table_row_area(areas.rules, ordinal)?;
                let mut action = WorkspaceAction::new(
                    format!("rule:{index}:{}", rule.id),
                    format!(
                        "Select {} {} alert",
                        rule.instrument.symbol,
                        rule.condition.label()
                    ),
                    area,
                );
                if preferred_row == Some(index) {
                    action = action.preferred();
                }
                Some(action)
            })
            .collect::<Vec<_>>();
        actions.extend(
            self.control_areas(areas.controls)
                .into_iter()
                .map(|(control, area)| {
                    let mut action = WorkspaceAction::new(
                        control.action_id(),
                        self.control_action_label(control),
                        area,
                    );
                    if !self.control_enabled(control) {
                        action = action.disabled();
                    }
                    action
                }),
        );
        let mut refresh = WorkspaceAction::new(
            "control:refresh-header",
            "Refresh live alert evaluation from the header",
            panel_header_area(areas.header),
        );
        if preferred_row.is_none() {
            refresh = refresh.preferred();
        }
        actions.push(refresh);
        actions
    }

    fn activate_action(&mut self, id: &str) -> bool {
        if id == "control:refresh-header" {
            return self.activate_control(AlertControl::Refresh);
        }
        if let Some(control) = AlertControl::from_action_id(id) {
            return self.activate_control(control);
        }
        let Some(rule) = id.strip_prefix("rule:") else {
            return false;
        };
        let Some((index, expected_id)) = rule.split_once(':') else {
            return false;
        };
        let Ok(index) = index.parse::<usize>() else {
            return false;
        };
        if self.rules.get(index).map(|rule| rule.id.as_str()) != Some(expected_id) {
            return false;
        }
        self.selected = index;
        self.pending_selected_rule_id = None;
        self.reveal_selection();
        true
    }

    fn poll_intents(&mut self) -> Vec<AppIntent> {
        self.poll_refresh();
        self.poll_persistence();
        std::mem::take(&mut self.pending_intents)
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let areas = alert_areas(area);
        let (viewport_top, viewport_rows) = self.update_viewport(areas.rules);
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
            areas.header,
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
        let rows = self
            .rules
            .iter()
            .enumerate()
            .skip(viewport_top)
            .take(viewport_rows)
            .map(|(index, rule)| {
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
            areas.rules,
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
            areas.audit,
        );

        for (control, control_area) in self.control_areas(areas.controls) {
            let style = if self.control_enabled(control) {
                Style::new().fg(AMBER.into())
            } else {
                Style::new().fg(MUTED.into())
            };
            frame.render_widget(
                Paragraph::new(self.control_text(control)).style(style),
                control_area,
            );
        }
        frame.render_widget(
            Paragraph::new(" SIMULATED · LOCAL ONLY · NO EXTERNAL NOTIFICATION").style(YELLOW),
            areas.disclosure,
        );
    }

    fn capture_view(&self) -> WorkspaceViewState {
        let mut state = WorkspaceViewState::new(ID.as_str());
        if let Some(id) = self
            .selected_rule()
            .map(|rule| rule.id.as_str().to_owned())
            .or_else(|| self.pending_selected_rule_id.clone())
        {
            state = state.with_field("selected_rule_id", ViewValue::Text(id));
        }
        if let Some(id) = self
            .rules
            .get(self.viewport_top.get())
            .map(|rule| rule.id.as_str().to_owned())
            .or_else(|| self.pending_top_rule_id.clone())
        {
            state = state.with_field("top_rule_id", ViewValue::Text(id));
        }
        state
    }

    fn restore_view(&mut self, state: &WorkspaceViewState) -> ViewRestoreReport {
        if !state.workspace.eq_ignore_ascii_case(ID.as_str()) {
            return ViewRestoreReport::warning(format!(
                "saved state belongs to {}, not alerts",
                state.workspace
            ));
        }

        let mut report = ViewRestoreReport::default();
        let selected = restore_alert_rule_field(
            &self.rules,
            state,
            "selected_rule_id",
            "selected rule",
            &mut report,
        );
        let top = restore_alert_rule_field(
            &self.rules,
            state,
            "top_rule_id",
            "viewport anchor",
            &mut report,
        );
        self.restore_rule_anchors(selected.as_deref(), top.as_deref());

        const KNOWN_FIELDS: [&str; 2] = ["selected_rule_id", "top_rule_id"];
        let unknown = state
            .fields
            .keys()
            .filter(|field| !KNOWN_FIELDS.contains(&field.as_str()))
            .count();
        if unknown > 0 {
            report.skipped_fields += unknown;
            report
                .warnings
                .push(format!("ignored {unknown} future Alerts field(s)"));
        }
        if !state.children.is_empty() {
            report.skipped_fields += state.children.len();
            report.warnings.push(format!(
                "ignored {} future Alerts child state(s)",
                state.children.len()
            ));
        }
        report
    }
}

fn restore_alert_rule_field(
    rules: &[AlertRule],
    state: &WorkspaceViewState,
    field: &str,
    label: &str,
    report: &mut ViewRestoreReport,
) -> Option<String> {
    let value = state.fields.get(field)?;
    let Some(id) = value.as_text().filter(|id| valid_alert_rule_id(id)) else {
        report.skipped_fields += 1;
        report
            .warnings
            .push(format!("saved Alerts {label} identity is invalid"));
        return None;
    };
    if rules.is_empty() || rules.iter().any(|rule| rule.id.as_str() == id) {
        report.restored_fields += 1;
        Some(id.to_owned())
    } else {
        report.skipped_fields += 1;
        report
            .warnings
            .push(format!("saved Alerts {label} is no longer available"));
        None
    }
}

fn valid_alert_rule_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 512
        && value.trim() == value
        && !value.chars().any(char::is_control)
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
    use std::{
        collections::{HashSet, VecDeque},
        sync::Mutex,
    };

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

    fn click(column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
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

    fn test_rule(index: usize) -> AlertRule {
        AlertRule::new(
            AlertRuleId::new(format!("test:rule:{index:03}")),
            InstrumentRef::new(format!("us:listed:test{index:03}"), format!("T{index:03}")),
            AlertCondition::price_above(100.0 + index as f64),
            DebouncePolicy::consecutive(1),
        )
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
    fn actions_share_geometry_revalidate_rule_ids_and_route_security() {
        let area = Rect::new(0, 0, 120, 36);
        let mut workspace = AlertsWorkspace::new(stub_query(true));
        settle(&mut workspace);

        let actions = workspace.actions(area);
        let ids = actions
            .iter()
            .map(|action| action.id.as_str())
            .collect::<HashSet<_>>();
        assert_eq!(ids.len(), actions.len());
        assert!(actions.iter().all(|action| {
            action.area.width > 0
                && action.area.height > 0
                && action.area.x >= area.x
                && action.area.y >= area.y
                && action.area.right() <= area.right()
                && action.area.bottom() <= area.bottom()
        }));
        let rule = actions
            .iter()
            .find(|action| action.id.starts_with("rule:0:"))
            .unwrap()
            .clone();
        assert!(rule.preferred);
        assert!(actions
            .iter()
            .any(|action| action.id == "control:acknowledge" && action.enabled));

        let acknowledge = actions
            .iter()
            .find(|action| action.id == "control:acknowledge")
            .unwrap();
        assert!(workspace.handle_mouse(click(acknowledge.area.x, acknowledge.area.y), area));
        assert!(matches!(
            workspace.rules[0].status,
            AlertStatus::Acknowledged { .. }
        ));
        assert!(workspace
            .actions(area)
            .iter()
            .any(|action| action.id == "control:acknowledge" && !action.enabled));

        assert!(workspace.activate_action("control:security"));
        assert_eq!(
            workspace.poll_intents(),
            vec![AppIntent::DispatchCommand {
                command: "SEC AAPL US".to_owned(),
                origin: ID,
            }]
        );

        workspace.rules[0].id = AlertRuleId::new("replacement:aapl");
        assert!(!workspace.activate_action(&rule.id));
        assert!(!workspace.activate_action("rule:999:missing"));
    }

    #[test]
    fn typed_view_round_trips_selected_rule_and_viewport_by_identity() {
        let mut source = AlertsWorkspace::new(stub_query(false));
        source.rules = (0..20).map(test_rule).collect();
        source.selected = 14;
        source.viewport_top.set(10);
        let state = source.capture_view();

        let mut restored = AlertsWorkspace::new(stub_query(false));
        restored.rules = (0..20).rev().map(test_rule).collect();
        let report = restored.restore_view(&state);

        assert_eq!(report.restored_fields, 2);
        assert_eq!(report.skipped_fields, 0);
        assert!(report.warnings.is_empty());
        assert_eq!(
            restored.selected_rule().unwrap().id.as_str(),
            "test:rule:014"
        );
        assert_eq!(
            restored.rules[restored.viewport_top.get()].id.as_str(),
            "test:rule:010"
        );
        assert_eq!(restored.capture_view(), state);
    }

    #[test]
    fn typed_view_resolves_pending_rule_identity_after_async_snapshot() {
        let state = WorkspaceViewState::new(ID.as_str())
            .with_field(
                "selected_rule_id",
                ViewValue::Text("test:rule:007".to_owned()),
            )
            .with_field("top_rule_id", ViewValue::Text("test:rule:004".to_owned()));
        let mut workspace = AlertsWorkspace::new(stub_query(false));
        workspace.rules.clear();

        let report = workspace.restore_view(&state);
        assert_eq!(report.restored_fields, 2);
        assert_eq!(workspace.capture_view(), state);

        workspace.apply_snapshot(AlertSnapshot::new(
            7,
            "2026-08-29T09:00:00Z",
            (0..12).map(test_rule).collect(),
            Vec::new(),
            "ASYNC TEST",
        ));

        assert_eq!(
            workspace.selected_rule().unwrap().id.as_str(),
            "test:rule:007"
        );
        assert_eq!(
            workspace.rules[workspace.viewport_top.get()].id.as_str(),
            "test:rule:004"
        );
        assert_eq!(workspace.capture_view(), state);
    }

    #[test]
    fn long_rule_tables_scroll_rendered_actions_with_selection() {
        let mut workspace = AlertsWorkspace::new(stub_query(false));
        workspace.rules = (0..24).map(test_rule).collect();
        workspace.selected = 23;
        let area = Rect::new(0, 0, 80, 24);
        let actions = workspace.actions(area);
        let top = workspace.viewport_top.get();

        assert!(top > 0);
        assert!(workspace.selected < top + workspace.viewport_rows.get());
        assert!(actions
            .iter()
            .any(|action| action.id == "rule:23:test:rule:023" && action.preferred));
        assert!(actions
            .iter()
            .filter(|action| action.id.starts_with("rule:"))
            .all(|action| action.area.bottom() <= alert_areas(area).rules.bottom()));
    }

    #[test]
    fn typed_view_degrades_missing_invalid_and_future_state_independently() {
        let state = WorkspaceViewState::new(ID.as_str())
            .with_field(
                "selected_rule_id",
                ViewValue::Text("missing:rule".to_owned()),
            )
            .with_field("top_rule_id", ViewValue::Text("bad\nrule".to_owned()))
            .with_field("future_field", ViewValue::Boolean(true))
            .with_child(WorkspaceViewState::new("future-alerts-child"));
        let mut workspace = AlertsWorkspace::new(stub_query(false));
        workspace.rules = (0..4).map(test_rule).collect();

        let report = workspace.restore_view(&state);

        assert_eq!(report.restored_fields, 0);
        assert_eq!(report.skipped_fields, 4);
        assert_eq!(report.warnings.len(), 4);
        assert_eq!(workspace.selected, 0);
        assert_eq!(workspace.viewport_top.get(), 0);
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
    fn a_trigger_without_other_changes_survives_restart() {
        let mut rule = test_rule(0);
        rule.debounce = DebouncePolicy::consecutive(2);
        let observation = |id| {
            AlertObservation::new(
                id,
                rule.instrument.canonical_id.as_str(),
                1_000_000.0,
                1.0,
                "2026-09-05T12:00:00Z",
            )
        };
        let first = observation("first");
        let second = observation("second");
        assert!(matches!(
            rule.evaluate(&first),
            AlertEvaluation::Pending { .. }
        ));
        let store = Arc::new(MemoryAlertState {
            state: Mutex::new(Some(AlertRulesState::new(1, vec![rule]).unwrap())),
        });
        let query = Arc::new(StubAlerts {
            snapshots: Mutex::new(VecDeque::from([AlertSnapshot::new(
                2,
                "now",
                vec![],
                vec![second.clone()],
                "TEST",
            )])),
        });
        let mut workspace = AlertsWorkspace::persistent(query, store.clone());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        while !matches!(workspace.rules[0].status, AlertStatus::Triggered { .. })
            && std::time::Instant::now() < deadline
        {
            workspace.poll_intents();
            std::thread::yield_now();
        }
        assert!(matches!(
            workspace.rules[0].status,
            AlertStatus::Triggered { .. }
        ));
        drop(workspace);
        let mut restored = AlertsWorkspace::persistent(stub_query(false), store);
        assert!(matches!(
            restored.rules[0].status,
            AlertStatus::Triggered { .. }
        ));
        assert_eq!(
            restored.rules[0].evaluate(&second),
            AlertEvaluation::Duplicate
        );
        assert_eq!(restored.state_revision, 2);
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

    #[test]
    fn scheduled_observations_confirm_rules_without_manual_refresh() {
        let mut rule = test_rule(0);
        rule.debounce = DebouncePolicy::consecutive(2);
        let snapshots = (1..=2)
            .map(|sequence| {
                AlertSnapshot::new(
                    sequence,
                    "now",
                    vec![rule.clone()],
                    vec![AlertObservation::new(
                        format!("tick-{sequence}"),
                        rule.instrument.canonical_id.as_str(),
                        110.0,
                        1.0,
                        "now",
                    )],
                    "TEST",
                )
            })
            .collect();
        let mut workspace = AlertsWorkspace::new(Arc::new(StubAlerts {
            snapshots: Mutex::new(snapshots),
        }))
        .with_refresh_interval(Duration::from_secs(5));
        settle(&mut workspace);
        assert!(matches!(
            workspace.rules[0].status,
            AlertStatus::Pending { .. }
        ));
        let due = workspace.next_refresh;
        workspace.poll_refresh_at(due - Duration::from_millis(1));
        assert_eq!(workspace.desired_generation, 1);
        workspace.poll_refresh_at(due);
        assert_eq!(workspace.desired_generation, 2);
        let deadline = Instant::now() + Duration::from_secs(1);
        while !matches!(workspace.rules[0].status, AlertStatus::Triggered { .. })
            && Instant::now() < deadline
        {
            workspace.poll_refresh_at(due);
            std::thread::yield_now();
        }
        assert!(matches!(
            workspace.rules[0].status,
            AlertStatus::Triggered { .. }
        ));
        assert_eq!(workspace.desired_generation, 2);
    }
}
