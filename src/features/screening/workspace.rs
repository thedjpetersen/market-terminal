use std::{
    cell::Cell as StateCell,
    sync::{
        mpsc::{sync_channel, Receiver, SyncSender, TrySendError},
        Arc,
    },
    thread::JoinHandle,
};

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
        contains, is_primary_click, scroll_key, table_row_at,
        theme::{AMBER, BG, CYAN, GREEN, INK, MUTED, RED, YELLOW},
    },
};

use super::{
    builtin_screen_definitions, evaluate_screen, Comparison, ScreenCatalogState, ScreenClause,
    ScreenDefinition, ScreenDimension, ScreenEvaluation, ScreenExpression, ScreenField,
    ScreenSortDirection, ScreenStateError, ScreenStateStore, ScreeningUniverseQuery,
    UniverseHistoryManifest, UniverseHistoryStore, ID, MAX_SAVED_SCREENS,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunSource {
    Live,
    Replay(u64),
}

struct RunRequest {
    generation: u64,
    definition: ScreenDefinition,
    source: RunSource,
}

struct RunResult {
    generation: u64,
    result: Result<ScreenEvaluation, String>,
    history: Option<UniverseHistoryManifest>,
    history_warning: Option<String>,
    source: RunSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScreenControl {
    Previous,
    Next,
    Security,
    Chart,
    Spreadsheet,
    Monitor,
    History,
    Live,
    Refresh,
}

impl ScreenControl {
    const fn id(self) -> &'static str {
        match self {
            Self::Previous => "control:previous",
            Self::Next => "control:next",
            Self::Security => "control:security",
            Self::Chart => "control:chart",
            Self::Spreadsheet => "control:spreadsheet",
            Self::Monitor => "control:monitor",
            Self::History => "control:history",
            Self::Live => "control:live",
            Self::Refresh => "control:refresh",
        }
    }

    fn parse(id: &str) -> Option<Self> {
        match id {
            "control:previous" => Some(Self::Previous),
            "control:next" => Some(Self::Next),
            "control:security" => Some(Self::Security),
            "control:chart" => Some(Self::Chart),
            "control:spreadsheet" => Some(Self::Spreadsheet),
            "control:monitor" => Some(Self::Monitor),
            "control:history" => Some(Self::History),
            "control:live" => Some(Self::Live),
            "control:refresh" => Some(Self::Refresh),
            _ => None,
        }
    }
}

pub struct ScreeningWorkspace {
    run_sender: SyncSender<RunRequest>,
    run_receiver: Receiver<RunResult>,
    pending_run: Option<RunRequest>,
    desired_generation: u64,
    applied_generation: u64,
    definitions: Vec<ScreenDefinition>,
    custom_revision: u64,
    history: UniverseHistoryManifest,
    active_screen_id: String,
    replay_version: Option<u64>,
    evaluation: Option<ScreenEvaluation>,
    selected: usize,
    viewport_top: StateCell<usize>,
    viewport_rows: StateCell<usize>,
    pending_selected_id: Option<String>,
    pending_top_id: Option<String>,
    status: String,
    pending_intents: Vec<AppIntent>,
    persist_sender: Option<SyncSender<ScreenCatalogState>>,
    persist_receiver: Option<Receiver<Result<u64, ScreenStateError>>>,
    pending_persist: Option<ScreenCatalogState>,
    persist_worker: Option<JoinHandle<()>>,
}

impl ScreeningWorkspace {
    pub fn new(query: Arc<dyn ScreeningUniverseQuery>) -> Self {
        Self::configured(query, None, None)
    }

    pub fn persistent(
        query: Arc<dyn ScreeningUniverseQuery>,
        store: Arc<dyn ScreenStateStore>,
    ) -> Self {
        Self::configured(query, Some(store), None)
    }

    pub fn persistent_with_history(
        query: Arc<dyn ScreeningUniverseQuery>,
        store: Arc<dyn ScreenStateStore>,
        history_store: Arc<dyn UniverseHistoryStore>,
    ) -> Self {
        Self::configured(query, Some(store), Some(history_store))
    }

    fn configured(
        query: Arc<dyn ScreeningUniverseQuery>,
        store: Option<Arc<dyn ScreenStateStore>>,
        history_store: Option<Arc<dyn UniverseHistoryStore>>,
    ) -> Self {
        let (run_sender, worker_receiver) = sync_channel::<RunRequest>(1);
        let (worker_sender, run_receiver) = sync_channel::<RunResult>(1);
        let worker_history = history_store.clone();
        std::thread::Builder::new()
            .name("screening-runner".to_owned())
            .spawn(move || {
                while let Ok(request) = worker_receiver.recv() {
                    let mut history = None;
                    let mut history_warning = None;
                    let snapshot = match request.source {
                        RunSource::Live => query
                            .load_universe(&request.definition.universe_id)
                            .map_err(|error| error.to_string())
                            .inspect(|snapshot| {
                                if let Some(store) = &worker_history {
                                    match store.record_snapshot(snapshot) {
                                        Ok(manifest) => history = Some(manifest),
                                        Err(error) => history_warning = Some(error.to_string()),
                                    }
                                }
                            }),
                        RunSource::Replay(version) => worker_history
                            .as_ref()
                            .ok_or_else(|| "universe history is not configured".to_owned())
                            .and_then(|store| {
                                store
                                    .load_snapshot(version)
                                    .map_err(|error| error.to_string())
                            }),
                    };
                    let result = snapshot.and_then(|snapshot| {
                        evaluate_screen(&request.definition, snapshot)
                            .map_err(|error| error.to_string())
                    });
                    if worker_sender
                        .send(RunResult {
                            generation: request.generation,
                            result,
                            history,
                            history_warning,
                            source: request.source,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .expect("screening worker should start");

        let mut definitions = builtin_screen_definitions();
        let mut custom_revision = 0;
        let mut status = "READY".to_owned();
        if let Some(store) = &store {
            match store.load_screens() {
                Ok(Some(state)) => {
                    custom_revision = state.revision;
                    definitions.extend(state.screens);
                    status = format!("RESTORED SAVED SCREENS · REV {custom_revision}");
                }
                Ok(None) => {}
                Err(error) => status = format!("SAVED SCREEN RESTORE DEGRADED · {error}"),
            }
        }
        let history = history_store
            .as_ref()
            .map_or_else(UniverseHistoryManifest::empty, |store| {
                match store.load_history() {
                    Ok(history) => history,
                    Err(error) => {
                        status = format!("UNIVERSE HISTORY RESTORE DEGRADED · {error}");
                        UniverseHistoryManifest::empty()
                    }
                }
            });

        let (persist_sender, persist_receiver, persist_worker) = if let Some(store) = store {
            let (sender, receiver) = sync_channel::<ScreenCatalogState>(1);
            let (result_sender, result_receiver) = sync_channel(1);
            let worker = std::thread::Builder::new()
                .name("screening-persistence".to_owned())
                .spawn(move || {
                    while let Ok(state) = receiver.recv() {
                        let revision = state.revision;
                        let result = store.save_screens(&state).map(|()| revision);
                        if result_sender.send(result).is_err() {
                            break;
                        }
                    }
                })
                .expect("screen persistence worker should start");
            (Some(sender), Some(result_receiver), Some(worker))
        } else {
            (None, None, None)
        };

        let active_screen_id = definitions
            .first()
            .expect("built-in screens are non-empty")
            .id
            .clone();
        let mut workspace = Self {
            run_sender,
            run_receiver,
            pending_run: None,
            desired_generation: 0,
            applied_generation: 0,
            definitions,
            custom_revision,
            history,
            active_screen_id,
            replay_version: None,
            evaluation: None,
            selected: 0,
            viewport_top: StateCell::new(0),
            viewport_rows: StateCell::new(0),
            pending_selected_id: None,
            pending_top_id: None,
            status,
            pending_intents: Vec::new(),
            persist_sender,
            persist_receiver,
            pending_persist: None,
            persist_worker,
        };
        workspace.refresh();
        workspace
    }

    fn active_definition(&self) -> &ScreenDefinition {
        self.definitions
            .iter()
            .find(|definition| definition.id == self.active_screen_id)
            .unwrap_or(&self.definitions[0])
    }

    fn refresh(&mut self) {
        self.desired_generation = self.desired_generation.saturating_add(1);
        let source = self
            .replay_version
            .map_or(RunSource::Live, RunSource::Replay);
        self.pending_run = Some(RunRequest {
            generation: self.desired_generation,
            definition: self.active_definition().clone(),
            source,
        });
        self.status = match source {
            RunSource::Live => format!("RUNNING {}…", self.active_definition().label),
            RunSource::Replay(version) => format!(
                "REPLAYING {} · V{version:016X}…",
                self.active_definition().label
            ),
        };
        self.dispatch_pending_run();
    }

    fn refresh_live(&mut self) {
        self.replay_version = None;
        self.refresh();
    }

    fn dispatch_pending_run(&mut self) {
        let Some(request) = self.pending_run.take() else {
            return;
        };
        match self.run_sender.try_send(request) {
            Ok(()) => {}
            Err(TrySendError::Full(request)) => self.pending_run = Some(request),
            Err(TrySendError::Disconnected(_)) => {
                self.status = "SCREENING WORKER STOPPED".to_owned()
            }
        }
    }

    fn poll_run(&mut self) {
        while let Ok(result) = self.run_receiver.try_recv() {
            if let Some(history) = result.history {
                self.history = history;
            }
            if result.generation != self.desired_generation {
                continue;
            }
            self.applied_generation = result.generation;
            match result.result {
                Ok(evaluation) => {
                    let selected = self
                        .pending_selected_id
                        .clone()
                        .or_else(|| self.selected_identity());
                    let top = self.pending_top_id.clone().or_else(|| self.top_identity());
                    self.evaluation = Some(evaluation);
                    self.restore_row_anchors(selected.as_deref(), top.as_deref());
                    let evaluation = self.evaluation.as_ref().expect("installed evaluation");
                    let mode = match result.source {
                        RunSource::Live => "LIVE COMPLETE".to_owned(),
                        RunSource::Replay(version) => format!("REPLAY V{version:016X}"),
                    };
                    self.status = format!(
                        "{mode} · {} MATCHES · {} EXCLUDED · {:.0}% COVERAGE",
                        evaluation.rows.len(),
                        evaluation.exclusions.len(),
                        evaluation.coverage_percent(),
                    );
                    if let Some(warning) = result.history_warning {
                        self.status.push_str(" · HISTORY DEGRADED · ");
                        self.status.push_str(&warning);
                    }
                }
                Err(error) => {
                    self.status = if self.evaluation.is_some() {
                        format!("REFRESH FAILED · SHOWING LAST RUN · {error}")
                    } else {
                        format!("SCREEN FAILED · {error}")
                    };
                }
            }
        }
        self.dispatch_pending_run();
    }

    fn selected_identity(&self) -> Option<String> {
        self.evaluation
            .as_ref()?
            .rows
            .get(self.selected)
            .map(|row| row.member.instrument_id.as_str().to_owned())
    }

    fn top_identity(&self) -> Option<String> {
        self.evaluation
            .as_ref()?
            .rows
            .get(self.viewport_top.get())
            .map(|row| row.member.instrument_id.as_str().to_owned())
    }

    fn restore_row_anchors(&mut self, selected: Option<&str>, top: Option<&str>) {
        let rows = self
            .evaluation
            .as_ref()
            .map_or(&[][..], |evaluation| evaluation.rows.as_slice());
        self.selected = selected
            .and_then(|id| {
                rows.iter()
                    .position(|row| row.member.instrument_id.as_str() == id)
            })
            .unwrap_or_default();
        self.viewport_top.set(
            top.and_then(|id| {
                rows.iter()
                    .position(|row| row.member.instrument_id.as_str() == id)
            })
            .unwrap_or_default(),
        );
        self.pending_selected_id = selected
            .filter(|id| {
                rows.iter()
                    .all(|row| row.member.instrument_id.as_str() != *id)
                    && rows.is_empty()
            })
            .map(str::to_owned);
        self.pending_top_id = top
            .filter(|id| {
                rows.iter()
                    .all(|row| row.member.instrument_id.as_str() != *id)
                    && rows.is_empty()
            })
            .map(str::to_owned);
        self.reveal_selection();
    }

    fn move_selection(&mut self, delta: isize) {
        self.pending_selected_id = None;
        let count = self
            .evaluation
            .as_ref()
            .map_or(0, |evaluation| evaluation.rows.len());
        if count == 0 {
            self.selected = 0;
            return;
        }
        self.selected = self.selected.saturating_add_signed(delta).min(count - 1);
        self.reveal_selection();
    }

    fn reveal_selection(&self) {
        let count = self
            .evaluation
            .as_ref()
            .map_or(0, |evaluation| evaluation.rows.len());
        let capacity = self.viewport_rows.get();
        if count == 0 {
            self.viewport_top.set(0);
            return;
        }
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
            .set(top.min(count.saturating_sub(capacity.max(1))));
    }

    fn update_viewport(&self, area: Rect) -> (usize, usize) {
        let capacity = usize::from(area.height.saturating_sub(4));
        self.viewport_rows.set(capacity);
        self.reveal_selection();
        (self.viewport_top.get(), capacity)
    }

    fn activate_screen(&mut self, id: &str) -> bool {
        let Some(definition) = self
            .definitions
            .iter()
            .find(|definition| definition.id.eq_ignore_ascii_case(id))
        else {
            self.status = format!("SCREEN NOT FOUND · {id}");
            return false;
        };
        self.active_screen_id.clone_from(&definition.id);
        self.replay_version = None;
        self.selected = 0;
        self.viewport_top.set(0);
        self.pending_selected_id = None;
        self.pending_top_id = None;
        self.refresh();
        true
    }

    fn history_summary(&mut self, universe_id: Option<&str>) {
        let universe_id = universe_id
            .unwrap_or(&self.active_definition().universe_id)
            .to_ascii_lowercase();
        let entries = self.history.entries_for(&universe_id);
        if entries.is_empty() {
            self.status = format!("NO RECORDED HISTORY · {universe_id}");
            return;
        }
        let summary = entries
            .iter()
            .take(4)
            .map(|entry| {
                format!(
                    "V{:016X} {} {} MEMBERS",
                    entry.version, entry.as_of, entry.member_count
                )
            })
            .collect::<Vec<_>>()
            .join("  |  ");
        self.status = format!(
            "HISTORY {universe_id} · {} FRAME(S) · {summary}",
            entries.len()
        );
    }

    fn replay(&mut self, version: u64, screen_id: Option<&str>) -> bool {
        let Some(entry) = self
            .history
            .entries
            .iter()
            .find(|entry| entry.version == version)
            .cloned()
        else {
            self.status = format!("HISTORY VERSION NOT FOUND · V{version:016X}");
            return false;
        };
        if let Some(screen_id) = screen_id {
            let Some(definition) = self
                .definitions
                .iter()
                .find(|definition| definition.id.eq_ignore_ascii_case(screen_id))
            else {
                self.status = format!("SCREEN NOT FOUND · {screen_id}");
                return false;
            };
            self.active_screen_id.clone_from(&definition.id);
        }
        if self.active_definition().universe_id != entry.universe_id {
            self.status = format!(
                "REPLAY UNIVERSE MISMATCH · SCREEN {} EXPECTS {} · VERSION IS {}",
                self.active_definition().id,
                self.active_definition().universe_id,
                entry.universe_id
            );
            return false;
        }
        self.replay_version = Some(version);
        self.selected = 0;
        self.viewport_top.set(0);
        self.pending_selected_id = None;
        self.pending_top_id = None;
        self.refresh();
        true
    }

    fn cycle_screen(&mut self, delta: isize) {
        let count = self.definitions.len();
        if count == 0 {
            return;
        }
        let current = self
            .definitions
            .iter()
            .position(|definition| definition.id == self.active_screen_id)
            .unwrap_or_default();
        let next = (current as isize + delta).rem_euclid(count as isize) as usize;
        let id = self.definitions[next].id.clone();
        self.activate_screen(&id);
    }

    fn save_definition(&mut self, args: &[String]) {
        match parse_saved_definition(args) {
            Ok(definition) => {
                if self
                    .definitions
                    .iter()
                    .any(|existing| existing.built_in && existing.id == definition.id)
                {
                    self.status = format!("BUILT-IN SCREEN IS PROTECTED · {}", definition.id);
                    return;
                }
                if self
                    .definitions
                    .iter()
                    .filter(|existing| !existing.built_in)
                    .all(|existing| existing.id != definition.id)
                    && self
                        .definitions
                        .iter()
                        .filter(|existing| !existing.built_in)
                        .count()
                        >= MAX_SAVED_SCREENS
                {
                    self.status = format!("SAVED SCREEN LIMIT REACHED · {MAX_SAVED_SCREENS}");
                    return;
                }
                if let Some(existing) = self
                    .definitions
                    .iter_mut()
                    .find(|existing| !existing.built_in && existing.id == definition.id)
                {
                    *existing = definition.clone();
                } else {
                    self.definitions.push(definition.clone());
                }
                self.active_screen_id.clone_from(&definition.id);
                self.custom_revision = self.custom_revision.saturating_add(1);
                self.queue_persist();
                self.refresh();
            }
            Err(error) => self.status = format!("SCREEN SAVE ERROR · {error}"),
        }
    }

    fn delete_definition(&mut self, id: &str) {
        if self
            .definitions
            .iter()
            .any(|definition| definition.id == id && definition.built_in)
        {
            self.status = format!("BUILT-IN SCREEN IS PROTECTED · {id}");
            return;
        }
        let before = self.definitions.len();
        self.definitions
            .retain(|definition| definition.built_in || definition.id != id);
        if self.definitions.len() == before {
            self.status = format!("SAVED SCREEN NOT FOUND · {id}");
            return;
        }
        if self.active_screen_id == id {
            self.active_screen_id = self.definitions[0].id.clone();
        }
        self.custom_revision = self.custom_revision.saturating_add(1);
        self.queue_persist();
        self.refresh();
    }

    fn custom_state(&self) -> ScreenCatalogState {
        ScreenCatalogState::new(
            self.custom_revision,
            self.definitions
                .iter()
                .filter(|definition| !definition.built_in)
                .cloned()
                .collect(),
        )
        .expect("workspace maintains valid screen definitions")
    }

    fn queue_persist(&mut self) {
        if self.persist_sender.is_none() {
            return;
        }
        self.pending_persist = Some(self.custom_state());
        self.dispatch_pending_persist();
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
                self.status = "SCREEN PERSISTENCE WORKER STOPPED".to_owned()
            }
        }
    }

    fn poll_persist(&mut self) {
        if let Some(receiver) = &self.persist_receiver {
            while let Ok(result) = receiver.try_recv() {
                if let Err(error) = result {
                    self.status = format!("SCREEN PERSISTENCE DEGRADED · {error}");
                }
            }
        }
        self.dispatch_pending_persist();
    }

    fn open_selected(&mut self) -> bool {
        let Some(symbol) = self
            .evaluation
            .as_ref()
            .and_then(|evaluation| evaluation.rows.get(self.selected))
            .map(|row| row.member.symbol.clone())
        else {
            return false;
        };
        self.pending_intents.push(AppIntent::DispatchCommand {
            command: format!("SEC {symbol}"),
            origin: ID,
        });
        true
    }

    fn send_selected_to_sheet(&mut self) -> bool {
        let Some(symbol) = self
            .evaluation
            .as_ref()
            .and_then(|evaluation| evaluation.rows.get(self.selected))
            .map(|row| row.member.symbol.clone())
        else {
            return false;
        };
        self.pending_intents.push(AppIntent::DispatchCommand {
            command: format!("SHEET INSERT {symbol}"),
            origin: ID,
        });
        true
    }

    fn open_selected_chart(&mut self) -> bool {
        let Some(symbol) = self
            .evaluation
            .as_ref()
            .and_then(|evaluation| evaluation.rows.get(self.selected))
            .map(|row| row.member.symbol.clone())
        else {
            return false;
        };
        self.pending_intents.push(AppIntent::DispatchCommand {
            command: format!("CHART {symbol}"),
            origin: ID,
        });
        true
    }

    fn open_universe_monitor(&mut self) -> bool {
        let universe = self.active_definition().universe_id.clone();
        self.pending_intents.push(AppIntent::DispatchCommand {
            command: format!("MON {universe}"),
            origin: ID,
        });
        true
    }

    fn activate_control(&mut self, control: ScreenControl) -> bool {
        match control {
            ScreenControl::Previous => self.cycle_screen(-1),
            ScreenControl::Next => self.cycle_screen(1),
            ScreenControl::Security => return self.open_selected(),
            ScreenControl::Chart => return self.open_selected_chart(),
            ScreenControl::Spreadsheet => return self.send_selected_to_sheet(),
            ScreenControl::Monitor => return self.open_universe_monitor(),
            ScreenControl::History => self.history_summary(None),
            ScreenControl::Live => self.refresh_live(),
            ScreenControl::Refresh => self.refresh(),
        }
        true
    }

    fn controls(&self, area: Rect) -> Vec<(ScreenControl, Rect)> {
        let parts = Layout::horizontal([
            Constraint::Length(9),
            Constraint::Length(9),
            Constraint::Length(12),
            Constraint::Length(10),
            Constraint::Length(12),
            Constraint::Length(11),
            Constraint::Length(11),
            Constraint::Length(8),
            Constraint::Min(10),
        ])
        .split(area);
        [
            ScreenControl::Previous,
            ScreenControl::Next,
            ScreenControl::Security,
            ScreenControl::Chart,
            ScreenControl::Spreadsheet,
            ScreenControl::Monitor,
            ScreenControl::History,
            ScreenControl::Live,
            ScreenControl::Refresh,
        ]
        .into_iter()
        .zip(parts.iter().copied())
        .filter(|(_, area)| area.width > 2)
        .collect()
    }

    fn control_enabled(&self, control: ScreenControl) -> bool {
        match control {
            ScreenControl::Security | ScreenControl::Chart | ScreenControl::Spreadsheet => self
                .evaluation
                .as_ref()
                .is_some_and(|evaluation| !evaluation.rows.is_empty()),
            ScreenControl::Previous | ScreenControl::Next => self.definitions.len() > 1,
            ScreenControl::History => !self.history.entries.is_empty(),
            ScreenControl::Live => self.replay_version.is_some(),
            ScreenControl::Monitor | ScreenControl::Refresh => true,
        }
    }
}

impl Workspace for ScreeningWorkspace {
    fn descriptor(&self) -> WorkspaceDescriptor {
        WorkspaceDescriptor {
            id: ID,
            label: "SCREEN",
            hotkey: '\0',
            commands: &["SCREEN", "SCREENER"],
        }
    }

    fn hotkey(&self) -> Option<char> {
        None
    }

    fn is_favorite(&self) -> bool {
        false
    }

    fn handle_command(&mut self, invocation: &CommandInvocation) -> bool {
        let Some(operation) = invocation.args.first() else {
            self.refresh();
            return true;
        };
        match operation.to_ascii_uppercase().as_str() {
            "RUN" => {
                if let Some(id) = invocation.args.get(1) {
                    self.activate_screen(id);
                } else {
                    self.status = "SCREEN RUN REQUIRES AN ID".to_owned();
                }
            }
            "SAVE" => self.save_definition(invocation.args.get(1..).unwrap_or_default()),
            "DELETE" | "DROP" => {
                if let Some(id) = invocation.args.get(1) {
                    self.delete_definition(id);
                } else {
                    self.status = "SCREEN DELETE REQUIRES AN ID".to_owned();
                }
            }
            "LIST" => {
                self.status = format!(
                    "SCREENS · {}",
                    self.definitions
                        .iter()
                        .map(|definition| definition.id.as_str())
                        .collect::<Vec<_>>()
                        .join(" · ")
                );
            }
            "HISTORY" => self.history_summary(invocation.args.get(1).map(String::as_str)),
            "REPLAY" => {
                let Some(version) = invocation
                    .args
                    .get(1)
                    .and_then(|value| parse_version(value))
                else {
                    self.status = "SCREEN REPLAY REQUIRES A DECIMAL OR HEX VERSION".to_owned();
                    return true;
                };
                self.replay(version, invocation.args.get(2).map(String::as_str));
            }
            "LIVE" => self.refresh_live(),
            "NEXT" => self.cycle_screen(1),
            "PREV" | "PREVIOUS" => self.cycle_screen(-1),
            _ => {
                self.activate_screen(operation);
            }
        }
        true
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::Enter | KeyCode::Char('s') => {
                return self.open_selected();
            }
            KeyCode::Char('c') => return self.open_selected_chart(),
            KeyCode::Char('a') => return self.send_selected_to_sheet(),
            KeyCode::Char('m') => return self.open_universe_monitor(),
            KeyCode::Char('h') => self.history_summary(None),
            KeyCode::Char('l') => self.refresh_live(),
            KeyCode::Char('r') => self.refresh(),
            KeyCode::Char('[') => self.cycle_screen(-1),
            KeyCode::Char(']') => self.cycle_screen(1),
            _ => return false,
        }
        true
    }

    fn handle_mouse(&mut self, event: MouseEvent, area: Rect) -> bool {
        let areas = screen_areas(area);
        let (top, rows) = self.update_viewport(areas.table);
        if let Some(visible) = table_row_at(
            event,
            areas.table,
            rows.min(
                self.evaluation
                    .as_ref()
                    .map_or(0, |evaluation| evaluation.rows.len().saturating_sub(top)),
            ),
        ) {
            self.selected = top.saturating_add(visible);
            self.reveal_selection();
            return true;
        }
        if is_primary_click(event, areas.footer) {
            for (control, control_area) in self.controls(areas.footer) {
                if contains(control_area, event.column, event.row) && self.control_enabled(control)
                {
                    return self.activate_control(control);
                }
            }
            return true;
        }
        if let Some(key) = scroll_key(event, areas.table) {
            return self.handle_key(key);
        }
        false
    }

    fn actions(&self, area: Rect) -> Vec<WorkspaceAction> {
        let areas = screen_areas(area);
        let (top, capacity) = self.update_viewport(areas.table);
        let mut actions = self
            .evaluation
            .as_ref()
            .into_iter()
            .flat_map(|evaluation| evaluation.rows.iter().skip(top).take(capacity).enumerate())
            .map(|(ordinal, row)| {
                let index = top.saturating_add(ordinal);
                let mut action = WorkspaceAction::new(
                    format!("row:{index}:{}", row.member.instrument_id),
                    format!("Open rank {} {} in Security", row.rank, row.member.symbol),
                    Rect::new(
                        areas.table.x.saturating_add(1),
                        areas
                            .table
                            .y
                            .saturating_add(3 + u16::try_from(ordinal).unwrap_or(u16::MAX)),
                        areas.table.width.saturating_sub(2),
                        1,
                    ),
                );
                if index == self.selected {
                    action = action.preferred();
                }
                action
            })
            .collect::<Vec<_>>();
        actions.extend(
            self.controls(areas.footer)
                .into_iter()
                .map(|(control, area)| {
                    let label = match control {
                        ScreenControl::Previous => "Run previous saved screen",
                        ScreenControl::Next => "Run next saved screen",
                        ScreenControl::Security => "Open selected result in Security",
                        ScreenControl::Chart => "Open selected result in Chart",
                        ScreenControl::Spreadsheet => "Insert selected symbol into Spreadsheet",
                        ScreenControl::Monitor => "Open source universe in Monitor",
                        ScreenControl::History => "List retained point-in-time universe versions",
                        ScreenControl::Live => "Leave replay and evaluate a new live snapshot",
                        ScreenControl::Refresh => "Re-run screen against a new versioned snapshot",
                    };
                    let action = WorkspaceAction::new(control.id(), label, area);
                    if self.control_enabled(control) {
                        action
                    } else {
                        action.disabled()
                    }
                }),
        );
        actions
    }

    fn activate_action(&mut self, id: &str) -> bool {
        if let Some(control) = ScreenControl::parse(id) {
            return self.control_enabled(control) && self.activate_control(control);
        }
        let Some(row) = id.strip_prefix("row:") else {
            return false;
        };
        let Some((index, expected_id)) = row.split_once(':') else {
            return false;
        };
        let Ok(index) = index.parse::<usize>() else {
            return false;
        };
        let Some(actual) = self
            .evaluation
            .as_ref()
            .and_then(|evaluation| evaluation.rows.get(index))
        else {
            return false;
        };
        if actual.member.instrument_id.as_str() != expected_id {
            return false;
        }
        self.selected = index;
        self.pending_selected_id = None;
        self.reveal_selection();
        self.open_selected()
    }

    fn poll_intents(&mut self) -> Vec<AppIntent> {
        self.poll_run();
        self.poll_persist();
        std::mem::take(&mut self.pending_intents)
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let areas = screen_areas(area);
        let (top, capacity) = self.update_viewport(areas.table);
        let definition = self.active_definition();
        let mode = self.replay_version.map_or_else(
            || "LIVE".to_owned(),
            |version| format!("REPLAY V{version:016X}"),
        );
        let (universe_label, version, as_of, source, matches, exclusions, coverage) = self
            .evaluation
            .as_ref()
            .map_or(("PENDING", 0, "--", "--", 0, 0, 0.0), |evaluation| {
                (
                    evaluation.universe.label.as_str(),
                    evaluation.universe.version,
                    evaluation.universe.as_of.as_str(),
                    evaluation.universe.source.as_str(),
                    evaluation.rows.len(),
                    evaluation.exclusions.len(),
                    evaluation.coverage_percent(),
                )
            });
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    format!(" {} ", definition.label),
                    Style::new().bg(AMBER.into()).fg(BG.into()).bold(),
                ),
                Span::styled(format!(" {universe_label}  "), INK),
                Span::styled(format!("V{version:016X}  "), MUTED),
                Span::styled(format!("{mode}  "), AMBER),
                Span::styled(
                    format!("{matches} MATCH  {exclusions} OUT  {coverage:.0}% COVERAGE  "),
                    CYAN,
                ),
                Span::styled(&self.status, YELLOW),
            ]))
            .block(terminal_block("SCREEN", "POINT-IN-TIME RANKING")),
            areas.header,
        );
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(vec![
                    Span::styled("EXPRESSION  ", AMBER),
                    Span::styled(definition.expression(), INK),
                    Span::styled(
                        format!(
                            "  SORT {} {}  LIMIT {}",
                            definition.sort_field.label(),
                            definition.sort_direction.label(),
                            definition.limit
                        ),
                        MUTED,
                    ),
                ]),
                Line::from(vec![
                    Span::styled("INPUT  ", AMBER),
                    Span::styled(
                        format!(
                            "{source} · AS OF {as_of} · {} RETAINED FRAME(S)",
                            self.history.entries_for(&definition.universe_id).len()
                        ),
                        MUTED,
                    ),
                ]),
            ]),
            areas.expression,
        );

        let header = Row::new([
            "#",
            "SYMBOL",
            "LAST",
            "% CHG",
            "VOLUME",
            "SPREAD",
            "QUALITY",
            "WHY RANKED",
        ])
        .style(Style::new().fg(AMBER.into()).bold())
        .bottom_margin(1);
        let rows = self
            .evaluation
            .as_ref()
            .into_iter()
            .flat_map(|evaluation| evaluation.rows.iter().enumerate().skip(top).take(capacity))
            .map(|(index, row)| {
                let style = if index == self.selected {
                    Style::new().bg(CYAN.into()).fg(BG.into()).bold()
                } else {
                    Style::new()
                };
                Row::new(vec![
                    Cell::from(row.rank.to_string()),
                    Cell::from(row.member.symbol.clone()),
                    Cell::from(format_optional(row.member.last, 2)),
                    Cell::from(format_signed_percent(row.member.change_percent)),
                    Cell::from(format_volume(row.member.volume)),
                    Cell::from(
                        row.member
                            .spread_bps
                            .map_or_else(|| "--".to_owned(), |value| format!("{value:.2}BP")),
                    ),
                    Cell::from(row.member.quality.clone()),
                    Cell::from(format!(
                        "{} {}",
                        definition.sort_field.label(),
                        super::domain::format_value(definition.sort_field, row.sort_value)
                    )),
                ])
                .style(style)
            });
        frame.render_widget(
            Table::new(
                rows,
                [
                    Constraint::Length(4),
                    Constraint::Length(10),
                    Constraint::Length(12),
                    Constraint::Length(10),
                    Constraint::Length(13),
                    Constraint::Length(11),
                    Constraint::Length(16),
                    Constraint::Min(20),
                ],
            )
            .header(header)
            .column_spacing(1)
            .block(terminal_block(
                "RESULT",
                "STABLE RANK · SELECT FOR EVIDENCE",
            )),
            areas.table,
        );

        let detail = self
            .evaluation
            .as_ref()
            .and_then(|evaluation| evaluation.rows.get(self.selected))
            .map_or_else(
                || vec![Line::styled("NO MATCHED ROW SELECTED", MUTED)],
                |row| {
                    let mut lines = vec![Line::from(vec![
                        Span::styled(
                            format!("{} · {}  ", row.member.symbol, row.member.description),
                            AMBER,
                        ),
                        Span::styled(
                            format!("{} · {}", row.member.provider, row.member.currency),
                            MUTED,
                        ),
                    ])];
                    lines.extend(row.evidence.iter().take(3).map(|evidence| {
                        Line::styled(evidence.label(), if evidence.passed { GREEN } else { RED })
                    }));
                    lines
                },
            );
        frame.render_widget(
            Paragraph::new(detail)
                .wrap(Wrap { trim: true })
                .block(terminal_block("WHY", "PREDICATE EVIDENCE")),
            areas.detail,
        );

        for (control, control_area) in self.controls(areas.footer) {
            let text = match control {
                ScreenControl::Previous => " [ PREV ",
                ScreenControl::Next => " ] NEXT ",
                ScreenControl::Security => " ENTER SEC ",
                ScreenControl::Chart => " C CHART ",
                ScreenControl::Spreadsheet => " A SHEET ",
                ScreenControl::Monitor => " M MONITOR ",
                ScreenControl::History => " H HISTORY ",
                ScreenControl::Live => " L LIVE ",
                ScreenControl::Refresh => " R REFRESH ",
            };
            frame.render_widget(
                Paragraph::new(text).style(if self.control_enabled(control) {
                    AMBER
                } else {
                    MUTED
                }),
                control_area,
            );
        }
    }

    fn capture_view(&self) -> WorkspaceViewState {
        let mut state = WorkspaceViewState::new(ID.as_str())
            .with_field("screen_id", ViewValue::Text(self.active_screen_id.clone()));
        if let Some(id) = self
            .selected_identity()
            .or_else(|| self.pending_selected_id.clone())
        {
            state = state.with_field("selected_instrument_id", ViewValue::Text(id));
        }
        if let Some(id) = self.top_identity().or_else(|| self.pending_top_id.clone()) {
            state = state.with_field("top_instrument_id", ViewValue::Text(id));
        }
        state
    }

    fn restore_view(&mut self, state: &WorkspaceViewState) -> ViewRestoreReport {
        if !state.workspace.eq_ignore_ascii_case(ID.as_str()) {
            return ViewRestoreReport::warning(format!(
                "saved state belongs to {}, not screening",
                state.workspace
            ));
        }
        let mut report = ViewRestoreReport::default();
        if let Some(value) = state.fields.get("screen_id") {
            if let Some(id) = value.as_text().filter(|id| {
                self.definitions
                    .iter()
                    .any(|definition| definition.id == *id)
            }) {
                self.active_screen_id = id.to_owned();
                report.restored_fields += 1;
            } else {
                report.skipped_fields += 1;
                report
                    .warnings
                    .push("saved screen is no longer available".to_owned());
            }
        }
        let selected = restore_identity_field(
            state,
            "selected_instrument_id",
            "selected result",
            &mut report,
        );
        let top =
            restore_identity_field(state, "top_instrument_id", "viewport anchor", &mut report);
        self.pending_selected_id = selected.clone();
        self.pending_top_id = top.clone();
        self.restore_row_anchors(selected.as_deref(), top.as_deref());
        // The installed evaluation can still belong to the previously active
        // screen. Keep saved identities authoritative until the new run lands.
        self.pending_selected_id = selected;
        self.pending_top_id = top;
        let known = ["screen_id", "selected_instrument_id", "top_instrument_id"];
        let unknown = state
            .fields
            .keys()
            .filter(|field| !known.contains(&field.as_str()))
            .count();
        if unknown > 0 {
            report.skipped_fields += unknown;
            report
                .warnings
                .push(format!("ignored {unknown} future Screening field(s)"));
        }
        if !state.children.is_empty() {
            report.skipped_fields += state.children.len();
            report.warnings.push(format!(
                "ignored {} future Screening child state(s)",
                state.children.len()
            ));
        }
        self.refresh();
        report
    }
}

impl Drop for ScreeningWorkspace {
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

fn parse_saved_definition(args: &[String]) -> Result<ScreenDefinition, String> {
    if args.len() < 5 {
        return Err(
            "usage: SCREEN SAVE <id> <universe> <expression> [SORT <field> <ASC|DESC>] [LIMIT <n>]"
                .to_owned(),
        );
    }
    let id = args[0].to_ascii_lowercase();
    let universe = args[1].to_ascii_lowercase();
    let tokens = expand_expression_tokens(&args[2..]);
    let mut parser = ScreenExpressionParser::new(&tokens);
    let expression = parser.parse()?;
    let mut index = parser.index;
    let mut clauses = Vec::new();
    collect_expression_clauses(&expression, &mut clauses);
    let mut sort_field = clauses
        .first()
        .expect("validated expressions have at least one predicate")
        .field;
    let mut sort_direction = ScreenSortDirection::Descending;
    let mut limit = 50;
    let mut saw_sort = false;
    let mut saw_limit = false;
    while index < tokens.len() {
        match tokens[index].to_ascii_uppercase().as_str() {
            "SORT" => {
                if saw_sort {
                    return Err("SORT may appear only once".to_owned());
                }
                let field = tokens
                    .get(index + 1)
                    .and_then(|value| ScreenField::parse(value))
                    .ok_or_else(|| "SORT requires a known field".to_owned())?;
                let direction = tokens
                    .get(index + 2)
                    .and_then(|value| ScreenSortDirection::parse(value))
                    .ok_or_else(|| "SORT requires ASC or DESC".to_owned())?;
                sort_field = field;
                sort_direction = direction;
                saw_sort = true;
                index += 3;
            }
            "LIMIT" => {
                if saw_limit {
                    return Err("LIMIT may appear only once".to_owned());
                }
                limit = tokens
                    .get(index + 1)
                    .and_then(|value| value.parse::<usize>().ok())
                    .ok_or_else(|| "LIMIT requires an integer".to_owned())?;
                saw_limit = true;
                index += 2;
            }
            token => return Err(format!("unexpected screen token {token}")),
        }
    }
    let label = id.replace(['-', '_'], " ").to_ascii_uppercase();
    ScreenDefinition::new_expression(
        id,
        label,
        universe,
        expression,
        sort_field,
        sort_direction,
        limit,
        false,
    )
    .map_err(|error| error.to_string())
}

fn expand_expression_tokens(args: &[String]) -> Vec<String> {
    let mut tokens = Vec::new();
    for argument in args {
        let mut current = String::new();
        for character in argument.chars() {
            if matches!(character, '(' | ')') {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
                tokens.push(character.to_string());
            } else {
                current.push(character);
            }
        }
        if !current.is_empty() {
            tokens.push(current);
        }
    }
    tokens
}

struct ScreenExpressionParser<'a> {
    tokens: &'a [String],
    index: usize,
}

impl<'a> ScreenExpressionParser<'a> {
    fn new(tokens: &'a [String]) -> Self {
        Self { tokens, index: 0 }
    }

    fn parse(&mut self) -> Result<ScreenExpression, String> {
        let expression = self.parse_or()?;
        if self
            .tokens
            .get(self.index)
            .is_some_and(|token| !matches!(token.to_ascii_uppercase().as_str(), "SORT" | "LIMIT"))
        {
            return Err(format!(
                "unexpected expression token {}",
                self.tokens[self.index]
            ));
        }
        expression.validate().map_err(|error| error.to_string())?;
        Ok(expression)
    }

    fn parse_or(&mut self) -> Result<ScreenExpression, String> {
        let mut items = vec![self.parse_and()?];
        while self.consume("OR") {
            items.push(self.parse_and()?);
        }
        Ok(if items.len() == 1 {
            items.remove(0)
        } else {
            ScreenExpression::Any(items)
        })
    }

    fn parse_and(&mut self) -> Result<ScreenExpression, String> {
        let mut items = vec![self.parse_not()?];
        while self.consume("AND") {
            items.push(self.parse_not()?);
        }
        Ok(if items.len() == 1 {
            items.remove(0)
        } else {
            ScreenExpression::All(items)
        })
    }

    fn parse_not(&mut self) -> Result<ScreenExpression, String> {
        if self.consume("NOT") {
            return Ok(ScreenExpression::Not(Box::new(self.parse_not()?)));
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<ScreenExpression, String> {
        if self.consume("(") {
            let expression = self.parse_or()?;
            if !self.consume(")") {
                return Err("screen expression has an unclosed group".to_owned());
            }
            return Ok(expression);
        }
        let clause = parse_clause(self.tokens, &mut self.index)?;
        Ok(ScreenExpression::Predicate(clause))
    }

    fn consume(&mut self, expected: &str) -> bool {
        if self
            .tokens
            .get(self.index)
            .is_some_and(|token| token.eq_ignore_ascii_case(expected))
        {
            self.index += 1;
            true
        } else {
            false
        }
    }
}

fn collect_expression_clauses(expression: &ScreenExpression, target: &mut Vec<ScreenClause>) {
    match expression {
        ScreenExpression::Predicate(clause) => target.push(clause.clone()),
        ScreenExpression::All(items) | ScreenExpression::Any(items) => {
            for item in items {
                collect_expression_clauses(item, target);
            }
        }
        ScreenExpression::Not(item) => collect_expression_clauses(item, target),
    }
}

fn parse_version(value: &str) -> Option<u64> {
    let value = value.trim();
    if let Some(value) = value.strip_prefix('V').or_else(|| value.strip_prefix('v')) {
        return u64::from_str_radix(value, 16).ok();
    }
    if let Some(value) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        return u64::from_str_radix(value, 16).ok();
    }
    value
        .parse::<u64>()
        .ok()
        .or_else(|| u64::from_str_radix(value, 16).ok())
}

fn parse_clause(args: &[String], index: &mut usize) -> Result<ScreenClause, String> {
    let field = args
        .get(*index)
        .and_then(|value| ScreenField::parse(value))
        .ok_or_else(|| "clause requires a known field".to_owned())?;
    let comparison = args
        .get(*index + 1)
        .and_then(|value| Comparison::parse(value))
        .ok_or_else(|| "clause requires >, >=, <, <=, or =".to_owned())?;
    let value = args
        .get(*index + 2)
        .ok_or_else(|| "clause requires a numeric threshold".to_owned())
        .and_then(|value| parse_threshold(field, value))?;
    *index += 3;
    ScreenClause::new(field, comparison, value).map_err(|error| error.to_string())
}

fn parse_threshold(field: ScreenField, input: &str) -> Result<f64, String> {
    let normalized = input.trim().to_ascii_lowercase().replace('_', "");
    let suffixes = ["bps", "bp", "pct", "%", "k", "m", "b"];
    let suffix = suffixes
        .into_iter()
        .find(|suffix| normalized.ends_with(suffix));
    let number = suffix.map_or(normalized.as_str(), |suffix| {
        &normalized[..normalized.len().saturating_sub(suffix.len())]
    });
    let value = number
        .parse::<f64>()
        .map_err(|_| "clause requires a numeric threshold".to_owned())?;
    let multiplier = match (field.dimension(), suffix) {
        (_, None) | (ScreenDimension::Percent, Some("%" | "pct")) => 1.0,
        (ScreenDimension::BasisPoints, Some("bp" | "bps")) => 1.0,
        (ScreenDimension::Quantity, Some("k")) => 1_000.0,
        (ScreenDimension::Quantity, Some("m")) => 1_000_000.0,
        (ScreenDimension::Quantity, Some("b")) => 1_000_000_000.0,
        (dimension, Some(unit)) => {
            return Err(format!(
                "unit {unit} is incompatible with {} ({dimension:?})",
                field.key()
            ));
        }
    };
    let value = value * multiplier;
    if !value.is_finite() {
        return Err("screen threshold must be finite".to_owned());
    }
    Ok(value)
}

fn restore_identity_field(
    state: &WorkspaceViewState,
    field: &str,
    label: &str,
    report: &mut ViewRestoreReport,
) -> Option<String> {
    let value = state.fields.get(field)?;
    let Some(id) = value.as_text().filter(|id| {
        !id.is_empty() && id.len() <= 128 && id.trim() == *id && !id.chars().any(char::is_control)
    }) else {
        report.skipped_fields += 1;
        report
            .warnings
            .push(format!("saved Screening {label} identity is invalid"));
        return None;
    };
    report.restored_fields += 1;
    Some(id.to_owned())
}

struct ScreenAreas {
    header: Rect,
    expression: Rect,
    table: Rect,
    detail: Rect,
    footer: Rect,
}

fn screen_areas(area: Rect) -> ScreenAreas {
    let parts = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(2),
        Constraint::Min(7),
        Constraint::Length(6),
        Constraint::Length(1),
    ])
    .split(area);
    ScreenAreas {
        header: parts[0],
        expression: parts[1],
        table: parts[2],
        detail: parts[3],
        footer: parts[4],
    }
}

fn format_optional(value: Option<f64>, decimals: usize) -> String {
    value.map_or_else(|| "--".to_owned(), |value| format!("{value:.decimals$}"))
}

fn format_signed_percent(value: Option<f64>) -> String {
    value.map_or_else(|| "--".to_owned(), |value| format!("{value:+.2}%"))
}

fn format_volume(value: Option<f64>) -> String {
    value.map_or_else(
        || "--".to_owned(),
        |value| {
            if value.abs() >= 1_000_000.0 {
                format!("{:.2}M", value / 1_000_000.0)
            } else {
                format!("{value:.0}")
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Mutex,
        },
        thread,
        time::Duration,
    };

    use super::*;
    use crate::features::screening::{ScreeningError, UniverseMember, UniverseSnapshot};
    use crate::foundation::InstrumentId;

    struct FixtureQuery;

    impl ScreeningUniverseQuery for FixtureQuery {
        fn load_universe(&self, id: &str) -> Result<UniverseSnapshot, ScreeningError> {
            UniverseSnapshot::new(
                id,
                "TEST UNIVERSE",
                42,
                "2026-08-29T10:00:00Z",
                "FIXTURE",
                (0..24)
                    .map(|index| UniverseMember {
                        instrument_id: InstrumentId::new(format!("us:test:{index:03}")),
                        symbol: format!("T{index:03}"),
                        description: format!("Test {index}"),
                        currency: "USD".to_owned(),
                        last: Some(100.0 + index as f64),
                        change_percent: Some(index as f64 / 10.0),
                        volume: Some(10_000_000.0 + index as f64 * 1_000_000.0),
                        spread_bps: Some(5.0 - index as f64 / 10.0),
                        day_range_percent: Some(1.0),
                        quality: "REALTIME".to_owned(),
                        provider: "fixture".to_owned(),
                    })
                    .collect(),
            )
            .map_err(|error| ScreeningError::InvalidSnapshot(error.to_string()))
        }
    }

    struct CountingQuery(Arc<AtomicUsize>);

    impl ScreeningUniverseQuery for CountingQuery {
        fn load_universe(&self, id: &str) -> Result<UniverseSnapshot, ScreeningError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            FixtureQuery.load_universe(id)
        }
    }

    #[derive(Default)]
    struct MemoryStore(Mutex<Option<ScreenCatalogState>>);

    impl ScreenStateStore for MemoryStore {
        fn load_screens(&self) -> Result<Option<ScreenCatalogState>, ScreenStateError> {
            Ok(self.0.lock().unwrap().clone())
        }

        fn save_screens(&self, state: &ScreenCatalogState) -> Result<(), ScreenStateError> {
            *self.0.lock().unwrap() = Some(state.clone());
            Ok(())
        }
    }

    struct MemoryHistoryStore {
        manifest: Mutex<UniverseHistoryManifest>,
        snapshots: Mutex<BTreeMap<u64, UniverseSnapshot>>,
    }

    impl Default for MemoryHistoryStore {
        fn default() -> Self {
            Self {
                manifest: Mutex::new(UniverseHistoryManifest::empty()),
                snapshots: Mutex::new(BTreeMap::new()),
            }
        }
    }

    impl UniverseHistoryStore for MemoryHistoryStore {
        fn load_history(&self) -> Result<UniverseHistoryManifest, ScreenStateError> {
            Ok(self.manifest.lock().unwrap().clone())
        }

        fn load_snapshot(&self, version: u64) -> Result<UniverseSnapshot, ScreenStateError> {
            self.snapshots
                .lock()
                .unwrap()
                .get(&version)
                .cloned()
                .ok_or_else(|| ScreenStateError::Corrupt("snapshot missing".to_owned()))
        }

        fn record_snapshot(
            &self,
            snapshot: &UniverseSnapshot,
        ) -> Result<UniverseHistoryManifest, ScreenStateError> {
            let mut manifest = self.manifest.lock().unwrap();
            let (next, evicted) = manifest
                .record(snapshot)
                .map_err(|error| ScreenStateError::Corrupt(error.to_string()))?;
            let mut snapshots = self.snapshots.lock().unwrap();
            if let Some(existing) = snapshots.get(&snapshot.version) {
                if existing != snapshot {
                    return Err(ScreenStateError::Corrupt(
                        "immutable snapshot changed".to_owned(),
                    ));
                }
            } else {
                snapshots.insert(snapshot.version, snapshot.clone());
            }
            if let Some(evicted) = evicted {
                snapshots.remove(&evicted.version);
            }
            *manifest = next.clone();
            Ok(next)
        }
    }

    fn settle(workspace: &mut ScreeningWorkspace) {
        for _ in 0..100 {
            workspace.poll_intents();
            if workspace.applied_generation == workspace.desired_generation {
                return;
            }
            thread::sleep(Duration::from_millis(1));
        }
        panic!("screen did not settle");
    }

    #[test]
    fn command_saves_multi_clause_screen_and_persists_definition() {
        let store = Arc::new(MemoryStore::default());
        let mut workspace = ScreeningWorkspace::persistent(Arc::new(FixtureQuery), store.clone());
        settle(&mut workspace);
        workspace.handle_command(&CommandInvocation {
            function: "SCREEN".to_owned(),
            args: vec![
                "SAVE".to_owned(),
                "leaders".to_owned(),
                "core".to_owned(),
                "change_pct".to_owned(),
                ">=".to_owned(),
                "1".to_owned(),
                "AND".to_owned(),
                "volume".to_owned(),
                ">=".to_owned(),
                "20000000".to_owned(),
                "SORT".to_owned(),
                "change_pct".to_owned(),
                "DESC".to_owned(),
                "LIMIT".to_owned(),
                "7".to_owned(),
            ],
        });
        settle(&mut workspace);
        for _ in 0..100 {
            workspace.poll_intents();
            if store.0.lock().unwrap().is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(1));
        }

        let definition = workspace.active_definition();
        assert_eq!(definition.id, "leaders");
        assert_eq!(definition.clauses.len(), 2);
        assert_eq!(definition.limit, 7);
        assert_eq!(workspace.evaluation.as_ref().unwrap().rows.len(), 7);
        assert_eq!(
            store.0.lock().unwrap().as_ref().unwrap().screens[0].id,
            "leaders"
        );
    }

    #[test]
    fn actions_use_visible_ranked_identity_and_reject_stale_rows() {
        let mut workspace = ScreeningWorkspace::new(Arc::new(FixtureQuery));
        settle(&mut workspace);
        workspace.selected = workspace.evaluation.as_ref().unwrap().rows.len() - 1;
        let area = Rect::new(0, 0, 80, 24);
        let actions = workspace.actions(area);
        let row = actions
            .iter()
            .find(|action| action.id.starts_with("row:") && action.preferred)
            .unwrap()
            .clone();
        assert!(workspace.activate_action(&row.id));
        assert!(
            matches!(workspace.poll_intents().as_slice(), [AppIntent::DispatchCommand { command, origin }] if command.starts_with("SEC ") && *origin == ID)
        );

        workspace.evaluation.as_mut().unwrap().rows[workspace.selected]
            .member
            .instrument_id = InstrumentId::new("replacement");
        assert!(!workspace.activate_action(&row.id));
    }

    #[test]
    fn saved_view_restores_screen_and_pending_row_identity() {
        let mut source = ScreeningWorkspace::new(Arc::new(FixtureQuery));
        settle(&mut source);
        source.selected = 3;
        source.viewport_top.set(2);
        let state = source.capture_view();

        let mut restored = ScreeningWorkspace::new(Arc::new(FixtureQuery));
        let report = restored.restore_view(&state);
        assert_eq!(report.restored_fields, 3);
        settle(&mut restored);
        assert_eq!(restored.capture_view(), state);
    }

    #[test]
    fn invalid_custom_syntax_and_builtin_mutation_fail_closed() {
        let mut workspace = ScreeningWorkspace::new(Arc::new(FixtureQuery));
        workspace.handle_command(&CommandInvocation {
            function: "SCREEN".to_owned(),
            args: vec!["SAVE".to_owned(), "bad".to_owned()],
        });
        assert!(workspace.status.contains("SAVE ERROR"));
        workspace.delete_definition("momentum");
        assert!(workspace.status.contains("PROTECTED"));

        workspace.activate_screen("momentum");
        workspace.cycle_screen(-1);
        assert_eq!(workspace.active_screen_id, "tight-spread");
        workspace.cycle_screen(1);
        assert_eq!(workspace.active_screen_id, "momentum");
        assert_eq!(parse_version("42"), Some(42));
        assert_eq!(parse_version("0x42"), Some(66));
        assert_eq!(parse_version("V0000000000000042"), Some(66));
    }

    #[test]
    fn custom_parser_supports_nested_logic_precedence_and_typed_units() {
        let args = [
            "quality-move",
            "core",
            "(",
            "change_pct",
            ">=",
            "1%",
            "OR",
            "volume",
            ">=",
            "20m",
            ")",
            "AND",
            "NOT",
            "spread_bps",
            ">",
            "5bps",
            "SORT",
            "change_pct",
            "DESC",
            "LIMIT",
            "25",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
        let definition = parse_saved_definition(&args).unwrap();
        assert!(definition.expression_tree.is_some());
        assert_eq!(definition.clauses.len(), 3);
        assert_eq!(definition.clauses[1].value, 20_000_000.0);
        assert_eq!(
            definition.expression(),
            "(% CHG >= 1.00% OR VOLUME >= 20.00M) AND NOT SPREAD BP > 5.00BP"
        );

        let mut incompatible = args;
        incompatible[5] = "1bps".to_owned();
        assert!(parse_saved_definition(&incompatible)
            .unwrap_err()
            .contains("incompatible"));

        let attached = [
            "attached",
            "core",
            "(change_pct",
            ">=",
            "1%",
            "OR",
            "volume",
            ">=",
            "20m)",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
        assert!(parse_saved_definition(&attached).is_ok());
    }

    #[test]
    fn selected_result_routes_directly_to_chart() {
        let mut workspace = ScreeningWorkspace::new(Arc::new(FixtureQuery));
        settle(&mut workspace);
        assert!(workspace.handle_key(KeyEvent::from(KeyCode::Char('c'))));
        assert!(
            matches!(workspace.poll_intents().as_slice(), [AppIntent::DispatchCommand { command, origin }] if command.starts_with("CHART ") && *origin == ID)
        );
    }

    #[test]
    fn live_input_is_recorded_and_exactly_replayed_after_restart() {
        let screens = Arc::new(MemoryStore::default());
        let history = Arc::new(MemoryHistoryStore::default());
        let provider_calls = Arc::new(AtomicUsize::new(0));
        let mut workspace = ScreeningWorkspace::persistent_with_history(
            Arc::new(CountingQuery(provider_calls.clone())),
            screens.clone(),
            history.clone(),
        );
        settle(&mut workspace);
        assert_eq!(provider_calls.load(Ordering::SeqCst), 1);
        let live = workspace.evaluation.clone().unwrap();
        assert_eq!(workspace.history.entries.len(), 1);
        assert_eq!(workspace.history.entries[0].version, 42);

        workspace.handle_command(&CommandInvocation {
            function: "SCREEN".to_owned(),
            args: vec!["HISTORY".to_owned(), "core".to_owned()],
        });
        assert!(workspace.status.contains("V000000000000002A"));
        workspace.handle_command(&CommandInvocation {
            function: "SCREEN".to_owned(),
            args: vec!["REPLAY".to_owned(), "V000000000000002A".to_owned()],
        });
        settle(&mut workspace);
        assert_eq!(workspace.replay_version, Some(42));
        assert_eq!(workspace.evaluation.as_ref().unwrap(), &live);
        assert_eq!(provider_calls.load(Ordering::SeqCst), 1);

        drop(workspace);
        let mut restarted = ScreeningWorkspace::persistent_with_history(
            Arc::new(CountingQuery(provider_calls.clone())),
            screens,
            history,
        );
        settle(&mut restarted);
        assert_eq!(provider_calls.load(Ordering::SeqCst), 2);
        assert_eq!(restarted.history.entries.len(), 1);
        restarted.handle_command(&CommandInvocation {
            function: "SCREEN".to_owned(),
            args: vec!["REPLAY".to_owned(), "0x2a".to_owned()],
        });
        settle(&mut restarted);
        assert_eq!(restarted.evaluation.as_ref().unwrap(), &live);
        assert_eq!(provider_calls.load(Ordering::SeqCst), 2);
        restarted.handle_command(&CommandInvocation {
            function: "SCREEN".to_owned(),
            args: vec!["LIVE".to_owned()],
        });
        assert_eq!(restarted.replay_version, None);
        settle(&mut restarted);
        assert_eq!(provider_calls.load(Ordering::SeqCst), 3);
    }
}
