mod command_inference;
mod desk;
mod events;
mod input;
mod keymap;
mod settings;
mod workspace;

use std::{
    collections::BTreeMap,
    sync::{mpsc, Arc},
    thread,
};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use crate::{
    features::persistence::{SessionState, SessionStateRepository},
    ui::{self, ShellClickTarget},
};

pub use command_inference::{
    CommandInference, CommandInferenceError, CommandInferenceRequest, InferredCommand,
};
pub use desk::{DeskWorkspace, DESK_ID};
pub use events::{
    CommandDispatched, EventBus, EventEnvelope, EventTopic, SubscriptionId, SubscriptionMetrics,
    WorkspaceActivated,
};
pub use input::{CommandEditMode, InputMode};
pub(crate) use keymap::{Keymap, ShellAction};
pub use settings::RuntimeSettingsSummary;
pub use workspace::{
    AppIntent, CommandArgument, CommandInvocation, CommandParseError, ShellChrome, ShellContext,
    Workspace, WorkspaceAction, WorkspaceDescriptor, WorkspaceId, WorkspaceNavigationItem,
    WorkspaceRegistry,
};

const HINT_ALPHABET: &[u8] = b"ASDFGHJKLQWERTYUIOPZXCVBNM";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ShellHintTarget {
    Workspace(WorkspaceId),
    WorkspaceAction {
        workspace: WorkspaceId,
        action: String,
    },
    Command,
    Help,
    Settings,
    Quit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ShellHint {
    pub code: String,
    pub target: ShellHintTarget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HintMode {
    input: String,
    hints: Vec<ShellHint>,
    workspace_area: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpatialDirection {
    Left,
    Right,
    Up,
    Down,
}

struct PendingCommandInference {
    input: String,
    result: mpsc::Receiver<Result<InferredCommand, CommandInferenceError>>,
}

pub struct App {
    pub(crate) active_workspace: WorkspaceId,
    pub(crate) command: String,
    pub(crate) input_mode: InputMode,
    command_edit_mode: CommandEditMode,
    command_cursor: usize,
    command_delete_pending: bool,
    history_cursor: Option<usize>,
    pub(crate) help_visible: bool,
    pub(crate) settings_visible: bool,
    pub(crate) settings_first_run: bool,
    runtime_settings: RuntimeSettingsSummary,
    keymap: Keymap,
    pub(crate) tmux_prefix_pending: bool,
    panel_focus: bool,
    focused_action: Option<String>,
    hint_mode: Option<HintMode>,
    terminal_area: Rect,
    assistant_workspace: Option<WorkspaceId>,
    assistant_drawer_visible: bool,
    pub(crate) ticks: u64,
    pub(crate) workspaces: WorkspaceRegistry,
    events: EventBus,
    command_inference: Option<Arc<dyn CommandInference>>,
    pending_command_inference: Option<PendingCommandInference>,
    command_feedback: Option<String>,
    persistence: Option<Arc<dyn SessionStateRepository>>,
    persistence_error: Option<String>,
    recent_commands: Vec<String>,
    preferences: BTreeMap<String, String>,
    theme_name: String,
    should_quit: bool,
}

impl App {
    pub fn new(mut workspaces: WorkspaceRegistry, initial_workspace: WorkspaceId) -> Self {
        let assistant_workspace = workspaces.resolve_target("assistant");
        workspaces.update_shell_context(initial_workspace);
        Self {
            active_workspace: initial_workspace,
            command: String::new(),
            input_mode: InputMode::Navigation,
            command_edit_mode: CommandEditMode::Insert,
            command_cursor: 0,
            command_delete_pending: false,
            history_cursor: None,
            help_visible: false,
            settings_visible: false,
            settings_first_run: false,
            runtime_settings: RuntimeSettingsSummary::demo(),
            keymap: Keymap::default(),
            tmux_prefix_pending: false,
            panel_focus: false,
            focused_action: None,
            hint_mode: None,
            terminal_area: Rect::new(0, 0, 120, 36),
            assistant_workspace,
            assistant_drawer_visible: false,
            ticks: 0,
            workspaces,
            events: EventBus::default(),
            command_inference: None,
            pending_command_inference: None,
            command_feedback: None,
            persistence: None,
            persistence_error: None,
            recent_commands: Vec::new(),
            preferences: BTreeMap::new(),
            theme_name: crate::ui::theme::active_theme_name().to_owned(),
            should_quit: false,
        }
    }

    /// Attaches durable shell state and restores the last valid layout.
    ///
    /// Persistence failures never prevent the terminal from starting. The
    /// adapter retains a previous valid generation, while the shell exposes a
    /// diagnostic for status surfaces and continues with defaults.
    pub fn with_session_repository(mut self, repository: Arc<dyn SessionStateRepository>) -> Self {
        match repository.load() {
            Ok(Some(state)) => {
                self.workspaces
                    .apply_workspace_order(state.workspace_order());
                if let Some(active) = state
                    .active_workspace()
                    .and_then(|target| self.workspaces.resolve_target(target))
                {
                    if Some(active) == self.assistant_workspace {
                        self.assistant_drawer_visible = true;
                        self.workspaces.on_focus(active);
                    } else {
                        self.active_workspace = active;
                    }
                }
                self.recent_commands = state.recent_commands().to_vec();
                self.preferences = state.preferences().clone();
                if let Some(name) = state
                    .preferences()
                    .get("theme")
                    .and_then(|name| crate::ui::theme::set_theme(name))
                {
                    self.theme_name = name.to_owned();
                }
            }
            Ok(None) => {
                self.settings_visible = true;
                self.settings_first_run = true;
            }
            Err(error) => self.persistence_error = Some(error.to_string()),
        }
        self.persistence = Some(repository);
        self.workspaces.update_shell_context(self.active_workspace);
        self
    }

    pub fn active_workspace(&self) -> WorkspaceId {
        self.active_workspace
    }

    pub fn input_mode(&self) -> InputMode {
        self.input_mode
    }

    pub fn command(&self) -> &str {
        &self.command
    }

    pub fn command_edit_mode(&self) -> CommandEditMode {
        self.command_edit_mode
    }

    pub fn command_cursor(&self) -> usize {
        self.command_cursor
    }

    pub fn command_feedback(&self) -> Option<&str> {
        self.command_feedback.as_deref()
    }

    pub fn command_inference_pending(&self) -> bool {
        self.pending_command_inference.is_some()
    }

    pub fn help_visible(&self) -> bool {
        self.help_visible
    }

    pub fn settings_visible(&self) -> bool {
        self.settings_visible
    }

    pub fn settings_first_run(&self) -> bool {
        self.settings_first_run
    }

    pub fn runtime_settings(&self) -> &RuntimeSettingsSummary {
        &self.runtime_settings
    }

    pub fn theme_name(&self) -> &str {
        &self.theme_name
    }

    pub fn with_runtime_settings(mut self, settings: RuntimeSettingsSummary) -> Self {
        self.runtime_settings = settings;
        self
    }

    pub fn with_command_inference(mut self, inference: Arc<dyn CommandInference>) -> Self {
        self.command_inference = Some(inference);
        self
    }

    pub(crate) fn with_keymap(mut self, keymap: Keymap) -> Self {
        self.keymap = keymap;
        self
    }

    pub(crate) fn key_labels(&self, actions: &[ShellAction]) -> String {
        self.keymap.labels(actions)
    }

    pub fn tmux_prefix_pending(&self) -> bool {
        self.tmux_prefix_pending
    }

    pub fn panel_focus(&self) -> bool {
        self.panel_focus
    }

    pub(crate) fn focused_workspace_action(&self, area: Rect) -> Option<WorkspaceAction> {
        let focused = self.focused_action.as_deref()?;
        self.workspace_actions(area, workspace::MAX_WORKSPACE_ACTIONS)
            .into_iter()
            .find(|action| action.id == focused)
    }

    pub(crate) fn focused_action_label(&self) -> Option<String> {
        let area = ui::ShellLayout::new(self.terminal_area).workspace;
        self.focused_workspace_action(area)
            .map(|action| action.label)
    }

    pub fn set_terminal_area(&mut self, area: Rect) {
        if area.width == 0 || area.height == 0 || area == self.terminal_area {
            return;
        }
        self.terminal_area = area;
        if self.hint_mode.take().is_some() {
            self.command_feedback = Some("HINTS CANCELED · TERMINAL RESIZED".to_owned());
        }
        if self.panel_focus {
            self.refresh_spatial_focus();
        }
    }

    pub(crate) fn workspace_actions(&self, area: Rect, limit: usize) -> Vec<WorkspaceAction> {
        self.workspaces.actions(self.active_workspace, area, limit)
    }

    pub(crate) fn shell_hints(&self) -> Option<(&str, &[ShellHint])> {
        self.hint_mode
            .as_ref()
            .map(|mode| (mode.input.as_str(), mode.hints.as_slice()))
    }

    pub fn assistant_drawer_visible(&self) -> bool {
        self.assistant_drawer_visible
    }

    pub fn assistant_workspace(&self) -> Option<WorkspaceId> {
        self.assistant_workspace
    }

    pub fn ticks(&self) -> u64 {
        self.ticks
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn persistence_error(&self) -> Option<&str> {
        self.persistence_error.as_deref()
    }

    pub fn recent_commands(&self) -> &[String] {
        &self.recent_commands
    }

    pub fn events(&self) -> &EventBus {
        &self.events
    }

    pub fn events_mut(&mut self) -> &mut EventBus {
        &mut self.events
    }

    pub fn advance_tick(&mut self) {
        self.ticks = self.ticks.wrapping_add(1);
        self.poll_command_inference();
        self.workspaces.update_shell_context(self.active_workspace);
        if self.panel_focus {
            self.refresh_spatial_focus();
        }
        let intents = self.workspaces.poll_intents();
        for intent in intents {
            self.apply_intent(intent);
        }
        self.workspaces.update_shell_context(self.active_workspace);
        if self.panel_focus {
            self.refresh_spatial_focus();
        }
    }

    /// Applies a key press to application state without performing terminal I/O.
    ///
    /// Keeping input handling independent of the runtime makes the application
    /// state machine directly usable from integration tests and alternate hosts.
    pub fn handle_key(&mut self, key: KeyEvent) {
        if input::is_force_quit(key) {
            self.should_quit = true;
            return;
        }

        if self.settings_visible {
            if key.code == KeyCode::Esc {
                self.close_settings();
                return;
            }
            match self.keymap.resolve(key) {
                keymap::BindingMatch::Action {
                    action: ShellAction::Quit | ShellAction::Settings,
                    ..
                } => self.close_settings(),
                keymap::BindingMatch::Action {
                    action: ShellAction::Help,
                    ..
                } => {
                    self.close_settings();
                    self.help_visible = true;
                }
                keymap::BindingMatch::Action {
                    action: ShellAction::OpenCommand,
                    ..
                } => {
                    self.close_settings();
                    self.open_command();
                }
                keymap::BindingMatch::Action {
                    action: ShellAction::NextTheme,
                    ..
                } => {
                    self.cycle_theme(1);
                    self.persist_session();
                }
                keymap::BindingMatch::Action {
                    action: ShellAction::PreviousTheme,
                    ..
                } => {
                    self.cycle_theme(-1);
                    self.persist_session();
                }
                _ => {}
            }
            return;
        }

        if self.help_visible {
            if key.code == KeyCode::Esc {
                self.help_visible = false;
                return;
            }
            match self.keymap.resolve(key) {
                keymap::BindingMatch::Action {
                    action: ShellAction::Quit | ShellAction::Help,
                    ..
                } => self.help_visible = false,
                keymap::BindingMatch::Action {
                    action: ShellAction::OpenCommand,
                    ..
                } => {
                    self.help_visible = false;
                    self.open_command();
                }
                keymap::BindingMatch::Action {
                    action: ShellAction::Settings,
                    ..
                } => {
                    self.help_visible = false;
                    self.open_settings();
                }
                keymap::BindingMatch::Action {
                    action: ShellAction::NextTheme,
                    ..
                } => {
                    self.cycle_theme(1);
                    self.persist_session();
                }
                keymap::BindingMatch::Action {
                    action: ShellAction::PreviousTheme,
                    ..
                } => {
                    self.cycle_theme(-1);
                    self.persist_session();
                }
                _ => {}
            }
            return;
        }

        if self.input_mode == InputMode::Command {
            self.handle_command_key(key);
            return;
        }

        if self.hint_mode.is_some() {
            self.handle_hint_key(key);
            return;
        }

        if self.assistant_drawer_visible {
            if self
                .assistant_workspace
                .is_some_and(|id| self.workspaces.handle_key(id, key))
            {
                return;
            }
            if key.code == KeyCode::Esc {
                self.close_assistant_drawer();
                return;
            }
        }

        if self.panel_focus {
            self.handle_panel_focus_key(key);
            return;
        }

        if key.code == KeyCode::Esc {
            self.enter_panel_focus();
            return;
        }
        if key.code == KeyCode::Char('f')
            && !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            self.open_hints();
            return;
        }

        if self.tmux_prefix_pending {
            self.tmux_prefix_pending = false;
            self.handle_tmux_key(key);
            return;
        }
        if key.code == KeyCode::Char('b') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.tmux_prefix_pending = true;
            return;
        }

        let default_action = match self.keymap.resolve(key) {
            keymap::BindingMatch::Action {
                action,
                customized: true,
            } => {
                self.handle_bound_action(action);
                return;
            }
            keymap::BindingMatch::Disabled => return,
            keymap::BindingMatch::Action {
                action,
                customized: false,
            } => Some(action),
            keymap::BindingMatch::Unmapped => None,
        };

        // Unmapped input remains feature-owned. This preserves modal editors,
        // workspace-specific Vim aliases, and direct navigation hotkeys.
        if self.workspaces.handle_key(self.active_workspace, key) {
            return;
        }
        if let Some(action) = default_action {
            self.handle_bound_action(action);
            return;
        }

        match input::navigation_action(key) {
            input::NavigationAction::Quit => self.should_quit = true,
            input::NavigationAction::OpenCommand => self.open_command(),
            input::NavigationAction::Hotkey(character) => {
                if let Some(id) = self.workspaces.resolve_hotkey(character) {
                    if Some(id) == self.assistant_workspace {
                        self.toggle_assistant_drawer();
                    } else {
                        self.activate_workspace(id);
                    }
                }
            }
            input::NavigationAction::Delegate => {}
        }
    }

    fn handle_bound_action(&mut self, action: ShellAction) {
        match action {
            ShellAction::Quit => self.should_quit = true,
            ShellAction::OpenCommand => self.open_command(),
            ShellAction::NextPanel => self.switch_relative_workspace(true),
            ShellAction::PreviousPanel => self.switch_relative_workspace(false),
            ShellAction::Settings => self.open_settings(),
            ShellAction::Help => self.help_visible = true,
            ShellAction::NextTheme => {
                self.cycle_theme(1);
                self.persist_session();
            }
            ShellAction::PreviousTheme => {
                self.cycle_theme(-1);
                self.persist_session();
            }
            action => {
                let code = match action {
                    ShellAction::Refresh => KeyCode::F(9),
                    ShellAction::Up => KeyCode::Up,
                    ShellAction::Down => KeyCode::Down,
                    ShellAction::Left => KeyCode::Left,
                    ShellAction::Right => KeyCode::Right,
                    ShellAction::PageUp => KeyCode::PageUp,
                    ShellAction::PageDown => KeyCode::PageDown,
                    ShellAction::Open => KeyCode::Enter,
                    _ => unreachable!("shell actions are handled above"),
                };
                let target = if self.assistant_drawer_visible {
                    self.assistant_workspace.unwrap_or(self.active_workspace)
                } else {
                    self.active_workspace
                };
                self.workspaces
                    .handle_key(target, KeyEvent::new(code, KeyModifiers::NONE));
            }
        }
    }

    /// Applies a terminal mouse event using the geometry of the last rendered frame.
    pub fn handle_mouse(&mut self, event: MouseEvent, frame_area: Rect) {
        self.tmux_prefix_pending = false;
        self.panel_focus = false;
        self.focused_action = None;
        self.hint_mode = None;
        let target = ui::hit_test(self, frame_area, event.column, event.row);
        if !matches!(event.kind, MouseEventKind::Down(MouseButton::Left)) {
            if let Some(ShellClickTarget::Workspace(area)) = target {
                self.workspaces
                    .handle_mouse(self.active_workspace, event, area);
            }
            return;
        }

        match target {
            Some(ShellClickTarget::CommandInput) => {
                self.help_visible = false;
                self.close_settings();
                self.close_assistant_drawer();
                if self.input_mode != InputMode::Command {
                    self.workspaces.on_blur(self.active_workspace);
                }
                self.open_command();
            }
            Some(ShellClickTarget::CommandGo) => {
                self.help_visible = false;
                self.close_settings();
                self.close_assistant_drawer();
                if self.input_mode == InputMode::Command {
                    self.execute_command();
                } else {
                    self.workspaces.on_blur(self.active_workspace);
                    self.open_command();
                }
            }
            Some(ShellClickTarget::Navigation(id)) => {
                self.help_visible = false;
                self.close_settings();
                self.cancel_command();
                if Some(id) == self.assistant_workspace {
                    self.toggle_assistant_drawer();
                } else {
                    self.close_assistant_drawer();
                    self.activate_workspace(id);
                }
            }
            Some(ShellClickTarget::Workspace(area)) => {
                if self.input_mode == InputMode::Command {
                    self.cancel_command();
                }
                self.workspaces
                    .handle_mouse(self.active_workspace, event, area);
            }
            Some(ShellClickTarget::AssistantDrawer(area)) => {
                if let Some(id) = self.assistant_workspace {
                    self.workspaces.handle_mouse(id, event, area);
                }
            }
            Some(ShellClickTarget::AssistantClose) | Some(ShellClickTarget::AssistantBackdrop) => {
                self.close_assistant_drawer()
            }
            Some(ShellClickTarget::HelpClose) => self.help_visible = false,
            Some(ShellClickTarget::HelpOverlay) => {}
            Some(ShellClickTarget::SettingsClose) => self.close_settings(),
            Some(ShellClickTarget::SettingsThemePrevious) => {
                self.cycle_theme(-1);
                self.persist_session();
            }
            Some(ShellClickTarget::SettingsThemeNext) => {
                self.cycle_theme(1);
                self.persist_session();
            }
            Some(ShellClickTarget::SettingsOverlay) => {}
            Some(ShellClickTarget::Quit) => self.should_quit = true,
            None => {}
        }
    }

    fn execute_command(&mut self) {
        let command = self.command.trim().to_owned();
        let invocation = CommandInvocation::parse(&command);
        let mut command_target = None;
        let mut inference_pending = false;
        let opens_help = invocation
            .as_ref()
            .is_some_and(|invocation| invocation.function == "HELP");
        let opens_settings = invocation.as_ref().is_some_and(|invocation| {
            matches!(
                invocation.function.as_str(),
                "SETTINGS" | "CONFIG" | "SETUP"
            )
        });
        if opens_help {
            self.settings_visible = false;
            self.help_visible = true;
        } else if opens_settings {
            self.open_settings();
        } else if let Some(invocation) = invocation
            .as_ref()
            .filter(|invocation| invocation.function == "THEME")
        {
            self.apply_theme_command(invocation);
        } else if self.workspaces.resolve_command(&command).is_some() {
            command_target = self.dispatch_workspace_command(&command);
        } else if !command.is_empty() {
            inference_pending = self.start_command_inference(command.clone());
        }
        if !command.is_empty() && !inference_pending {
            self.events.publish(CommandDispatched {
                command: command.clone(),
                target: command_target,
            });
            tracing::debug!(
                command = invocation
                    .as_ref()
                    .map_or("INVALID", |invocation| invocation.function.as_str()),
                target = command_target.map(WorkspaceId::as_str),
                "command dispatched"
            );
        }
        if !command.is_empty() && command.len() <= 512 {
            self.recent_commands.retain(|recent| recent != &command);
            self.recent_commands.insert(0, command);
            self.recent_commands.truncate(100);
        }
        self.command.clear();
        self.input_mode = InputMode::Navigation;
        self.reset_command_editor();
        self.persist_session();
    }

    fn dispatch_workspace_command(&mut self, command: &str) -> Option<WorkspaceId> {
        let id = self.workspaces.dispatch_command(command)?;
        if Some(id) == self.assistant_workspace {
            self.open_assistant_drawer();
        } else {
            self.close_assistant_drawer();
        }
        if Some(id) != self.assistant_workspace && self.active_workspace != id {
            let previous = self.active_workspace;
            self.workspaces.on_blur(previous);
            self.active_workspace = id;
            self.publish_workspace_activation(previous);
        }
        Some(id)
    }

    fn start_command_inference(&mut self, input: String) -> bool {
        let Some(inference) = self.command_inference.clone() else {
            self.command_feedback = Some("AI INFERENCE IS NOT CONFIGURED".to_owned());
            return false;
        };
        if input.len() > 512
            || input
                .chars()
                .any(|character| matches!(character, '\r' | '\n'))
        {
            self.command_feedback = Some("AI INFERENCE REJECTED UNBOUNDED INPUT".to_owned());
            return false;
        }
        let request = CommandInferenceRequest {
            input: input.clone(),
            active_workspace: self.active_workspace.as_str().to_owned(),
            available_workspaces: self
                .workspaces
                .workspace_order()
                .into_iter()
                .map(|id| id.as_str().to_owned())
                .collect(),
            available_commands: self.workspaces.command_aliases(),
        };
        let (sender, result) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("market-terminal-command-inference".to_owned())
            .spawn(move || {
                let _ = sender.send(inference.infer(request));
            });
        if worker.is_err() {
            self.command_feedback = Some("AI INFERENCE WORKER COULD NOT START".to_owned());
            return false;
        }
        self.pending_command_inference = Some(PendingCommandInference {
            input: input.clone(),
            result,
        });
        self.command_feedback = Some(format!("AI INFERRING · {input}"));
        true
    }

    fn poll_command_inference(&mut self) {
        let outcome = self.pending_command_inference.as_ref().and_then(|pending| {
            match pending.result.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => Some(Err(
                    CommandInferenceError::Provider("AI inference worker stopped".to_owned()),
                )),
            }
        });
        let Some(outcome) = outcome else {
            return;
        };
        let original = self
            .pending_command_inference
            .take()
            .expect("completed inference is pending")
            .input;
        let command_target = match outcome {
            Ok(inferred)
                if inferred.command().len() <= 512
                    && !inferred
                        .command()
                        .chars()
                        .any(|character| matches!(character, '\r' | '\n'))
                    && self
                        .workspaces
                        .resolve_command(inferred.command())
                        .is_some() =>
            {
                let target = self.dispatch_workspace_command(inferred.command());
                self.command_feedback = Some(format!(
                    "AI · {} → {}",
                    inferred.model(),
                    inferred.command()
                ));
                tracing::debug!(
                    input = original,
                    command = inferred.command(),
                    model = inferred.model(),
                    target = target.map(WorkspaceId::as_str),
                    "AI command inference dispatched"
                );
                target
            }
            Ok(inferred) => {
                self.command_feedback =
                    Some(format!("AI INFERENCE REJECTED · {}", inferred.command()));
                None
            }
            Err(error) => {
                self.command_feedback = Some(format!("AI INFERENCE FAILED · {error}"));
                None
            }
        };
        self.events.publish(CommandDispatched {
            command: original,
            target: command_target,
        });
    }

    fn apply_theme_command(&mut self, invocation: &CommandInvocation) {
        let Some(argument) = invocation.args.first() else {
            self.cycle_theme(1);
            return;
        };
        if invocation.args.len() != 1 {
            return;
        }
        match argument.trim().to_ascii_lowercase().as_str() {
            "next" => self.cycle_theme(1),
            "prev" | "previous" => self.cycle_theme(-1),
            "list" => self.open_settings(),
            name => {
                if let Some(name) = crate::ui::theme::set_theme(name) {
                    self.theme_name = name.to_owned();
                }
            }
        }
    }

    fn cycle_theme(&mut self, direction: isize) {
        self.theme_name = crate::ui::theme::cycle_theme(direction).to_owned();
    }

    fn open_command(&mut self) {
        self.command_feedback = None;
        self.input_mode = InputMode::Command;
        self.command_edit_mode = CommandEditMode::Insert;
        self.command_cursor = self.command.len();
        self.command_delete_pending = false;
        self.history_cursor = None;
    }

    fn cancel_command(&mut self) {
        self.command.clear();
        self.input_mode = InputMode::Navigation;
        self.reset_command_editor();
    }

    fn reset_command_editor(&mut self) {
        self.command_edit_mode = CommandEditMode::Insert;
        self.command_cursor = 0;
        self.command_delete_pending = false;
        self.history_cursor = None;
    }

    fn handle_command_key(&mut self, key: KeyEvent) {
        let action = input::command_action(key, self.command_edit_mode);
        if action != input::CommandAction::DeleteOperator {
            self.command_delete_pending = false;
        }
        match action {
            input::CommandAction::Cancel => self.cancel_command(),
            input::CommandAction::Execute => self.execute_command(),
            input::CommandAction::EnterNormal => {
                if self.command.is_empty() {
                    self.cancel_command();
                } else {
                    self.command_edit_mode = CommandEditMode::Normal;
                    self.command_cursor =
                        previous_char_boundary(&self.command, self.command_cursor);
                }
            }
            input::CommandAction::EnterInsert => {
                self.command_edit_mode = CommandEditMode::Insert;
            }
            input::CommandAction::AppendAfter => {
                self.command_cursor = next_char_boundary(&self.command, self.command_cursor);
                self.command_edit_mode = CommandEditMode::Insert;
            }
            input::CommandAction::InsertAtStart => {
                self.command_cursor = 0;
                self.command_edit_mode = CommandEditMode::Insert;
            }
            input::CommandAction::InsertAtEnd => {
                self.command_cursor = self.command.len();
                self.command_edit_mode = CommandEditMode::Insert;
            }
            input::CommandAction::Insert(character) => self.insert_command_character(character),
            input::CommandAction::Backspace => self.delete_command_backward(),
            input::CommandAction::DeleteAt => self.delete_command_at_cursor(),
            input::CommandAction::DeleteToEnd => {
                self.command.truncate(self.command_cursor);
                self.normalize_normal_cursor();
            }
            input::CommandAction::DeletePreviousWord => {
                let start = previous_word_start(&self.command, self.command_cursor);
                self.command.drain(start..self.command_cursor);
                self.command_cursor = start;
                self.history_cursor = None;
                self.normalize_normal_cursor();
            }
            input::CommandAction::DeleteOperator => {
                if self.command_delete_pending {
                    self.command.clear();
                    self.command_cursor = 0;
                    self.command_delete_pending = false;
                    self.history_cursor = None;
                } else {
                    self.command_delete_pending = true;
                }
            }
            input::CommandAction::Clear => {
                self.command.clear();
                self.command_cursor = 0;
                self.history_cursor = None;
            }
            input::CommandAction::MoveLeft => {
                self.command_cursor = previous_char_boundary(&self.command, self.command_cursor);
            }
            input::CommandAction::MoveRight => {
                let next = next_char_boundary(&self.command, self.command_cursor);
                if self.command_edit_mode == CommandEditMode::Insert || next < self.command.len() {
                    self.command_cursor = next;
                }
            }
            input::CommandAction::MoveStart => self.command_cursor = 0,
            input::CommandAction::MoveEnd => {
                self.command_cursor = if self.command_edit_mode == CommandEditMode::Normal
                    && !self.command.is_empty()
                {
                    previous_char_boundary(&self.command, self.command.len())
                } else {
                    self.command.len()
                };
            }
            input::CommandAction::MoveWordForward => {
                self.command_cursor = next_word_start(&self.command, self.command_cursor);
                self.normalize_normal_cursor();
            }
            input::CommandAction::MoveWordBackward => {
                self.command_cursor = previous_word_start(&self.command, self.command_cursor);
            }
            input::CommandAction::HistoryPrevious => self.select_command_history(true),
            input::CommandAction::HistoryNext => self.select_command_history(false),
            input::CommandAction::None => {}
        }
    }

    fn insert_command_character(&mut self, character: char) {
        if self.command.len() + character.len_utf8() > workspace::MAX_COMMAND_BYTES {
            return;
        }
        self.command.insert(self.command_cursor, character);
        self.command_cursor += character.len_utf8();
        self.history_cursor = None;
    }

    fn delete_command_backward(&mut self) {
        let previous = previous_char_boundary(&self.command, self.command_cursor);
        if previous == self.command_cursor {
            return;
        }
        self.command.drain(previous..self.command_cursor);
        self.command_cursor = previous;
        self.history_cursor = None;
    }

    fn delete_command_at_cursor(&mut self) {
        let next = next_char_boundary(&self.command, self.command_cursor);
        if next == self.command_cursor {
            return;
        }
        self.command.drain(self.command_cursor..next);
        self.history_cursor = None;
        self.normalize_normal_cursor();
    }

    fn normalize_normal_cursor(&mut self) {
        if self.command_edit_mode == CommandEditMode::Normal
            && self.command_cursor == self.command.len()
            && !self.command.is_empty()
        {
            self.command_cursor = previous_char_boundary(&self.command, self.command_cursor);
        }
    }

    fn select_command_history(&mut self, previous: bool) {
        if self.recent_commands.is_empty() {
            return;
        }
        let index = if previous {
            self.history_cursor
                .map(|index| (index + 1).min(self.recent_commands.len() - 1))
                .unwrap_or(0)
        } else {
            let Some(index) = self.history_cursor else {
                return;
            };
            if index == 0 {
                self.history_cursor = None;
                self.command.clear();
                self.command_cursor = 0;
                return;
            }
            index - 1
        };
        self.history_cursor = Some(index);
        self.command.clone_from(&self.recent_commands[index]);
        self.command_cursor = self.command.len();
        self.normalize_normal_cursor();
    }

    fn handle_tmux_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Left | KeyCode::Up | KeyCode::Char('p' | 'P') => {
                self.switch_relative_workspace(false);
            }
            KeyCode::Right | KeyCode::Down | KeyCode::Char('n' | 'N') => {
                self.switch_relative_workspace(true);
            }
            KeyCode::Char('?') => self.help_visible = true,
            KeyCode::Char(character @ '1'..='9') => {
                self.switch_to_navigation_index(character as usize - '1' as usize);
            }
            KeyCode::Char('0') => self.switch_to_navigation_index(9),
            _ => {}
        }
    }

    fn enter_panel_focus(&mut self) {
        self.close_assistant_drawer();
        self.workspaces.on_blur(self.active_workspace);
        self.panel_focus = true;
        self.focused_action = None;
        self.refresh_spatial_focus();
        self.command_feedback = None;
    }

    fn handle_panel_focus_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Left => self.move_spatial_focus(SpatialDirection::Left),
            KeyCode::Right => self.move_spatial_focus(SpatialDirection::Right),
            KeyCode::Up => self.move_spatial_focus(SpatialDirection::Up),
            KeyCode::Down => self.move_spatial_focus(SpatialDirection::Down),
            KeyCode::Enter => self.activate_spatial_focus(),
            KeyCode::Esc => {
                self.panel_focus = false;
                self.focused_action = None;
                self.workspaces.on_focus(self.active_workspace);
            }
            KeyCode::Char('f' | 'F') => self.open_hints(),
            KeyCode::Char(character @ '1'..='9') => {
                self.switch_to_navigation_index(character as usize - '1' as usize);
                self.refresh_spatial_focus();
            }
            KeyCode::Char('0') => {
                self.switch_to_navigation_index(9);
                self.refresh_spatial_focus();
            }
            KeyCode::Char('q' | 'Q') => self.should_quit = true,
            _ => {}
        }
    }

    fn refresh_spatial_focus(&mut self) {
        if !self.panel_focus {
            self.focused_action = None;
            return;
        }
        let area = ui::ShellLayout::new(self.terminal_area).workspace;
        let actions = self.workspace_actions(area, workspace::MAX_WORKSPACE_ACTIONS);
        if self
            .focused_action
            .as_ref()
            .is_some_and(|focused| actions.iter().any(|action| action.id == *focused))
        {
            return;
        }
        self.focused_action = actions
            .iter()
            .find(|action| action.preferred)
            .or_else(|| actions.first())
            .map(|action| action.id.clone());
    }

    fn move_spatial_focus(&mut self, direction: SpatialDirection) {
        let area = ui::ShellLayout::new(self.terminal_area).workspace;
        let actions = self.workspace_actions(area, workspace::MAX_WORKSPACE_ACTIONS);
        if actions.is_empty() {
            self.switch_relative_workspace(matches!(
                direction,
                SpatialDirection::Right | SpatialDirection::Down
            ));
            self.focused_action = None;
            self.refresh_spatial_focus();
            return;
        }

        let Some(current) = self
            .focused_action
            .as_deref()
            .and_then(|focused| actions.iter().find(|action| action.id == focused))
        else {
            self.focused_action = actions
                .iter()
                .find(|action| action.preferred)
                .or_else(|| actions.first())
                .map(|action| action.id.clone());
            self.command_feedback = Some("FOCUS RESET · VISIBLE ACTIONS CHANGED".to_owned());
            return;
        };

        if let Some(target) = spatial_target(&actions, current, direction) {
            self.focused_action = Some(target.id.clone());
            self.command_feedback = None;
        }
    }

    fn activate_spatial_focus(&mut self) {
        let area = ui::ShellLayout::new(self.terminal_area).workspace;
        let Some(action) = self.focused_action.clone() else {
            self.panel_focus = false;
            self.workspaces.on_focus(self.active_workspace);
            return;
        };
        if self
            .workspaces
            .activate_action(self.active_workspace, &action, area)
        {
            self.panel_focus = false;
            self.focused_action = None;
            self.workspaces.on_focus(self.active_workspace);
        } else {
            self.focused_action = None;
            self.refresh_spatial_focus();
            self.command_feedback = Some(format!("ACTION UNAVAILABLE · {action}"));
        }
    }

    fn open_hints(&mut self) {
        self.close_assistant_drawer();
        self.help_visible = false;
        self.close_settings();
        self.panel_focus = false;
        self.focused_action = None;
        let mut targets = self
            .workspaces
            .navigation_items()
            .map(|item| ShellHintTarget::Workspace(item.id))
            .chain([
                ShellHintTarget::Command,
                ShellHintTarget::Help,
                ShellHintTarget::Settings,
                ShellHintTarget::Quit,
            ])
            .collect::<Vec<_>>();
        let workspace_area = ui::ShellLayout::new(self.terminal_area).workspace;
        let remaining = HINT_ALPHABET.len().pow(2).saturating_sub(targets.len());
        targets.extend(
            self.workspace_actions(workspace_area, remaining)
                .into_iter()
                .map(|action| ShellHintTarget::WorkspaceAction {
                    workspace: self.active_workspace,
                    action: action.id,
                }),
        );
        let hints = hint_codes(targets.len())
            .zip(targets)
            .map(|(code, target)| ShellHint { code, target })
            .collect();
        self.hint_mode = Some(HintMode {
            input: String::new(),
            hints,
            workspace_area,
        });
    }

    fn handle_hint_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.hint_mode = None;
                return;
            }
            KeyCode::Backspace => {
                if let Some(mode) = &mut self.hint_mode {
                    mode.input.pop();
                }
                return;
            }
            KeyCode::Char(character)
                if character.is_ascii_alphabetic()
                    && !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                if let Some(mode) = &mut self.hint_mode {
                    mode.input.push(character.to_ascii_uppercase());
                }
            }
            _ => return,
        }

        let Some(mode) = &self.hint_mode else {
            return;
        };
        let input = mode.input.clone();
        let matches = mode
            .hints
            .iter()
            .filter(|hint| hint.code.starts_with(&input))
            .collect::<Vec<_>>();
        if matches.is_empty() {
            self.hint_mode = None;
            self.command_feedback = Some(format!("NO HINT · {input}"));
            return;
        }
        let exact = matches
            .iter()
            .find(|hint| hint.code == input)
            .map(|hint| hint.target.clone());
        if let Some(target) = exact {
            let workspace_area = mode.workspace_area;
            self.hint_mode = None;
            self.activate_hint(target, workspace_area);
        }
    }

    fn activate_hint(&mut self, target: ShellHintTarget, workspace_area: Rect) {
        match target {
            ShellHintTarget::Workspace(id) if Some(id) == self.assistant_workspace => {
                self.open_assistant_drawer();
            }
            ShellHintTarget::Workspace(id) => {
                self.activate_workspace(id);
                self.workspaces.on_focus(id);
            }
            ShellHintTarget::WorkspaceAction { workspace, action } => {
                if workspace != self.active_workspace
                    || !self
                        .workspaces
                        .activate_action(workspace, &action, workspace_area)
                {
                    self.command_feedback = Some(format!("ACTION UNAVAILABLE · {action}"));
                }
            }
            ShellHintTarget::Command => self.open_command(),
            ShellHintTarget::Help => self.help_visible = true,
            ShellHintTarget::Settings => self.open_settings(),
            ShellHintTarget::Quit => self.should_quit = true,
        }
    }

    fn switch_relative_workspace(&mut self, forward: bool) {
        let navigation = self
            .workspaces
            .navigation_items()
            .map(|item| item.id)
            .collect::<Vec<_>>();
        let Some(current) = navigation
            .iter()
            .position(|id| *id == self.active_workspace)
        else {
            if let Some(first) = navigation.first().copied() {
                self.activate_workspace(first);
            }
            return;
        };
        let target = if forward {
            (current + 1) % navigation.len()
        } else {
            (current + navigation.len() - 1) % navigation.len()
        };
        self.activate_workspace(navigation[target]);
    }

    fn switch_to_navigation_index(&mut self, index: usize) {
        let target = self
            .workspaces
            .navigation_items()
            .nth(index)
            .map(|item| item.id);
        if let Some(target) = target {
            self.activate_workspace(target);
        }
    }

    fn activate_workspace(&mut self, target: WorkspaceId) {
        if self.active_workspace == target {
            return;
        }
        let previous = self.active_workspace;
        self.workspaces.on_blur(previous);
        self.active_workspace = target;
        self.publish_workspace_activation(previous);
        self.persist_session();
        self.focused_action = None;
        if self.panel_focus {
            self.refresh_spatial_focus();
        }
    }

    fn publish_workspace_activation(&mut self, previous: WorkspaceId) {
        self.events.publish(WorkspaceActivated {
            previous,
            current: self.active_workspace,
        });
        tracing::debug!(
            previous = previous.as_str(),
            current = self.active_workspace.as_str(),
            "workspace activated"
        );
    }

    fn open_assistant_drawer(&mut self) {
        let Some(id) = self.assistant_workspace else {
            return;
        };
        if !self.assistant_drawer_visible {
            self.assistant_drawer_visible = true;
            self.workspaces.on_focus(id);
        }
    }

    fn close_assistant_drawer(&mut self) {
        let Some(id) = self.assistant_workspace else {
            return;
        };
        if self.assistant_drawer_visible {
            self.assistant_drawer_visible = false;
            self.workspaces.on_blur(id);
        }
    }

    fn toggle_assistant_drawer(&mut self) {
        if self.assistant_drawer_visible {
            self.close_assistant_drawer();
        } else {
            self.open_assistant_drawer();
        }
    }

    fn open_settings(&mut self) {
        self.help_visible = false;
        self.close_assistant_drawer();
        self.settings_visible = true;
    }

    fn close_settings(&mut self) {
        if self.settings_visible || self.settings_first_run {
            self.settings_visible = false;
            self.settings_first_run = false;
            self.persist_session();
        }
    }

    fn apply_intent(&mut self, intent: AppIntent) {
        let previous_active = self.active_workspace;
        let previous_order = self.workspaces.workspace_order();
        match intent {
            AppIntent::ActivateWorkspace { target } => {
                if let Some(id) = self.workspaces.resolve_target(&target) {
                    if Some(id) == self.assistant_workspace {
                        self.open_assistant_drawer();
                    } else {
                        self.active_workspace = id;
                    }
                }
            }
            AppIntent::BringWorkspaceForward { target } => {
                if let Some(id) = self.workspaces.resolve_target(&target) {
                    self.workspaces.bring_forward(id);
                    if Some(id) == self.assistant_workspace {
                        self.open_assistant_drawer();
                    } else {
                        self.active_workspace = id;
                    }
                }
            }
            AppIntent::DispatchCommand { command, origin } => {
                let destination = self.workspaces.resolve_command(&command);
                if destination == Some(origin) {
                    return;
                }
                if let Some(id) = self.workspaces.dispatch_command(&command) {
                    self.events.publish(CommandDispatched {
                        command: command.clone(),
                        target: Some(id),
                    });
                    if Some(id) == self.assistant_workspace {
                        self.open_assistant_drawer();
                    } else {
                        self.active_workspace = id;
                    }
                }
            }
            AppIntent::RestoreWorkspaceOrder => self.workspaces.restore_order(),
        }
        if previous_active != self.active_workspace
            || previous_order != self.workspaces.workspace_order()
        {
            if previous_active != self.active_workspace {
                self.focused_action = None;
                self.workspaces.on_blur(previous_active);
                self.publish_workspace_activation(previous_active);
            }
            self.persist_session();
        }
    }

    fn persist_session(&mut self) {
        let Some(repository) = self.persistence.clone() else {
            return;
        };
        self.preferences
            .insert("theme".to_owned(), self.theme_name.clone());
        let state = SessionState::new(
            Some(self.active_workspace.as_str().to_owned()),
            self.workspaces
                .workspace_order()
                .into_iter()
                .map(|id| id.as_str().to_owned())
                .collect(),
            self.recent_commands.clone(),
            self.preferences.clone(),
        );
        self.persistence_error = match state {
            Ok(state) => repository.save(&state).err().map(|error| error.to_string()),
            Err(error) => Some(error.to_string()),
        };
        if let Some(error) = self.persistence_error.as_deref() {
            tracing::warn!(error, "shell session persistence failed");
        }
    }
}

fn spatial_target<'a>(
    actions: &'a [WorkspaceAction],
    current: &WorkspaceAction,
    direction: SpatialDirection,
) -> Option<&'a WorkspaceAction> {
    actions
        .iter()
        .filter(|candidate| candidate.id != current.id)
        .filter_map(|candidate| {
            spatial_score(current.area, candidate.area, direction).map(|score| (candidate, score))
        })
        .min_by(|(left_action, left_score), (right_action, right_score)| {
            left_score
                .cmp(right_score)
                .then_with(|| left_action.area.y.cmp(&right_action.area.y))
                .then_with(|| left_action.area.x.cmp(&right_action.area.x))
                .then_with(|| left_action.id.cmp(&right_action.id))
        })
        .map(|(action, _)| action)
}

fn spatial_score(
    current: Rect,
    candidate: Rect,
    direction: SpatialDirection,
) -> Option<(u8, u16, u16, u32)> {
    let (primary_gap, orthogonal_gap, orthogonal_center_delta) = match direction {
        SpatialDirection::Left if candidate.right() <= current.x => (
            current.x.saturating_sub(candidate.right()),
            interval_gap(current.y, current.bottom(), candidate.y, candidate.bottom()),
            center_twice(current.y, current.height)
                .abs_diff(center_twice(candidate.y, candidate.height)),
        ),
        SpatialDirection::Right if candidate.x >= current.right() => (
            candidate.x.saturating_sub(current.right()),
            interval_gap(current.y, current.bottom(), candidate.y, candidate.bottom()),
            center_twice(current.y, current.height)
                .abs_diff(center_twice(candidate.y, candidate.height)),
        ),
        SpatialDirection::Up if candidate.bottom() <= current.y => (
            current.y.saturating_sub(candidate.bottom()),
            interval_gap(current.x, current.right(), candidate.x, candidate.right()),
            center_twice(current.x, current.width)
                .abs_diff(center_twice(candidate.x, candidate.width)),
        ),
        SpatialDirection::Down if candidate.y >= current.bottom() => (
            candidate.y.saturating_sub(current.bottom()),
            interval_gap(current.x, current.right(), candidate.x, candidate.right()),
            center_twice(current.x, current.width)
                .abs_diff(center_twice(candidate.x, candidate.width)),
        ),
        _ => return None,
    };
    Some((
        u8::from(orthogonal_gap > 0),
        primary_gap,
        orthogonal_gap,
        orthogonal_center_delta,
    ))
}

fn interval_gap(first_start: u16, first_end: u16, second_start: u16, second_end: u16) -> u16 {
    if second_start >= first_end {
        second_start.saturating_sub(first_end)
    } else if first_start >= second_end {
        first_start.saturating_sub(second_end)
    } else {
        0
    }
}

fn center_twice(start: u16, length: u16) -> u32 {
    u32::from(start)
        .saturating_mul(2)
        .saturating_add(u32::from(length.saturating_sub(1)))
}

fn hint_codes(count: usize) -> impl Iterator<Item = String> {
    let alphabet = HINT_ALPHABET
        .iter()
        .map(|byte| char::from(*byte))
        .collect::<Vec<_>>();
    let radix = alphabet.len();
    let mut width = 1;
    let mut capacity = radix;
    while count > capacity {
        width += 1;
        capacity = capacity.saturating_mul(radix);
    }
    let codes = (0..count)
        .map(|mut index| {
            let mut digits = vec![alphabet[0]; width];
            for digit in digits.iter_mut().rev() {
                *digit = alphabet[index % radix];
                index /= radix;
            }
            digits.into_iter().collect()
        })
        .collect::<Vec<_>>();
    codes.into_iter()
}

fn previous_char_boundary(value: &str, cursor: usize) -> usize {
    value[..cursor.min(value.len())]
        .char_indices()
        .next_back()
        .map(|(index, _)| index)
        .unwrap_or(0)
}

fn next_char_boundary(value: &str, cursor: usize) -> usize {
    let cursor = cursor.min(value.len());
    value[cursor..]
        .chars()
        .next()
        .map(|character| cursor + character.len_utf8())
        .unwrap_or(cursor)
}

fn previous_word_start(value: &str, cursor: usize) -> usize {
    let mut position = cursor.min(value.len());
    while position > 0 {
        let previous = previous_char_boundary(value, position);
        let character = value[previous..position].chars().next().unwrap_or(' ');
        if !character.is_whitespace() {
            break;
        }
        position = previous;
    }
    while position > 0 {
        let previous = previous_char_boundary(value, position);
        let character = value[previous..position].chars().next().unwrap_or(' ');
        if character.is_whitespace() {
            break;
        }
        position = previous;
    }
    position
}

fn next_word_start(value: &str, cursor: usize) -> usize {
    let mut position = cursor.min(value.len());
    while position < value.len() {
        let next = next_char_boundary(value, position);
        let character = value[position..next].chars().next().unwrap_or(' ');
        if character.is_whitespace() {
            break;
        }
        position = next;
    }
    while position < value.len() {
        let next = next_char_boundary(value, position);
        let character = value[position..next].chars().next().unwrap_or(' ');
        if !character.is_whitespace() {
            break;
        }
        position = next;
    }
    position
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use crossterm::event::{
        KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };
    use ratatui::{backend::TestBackend, layout::Rect, Frame, Terminal};

    use super::*;
    use crate::{
        bootstrap,
        features::{
            assistant::ID as ASSISTANT,
            charting::ID as CHARTING,
            news::ID as NEWS,
            overview::ID as OVERVIEW,
            persistence::{PersistenceError, SessionState},
            portfolio::ID as PORTFOLIO,
            security::ID as SECURITY,
        },
    };

    fn left_click(column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    struct FixedCommandInference;

    impl CommandInference for FixedCommandInference {
        fn infer(
            &self,
            request: CommandInferenceRequest,
        ) -> Result<InferredCommand, CommandInferenceError> {
            assert_eq!(request.input, "$meta");
            assert!(request
                .available_commands
                .iter()
                .any(|command| command == "SEC"));
            Ok(InferredCommand::new("SEC META", "test-ai"))
        }
    }

    #[test]
    fn clicking_the_ai_navigation_item_opens_the_drawer() {
        let frame_area = Rect::new(0, 0, 160, 48);
        let mut app = bootstrap::demo_app();
        app.handle_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE));
        let underlying = app.active_workspace();
        let navigation_row = crate::ui::ShellLayout::new(frame_area).navigation.y;
        let assistant_column = (0..frame_area.width)
            .find(|column| {
                crate::ui::hit_test(&app, frame_area, *column, navigation_row)
                    == Some(crate::ui::ShellClickTarget::Navigation(ASSISTANT))
            })
            .expect("assistant navigation item should be clickable");

        app.handle_mouse(left_click(assistant_column, navigation_row), frame_area);

        assert_eq!(app.active_workspace, underlying);
        assert!(app.assistant_drawer_visible());
    }

    #[test]
    fn routed_commands_and_workspace_changes_publish_kernel_events() {
        let mut app = bootstrap::demo_app();
        let subscription = app
            .events_mut()
            .subscribe(["shell.command.dispatched", "shell.workspace.activated"], 8);
        let previous = app.active_workspace();
        app.command = "CHART IBM US Equity".to_owned();
        app.execute_command();

        let events = app.events_mut().drain(subscription);
        assert_eq!(events.len(), 2);
        let activation = events[0]
            .downcast_ref::<WorkspaceActivated>()
            .expect("workspace activation event");
        assert_eq!(activation.previous, previous);
        assert_eq!(activation.current, CHARTING);
        let command = events[1]
            .downcast_ref::<CommandDispatched>()
            .expect("command dispatch event");
        assert_eq!(command.command, "CHART IBM US Equity");
        assert_eq!(command.target, Some(CHARTING));
    }

    #[test]
    fn spreadsheet_and_research_exchange_selection_through_intents() {
        let mut app = bootstrap::demo_app();
        app.command = "SHEET A2".to_owned();
        app.execute_command();
        app.command = "SHEET SEC".to_owned();
        app.execute_command();
        assert_ne!(app.active_workspace(), SECURITY);
        app.advance_tick();
        assert_eq!(app.active_workspace(), SECURITY);

        app.command = "SHEET A20".to_owned();
        app.execute_command();
        app.command = "SEC MSFT US".to_owned();
        app.execute_command();
        app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        app.advance_tick();
        assert_eq!(app.active_workspace().as_str(), "spreadsheet");

        let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
        terminal
            .draw(|frame| crate::ui::render(frame, &app))
            .unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("MSFT US"));
    }

    #[test]
    fn assistant_drawer_renders_over_the_current_workspace_and_closes_by_click() {
        let frame_area = Rect::new(0, 0, 160, 48);
        let mut app = bootstrap::demo_app();
        app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        for character in "summarize risk".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        let mut terminal = Terminal::new(TestBackend::new(160, 48)).unwrap();

        terminal
            .draw(|frame| crate::ui::render(frame, &app))
            .unwrap();

        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("ASSISTANT DRAWER"));
        assert!(rendered.contains("summarize risk"));
        assert_eq!(app.active_workspace(), OVERVIEW);

        let close = crate::ui::assistant_close_area(frame_area);
        app.handle_mouse(left_click(close.x + 1, close.y), frame_area);
        assert!(!app.assistant_drawer_visible());
        assert_eq!(app.active_workspace(), OVERVIEW);
    }

    #[test]
    fn clicking_the_command_box_and_go_dispatches_the_command() {
        let mut app = bootstrap::demo_app();
        app.handle_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE));

        app.handle_mouse(left_click(40, 1), Rect::new(0, 0, 160, 48));
        assert_eq!(app.input_mode, InputMode::Command);
        for character in "PORT".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        app.handle_mouse(left_click(131, 1), Rect::new(0, 0, 160, 48));

        assert_eq!(app.active_workspace, PORTFOLIO);
        assert_eq!(app.input_mode, InputMode::Navigation);
    }

    #[test]
    fn cashtag_command_infers_security_navigation() {
        let mut app = bootstrap::demo_app().with_command_inference(Arc::new(FixedCommandInference));
        let subscription = app.events_mut().subscribe(["shell.command.dispatched"], 2);

        app.command = "$meta".to_owned();
        app.execute_command();

        assert!(app.command_inference_pending());
        assert_eq!(app.active_workspace, OVERVIEW);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        while app.command_inference_pending() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(1));
            app.advance_tick();
        }

        assert!(!app.command_inference_pending());
        assert_eq!(app.active_workspace, SECURITY);
        assert_eq!(app.recent_commands(), ["$meta"]);
        assert_eq!(app.command_feedback(), Some("AI · test-ai → SEC META"));
        let events = app.events_mut().drain(subscription);
        let command = events[0]
            .downcast_ref::<CommandDispatched>()
            .expect("command dispatch event");
        assert_eq!(command.command, "$meta");
        assert_eq!(command.target, Some(SECURITY));
    }

    #[test]
    fn clicking_a_portfolio_position_opens_its_security() {
        let mut app = bootstrap::demo_app();
        app.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE));

        app.handle_mouse(left_click(2, 13), Rect::new(0, 0, 160, 48));
        app.advance_tick();

        assert_eq!(app.active_workspace, SECURITY);
    }

    #[test]
    fn clicking_away_blurs_assistant_composition() {
        let mut app = bootstrap::demo_app();
        app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        assert!(app.assistant_drawer_visible());

        app.handle_mouse(left_click(2, 10), Rect::new(0, 0, 160, 48));
        app.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE));

        assert!(!app.assistant_drawer_visible());
        assert_eq!(app.active_workspace, PORTFOLIO);
    }

    #[test]
    fn clicking_a_market_index_opens_its_chart() {
        let mut app = bootstrap::demo_app();
        app.handle_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE));

        app.handle_mouse(left_click(2, 8), Rect::new(0, 0, 160, 48));
        app.advance_tick();

        assert_eq!(app.active_workspace, CHARTING);
    }

    #[test]
    fn hotkeys_switch_workspaces() {
        let mut app = bootstrap::demo_app();
        app.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE));
        assert_eq!(app.active_workspace, PORTFOLIO);
    }

    #[test]
    fn hotkeys_switch_away_from_the_assistant_when_not_composing() {
        let mut app = bootstrap::demo_app();
        app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        assert_eq!(app.active_workspace, OVERVIEW);
        assert!(app.assistant_drawer_visible());

        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE));

        assert!(!app.assistant_drawer_visible());
        assert_eq!(app.active_workspace, PORTFOLIO);
    }

    #[test]
    fn assistant_composition_can_be_exited_before_using_a_hotkey() {
        let mut app = bootstrap::demo_app();
        app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));

        app.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE));
        assert_eq!(app.active_workspace, OVERVIEW);
        assert!(app.assistant_drawer_visible());

        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE));

        assert!(!app.assistant_drawer_visible());
        assert_eq!(app.active_workspace, PORTFOLIO);
    }

    #[test]
    fn command_palette_opens_after_closing_the_assistant_drawer() {
        let mut app = bootstrap::demo_app();
        app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));

        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));

        assert_eq!(app.input_mode, InputMode::Command);
    }

    #[test]
    fn command_palette_switches_workspaces() {
        let mut app = bootstrap::demo_app();
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for character in "aapl us".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.active_workspace, SECURITY);
        assert!(app.command.is_empty());
        assert_eq!(app.input_mode, InputMode::Navigation);
    }

    #[test]
    fn command_insert_mode_preserves_case_and_supports_history() {
        let mut app = bootstrap::demo_app();
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for character in "PORT IMPORT ~/Downloads/positions.csv".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }

        assert_eq!(app.command(), "PORT IMPORT ~/Downloads/positions.csv");
        assert_eq!(app.command_cursor(), app.command().len());

        app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        for character in "PORT".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));

        assert_eq!(app.command(), "PORT");
    }

    #[test]
    fn command_normal_mode_supports_vim_motions_and_edits() {
        let mut app = bootstrap::demo_app();
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for character in "HEPL".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.command_edit_mode(), CommandEditMode::Normal);

        app.handle_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('P'), KeyModifiers::SHIFT));

        assert_eq!(app.command(), "HELP");
        assert_eq!(app.command_edit_mode(), CommandEditMode::Insert);

        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.help_visible());
    }

    #[test]
    fn command_normal_mode_dd_clears_the_line() {
        let mut app = bootstrap::demo_app();
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for character in "PORT".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));

        assert!(app.command().is_empty());
        assert_eq!(app.command_edit_mode(), CommandEditMode::Normal);
    }

    #[test]
    fn help_command_opens_help_without_replacing_the_active_workspace() {
        let mut app = bootstrap::demo_app();
        let active = app.active_workspace();
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for character in "HELP".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }

        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert!(app.help_visible());
        assert_eq!(app.active_workspace(), active);
        assert_eq!(app.input_mode(), InputMode::Navigation);
    }

    #[test]
    fn escape_closes_help_without_quitting_the_application() {
        let mut app = bootstrap::demo_app();
        app.handle_key(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE));
        assert!(app.help_visible());

        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        assert!(!app.help_visible());
        assert!(!app.should_quit());
    }

    #[test]
    fn help_screen_renders_commands_and_has_a_clickable_close_control() {
        let frame_area = Rect::new(0, 0, 160, 48);
        let mut app = bootstrap::demo_app();
        app.handle_key(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE));
        let mut terminal = Terminal::new(TestBackend::new(160, 48)).unwrap();

        terminal
            .draw(|frame| crate::ui::render(frame, &app))
            .unwrap();

        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("MARKET TERMINAL GUIDE"));
        assert!(rendered.contains("PORT IMPORT <CSV>"));

        let close = crate::ui::help_close_area(crate::ui::ShellLayout::new(frame_area).workspace);
        app.handle_mouse(left_click(close.x + 1, close.y), frame_area);

        assert!(!app.help_visible());
    }

    #[test]
    fn settings_command_opens_a_secret_free_overlay_without_switching_workspaces() {
        let frame_area = Rect::new(0, 0, 160, 48);
        let mut app = bootstrap::demo_app();
        let active = app.active_workspace();
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for character in "SETTINGS".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.settings_visible());
        assert_eq!(app.active_workspace(), active);

        let mut terminal = Terminal::new(TestBackend::new(160, 48)).unwrap();
        terminal
            .draw(|frame| crate::ui::render(frame, &app))
            .unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("EFFECTIVE SETTINGS"));
        assert!(rendered.contains("ACTIVE PROCESS"));
        assert!(rendered.contains("Secrets are never displayed"));

        let initial_theme = app.theme_name().to_owned();
        let next_theme =
            crate::ui::settings_theme_next_area(crate::ui::ShellLayout::new(frame_area).workspace);
        app.handle_mouse(left_click(next_theme.x + 1, next_theme.y), frame_area);
        assert_ne!(app.theme_name(), initial_theme);
        let previous_theme = crate::ui::settings_theme_previous_area(
            crate::ui::ShellLayout::new(frame_area).workspace,
        );
        app.handle_mouse(
            left_click(previous_theme.x + 1, previous_theme.y),
            frame_area,
        );
        assert_eq!(app.theme_name(), initial_theme);

        let close =
            crate::ui::settings_close_area(crate::ui::ShellLayout::new(frame_area).workspace);
        app.handle_mouse(left_click(close.x + 1, close.y), frame_area);
        assert!(!app.settings_visible());
    }

    #[test]
    fn theme_command_and_function_keys_select_and_persist_presets() {
        let repository = Arc::new(MemorySessionRepository {
            state: Mutex::new(Some(
                SessionState::new(None, Vec::new(), Vec::new(), BTreeMap::new())
                    .expect("valid session"),
            )),
        });
        let mut app = bootstrap::demo_app().with_session_repository(repository.clone());
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for character in "THEME NORD".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(app.theme_name(), "nord");
        assert_eq!(
            repository
                .state
                .lock()
                .expect("session lock")
                .as_ref()
                .and_then(|state| state.preferences().get("theme"))
                .map(String::as_str),
            Some("nord")
        );

        app.handle_key(KeyEvent::new(KeyCode::F(3), KeyModifiers::NONE));
        assert_eq!(app.theme_name(), "catppuccin-latte");
        app.handle_key(KeyEvent::new(KeyCode::F(3), KeyModifiers::SHIFT));
        assert_eq!(app.theme_name(), "nord");
        crate::ui::theme::set_theme("default").expect("restore default theme");
    }

    #[test]
    fn f2_opens_settings_and_f1_moves_to_help() {
        let mut app = bootstrap::demo_app();
        app.handle_key(KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE));
        assert!(app.settings_visible());

        app.handle_key(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE));
        assert!(!app.settings_visible());
        assert!(app.help_visible());
    }

    #[test]
    fn configured_shell_bindings_replace_defaults_and_switch_panels() {
        let (keymap, warnings) =
            Keymap::from_spec("help=ctrl-h;next_panel=alt-l;previous_panel=alt-h");
        assert!(warnings.is_empty(), "{warnings:?}");
        let mut app = bootstrap::demo_app().with_keymap(keymap);
        let first = app.active_workspace();
        let second = app.workspaces.navigation_items().nth(1).unwrap().id;

        app.handle_key(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE));
        assert!(!app.help_visible());
        app.handle_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::CONTROL));
        assert!(app.help_visible());
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL));
        assert_eq!(app.active_workspace(), first);
        app.handle_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::ALT));
        assert_eq!(app.active_workspace(), second);
    }

    #[test]
    fn tmux_prefix_switches_to_the_next_and_previous_panels() {
        let mut app = bootstrap::demo_app();
        let first = app.active_workspace();
        let second = app.workspaces.navigation_items().nth(1).unwrap().id;

        app.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL));
        assert!(app.tmux_prefix_pending());
        app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));

        assert_eq!(app.active_workspace(), second);
        assert!(!app.tmux_prefix_pending());

        app.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL));
        app.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE));

        assert_eq!(app.active_workspace(), first);
    }

    #[test]
    fn tmux_prefix_selects_numbered_panels_and_opens_help() {
        let mut app = bootstrap::demo_app();
        let third = app.workspaces.navigation_items().nth(2).unwrap().id;

        app.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL));
        app.handle_key(KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE));
        assert_eq!(app.active_workspace(), third);

        app.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL));
        app.handle_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::SHIFT));

        assert!(app.help_visible());
    }

    #[test]
    fn escape_lifts_focus_and_arrows_move_between_panels() {
        let mut app = bootstrap::demo_app();
        let second = app.workspaces.navigation_items().nth(1).unwrap().id;

        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.panel_focus());

        app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        assert_eq!(app.active_workspace(), second);
        assert!(app.panel_focus());

        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(!app.panel_focus());
    }

    #[test]
    fn spatial_focus_prefers_aligned_neighbors_and_never_wraps() {
        let actions = vec![
            WorkspaceAction::new("current", "Current", Rect::new(10, 10, 6, 2)),
            WorkspaceAction::new("right-diagonal", "Diagonal", Rect::new(16, 2, 4, 1)),
            WorkspaceAction::new("right-aligned", "Aligned", Rect::new(20, 10, 4, 1)),
            WorkspaceAction::new("left", "Left", Rect::new(2, 10, 4, 1)),
            WorkspaceAction::new("up", "Up", Rect::new(10, 4, 6, 2)),
            WorkspaceAction::new("down", "Down", Rect::new(10, 16, 6, 2)),
        ];
        let current = &actions[0];

        assert_eq!(
            spatial_target(&actions, current, SpatialDirection::Right).map(|action| &*action.id),
            Some("right-aligned")
        );
        assert_eq!(
            spatial_target(&actions, current, SpatialDirection::Left).map(|action| &*action.id),
            Some("left")
        );
        assert_eq!(
            spatial_target(&actions, current, SpatialDirection::Up).map(|action| &*action.id),
            Some("up")
        );
        assert_eq!(
            spatial_target(&actions, current, SpatialDirection::Down).map(|action| &*action.id),
            Some("down")
        );
        assert!(spatial_target(&actions, &actions[2], SpatialDirection::Right).is_none());
    }

    #[test]
    fn portfolio_spatial_focus_moves_visible_rows_and_opens_the_target() {
        let mut app = bootstrap::demo_app();
        let frame_area = Rect::new(0, 0, 160, 48);
        app.set_terminal_area(frame_area);
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for character in "PORT".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let workspace_area = crate::ui::ShellLayout::new(frame_area).workspace;

        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        let first = app.focused_workspace_action(workspace_area).unwrap();
        assert!(first.id.starts_with("row:positions:0:"));

        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        let second = app.focused_workspace_action(workspace_area).unwrap();
        assert!(second.id.starts_with("row:positions:1:"));
        assert_ne!(first.id, second.id);

        let mut terminal = Terminal::new(TestBackend::new(160, 48)).unwrap();
        terminal
            .draw(|frame| crate::ui::render(frame, &app))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let focus_index = usize::from(second.area.y) * usize::from(buffer.area.width)
            + usize::from(second.area.x);
        assert_eq!(
            buffer.content()[focus_index].bg,
            crate::ui::theme::CYAN.into()
        );
        assert_eq!(
            buffer.content()[focus_index].fg,
            crate::ui::theme::BG.into()
        );

        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(!app.panel_focus());
        app.advance_tick();
        assert_eq!(
            app.active_workspace(),
            app.workspaces.resolve_target("security").unwrap()
        );
    }

    #[test]
    fn desk_panes_support_spatial_focus_and_follow_hints() {
        let mut app = bootstrap::demo_app();
        let frame_area = Rect::new(0, 0, 160, 48);
        app.set_terminal_area(frame_area);
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for character in "DESK".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let workspace_area = crate::ui::ShellLayout::new(frame_area).workspace;

        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(
            app.focused_workspace_action(workspace_area).unwrap().id,
            "pane:monitor"
        );
        app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        assert_eq!(
            app.focused_workspace_action(workspace_area).unwrap().id,
            "pane:chart"
        );
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(
            app.focused_workspace_action(workspace_area).unwrap().id,
            "pane:chart"
        );

        app.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));
        let news_code = app
            .shell_hints()
            .unwrap()
            .1
            .iter()
            .find(|hint| {
                matches!(
                    &hint.target,
                    ShellHintTarget::WorkspaceAction { action, .. }
                        if action == "pane:news"
                )
            })
            .unwrap()
            .code
            .clone();
        for character in news_code.chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(
            app.focused_workspace_action(workspace_area).unwrap().id,
            "pane:news"
        );

        app.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));
        let monitor_row_code = app
            .shell_hints()
            .unwrap()
            .1
            .iter()
            .find(|hint| {
                matches!(
                    &hint.target,
                    ShellHintTarget::WorkspaceAction { action, .. }
                        if action.starts_with("pane:monitor/row:0:")
                )
            })
            .unwrap()
            .code
            .clone();
        for character in monitor_row_code.chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        app.advance_tick();
        assert_eq!(
            app.active_workspace(),
            app.workspaces.resolve_target("security").unwrap()
        );
    }

    #[test]
    fn follow_hints_route_to_visible_workspaces_and_shell_actions() {
        let mut app = bootstrap::demo_app();
        let second = app.workspaces.navigation_items().nth(1).unwrap().id;

        app.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));
        let code = app
            .shell_hints()
            .unwrap()
            .1
            .iter()
            .find(|hint| hint.target == ShellHintTarget::Workspace(second))
            .unwrap()
            .code
            .clone();
        for character in code.chars() {
            app.handle_key(KeyEvent::new(
                KeyCode::Char(character.to_ascii_lowercase()),
                KeyModifiers::NONE,
            ));
        }
        assert_eq!(app.active_workspace(), second);
        assert!(app.shell_hints().is_none());

        app.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));
        let help_code = app
            .shell_hints()
            .unwrap()
            .1
            .iter()
            .find(|hint| hint.target == ShellHintTarget::Help)
            .unwrap()
            .code
            .clone();
        for character in help_code.chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        assert!(app.help_visible());
    }

    #[test]
    fn follow_hints_activate_feature_owned_tabs_and_rows() {
        let mut app = bootstrap::demo_app();
        app.set_terminal_area(Rect::new(0, 0, 160, 48));
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for character in "PORT".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        app.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));
        let attribution_code = app
            .shell_hints()
            .unwrap()
            .1
            .iter()
            .find(|hint| {
                matches!(
                    &hint.target,
                    ShellHintTarget::WorkspaceAction { action, .. }
                        if action == "view:attribution"
                )
            })
            .unwrap()
            .code
            .clone();
        let shell_layout = crate::ui::ShellLayout::new(Rect::new(0, 0, 160, 48));
        let attribution_area = app
            .workspace_actions(shell_layout.workspace, 128)
            .into_iter()
            .find(|action| action.id == "view:attribution")
            .unwrap()
            .area;
        let mut terminal = Terminal::new(TestBackend::new(160, 48)).unwrap();
        terminal
            .draw(|frame| crate::ui::render(frame, &app))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let badge = (0..attribution_code.len() + 2)
            .map(|offset| {
                let index = usize::from(attribution_area.y) * usize::from(buffer.area.width)
                    + usize::from(attribution_area.x)
                    + offset;
                buffer.content()[index].symbol()
            })
            .collect::<String>();
        assert_eq!(badge, format!(" {attribution_code} "));
        let badge_index = usize::from(attribution_area.y) * usize::from(buffer.area.width)
            + usize::from(attribution_area.x);
        assert_eq!(
            buffer.content()[badge_index].bg,
            crate::ui::theme::AMBER.into()
        );
        assert_eq!(
            buffer.content()[badge_index].fg,
            crate::ui::theme::BG.into()
        );
        for character in attribution_code.chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }

        app.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));
        let row_code = app
            .shell_hints()
            .unwrap()
            .1
            .iter()
            .find(|hint| {
                matches!(
                    &hint.target,
                    ShellHintTarget::WorkspaceAction { action, .. }
                        if action.starts_with("row:attribution:0:")
                )
            })
            .unwrap()
            .code
            .clone();
        for character in row_code.chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        app.advance_tick();

        assert_eq!(
            app.active_workspace(),
            app.workspaces.resolve_target("security").unwrap()
        );
        assert!(app.shell_hints().is_none());
    }

    #[test]
    fn follow_hint_codes_are_prefix_free_and_compact() {
        let short = hint_codes(HINT_ALPHABET.len()).collect::<Vec<_>>();
        assert!(short.iter().all(|code| code.len() == 1));

        let long = hint_codes(HINT_ALPHABET.len() + 1).collect::<Vec<_>>();
        assert!(long.iter().all(|code| code.len() == 2));
        assert_eq!(
            long.iter().collect::<std::collections::HashSet<_>>().len(),
            long.len()
        );

        let exceptional = hint_codes(HINT_ALPHABET.len().pow(2) + 1).collect::<Vec<_>>();
        assert!(exceptional.iter().all(|code| code.len() == 3));
    }

    #[test]
    fn terminal_resize_cancels_stale_action_geometry() {
        let mut app = bootstrap::demo_app();
        app.set_terminal_area(Rect::new(0, 0, 120, 36));
        app.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));
        assert!(app.shell_hints().is_some());

        app.set_terminal_area(Rect::new(0, 0, 100, 30));

        assert!(app.shell_hints().is_none());
        assert_eq!(
            app.command_feedback(),
            Some("HINTS CANCELED · TERMINAL RESIZED")
        );
    }

    #[test]
    fn command_palette_bounds_untrusted_input() {
        let mut app = bootstrap::demo_app();
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for _ in 0..workspace::MAX_COMMAND_BYTES + 32 {
            app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        }
        assert_eq!(app.command.len(), workspace::MAX_COMMAND_BYTES);
    }

    struct CapturingWorkspace;

    const CAPTURING: WorkspaceId = WorkspaceId::new("capturing");

    impl Workspace for CapturingWorkspace {
        fn descriptor(&self) -> WorkspaceDescriptor {
            WorkspaceDescriptor {
                id: CAPTURING,
                label: "CAPTURING",
                hotkey: 'c',
                commands: &["CAPTURE"],
            }
        }

        fn render(&self, _frame: &mut Frame, _area: Rect) {}

        fn handle_key(&mut self, key: KeyEvent) -> bool {
            matches!(key.code, KeyCode::Char('p') | KeyCode::Char('/'))
        }
    }

    #[test]
    fn active_workspace_can_capture_global_hotkey() {
        let registry = WorkspaceRegistry::new(vec![
            Box::new(CapturingWorkspace),
            Box::new(TestWorkspace {
                id: PORTFOLIO,
                hotkey: 'p',
            }),
        ]);
        let mut app = App::new(registry, CAPTURING);

        app.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE));

        assert_eq!(app.active_workspace, CAPTURING);
    }

    #[test]
    fn active_workspace_can_capture_command_palette_key() {
        let mut app = App::new(
            WorkspaceRegistry::new(vec![Box::new(CapturingWorkspace)]),
            CAPTURING,
        );

        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));

        assert_eq!(app.input_mode, InputMode::Navigation);
    }

    #[test]
    fn app_intents_reorder_focus_and_restore_workspaces() {
        let mut app = bootstrap::demo_app();

        app.apply_intent(AppIntent::BringWorkspaceForward {
            target: "news".to_owned(),
        });
        assert_eq!(app.active_workspace(), NEWS);
        assert_eq!(
            app.workspaces.navigation_items().next().map(|item| item.id),
            Some(NEWS)
        );

        app.apply_intent(AppIntent::RestoreWorkspaceOrder);
        assert_eq!(
            app.workspaces.navigation_items().next().map(|item| item.id),
            Some(OVERVIEW)
        );
    }

    #[derive(Default)]
    struct MemorySessionRepository {
        state: Mutex<Option<SessionState>>,
    }

    #[test]
    fn a_missing_session_opens_first_run_setup_once_and_close_creates_state() {
        let repository = Arc::new(MemorySessionRepository::default());
        let mut app = bootstrap::demo_app().with_session_repository(repository.clone());
        assert!(app.settings_visible());
        assert!(app.settings_first_run());

        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        assert!(!app.settings_visible());
        assert!(!app.settings_first_run());
        assert!(repository.state.lock().expect("session lock").is_some());
    }

    impl SessionStateRepository for MemorySessionRepository {
        fn load(&self) -> Result<Option<SessionState>, PersistenceError> {
            Ok(self.state.lock().expect("session lock").clone())
        }

        fn save(&self, state: &SessionState) -> Result<(), PersistenceError> {
            *self.state.lock().expect("session lock") = Some(state.clone());
            Ok(())
        }
    }

    #[test]
    fn session_repository_restores_and_saves_shell_layout() {
        let repository = Arc::new(MemorySessionRepository {
            state: Mutex::new(Some(
                SessionState::new(
                    Some(PORTFOLIO.as_str().to_owned()),
                    vec![PORTFOLIO.as_str().to_owned(), CAPTURING.as_str().to_owned()],
                    vec!["PORT".to_owned()],
                    BTreeMap::new(),
                )
                .expect("valid session"),
            )),
        });
        let registry = WorkspaceRegistry::new(vec![
            Box::new(CapturingWorkspace),
            Box::new(TestWorkspace {
                id: PORTFOLIO,
                hotkey: 'p',
            }),
        ]);

        let mut app = App::new(registry, CAPTURING).with_session_repository(repository.clone());

        assert_eq!(app.active_workspace(), PORTFOLIO);
        assert_eq!(app.workspaces.workspace_order(), vec![PORTFOLIO, CAPTURING]);
        assert_eq!(app.recent_commands(), ["PORT"]);

        app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
        let saved = repository
            .state
            .lock()
            .expect("session lock")
            .clone()
            .expect("saved session");
        assert_eq!(saved.active_workspace(), Some(CAPTURING.as_str()));
        assert_eq!(saved.workspace_order(), &["portfolio", "capturing"]);
    }

    #[test]
    fn unknown_ai_targets_cannot_change_application_state() {
        let mut app = bootstrap::demo_app();
        let initial = app.active_workspace();

        app.apply_intent(AppIntent::ActivateWorkspace {
            target: "shell".to_owned(),
        });

        assert_eq!(app.active_workspace(), initial);
    }

    #[test]
    fn feature_intents_cannot_dispatch_commands_back_to_their_origin() {
        let mut app = bootstrap::demo_app();

        app.apply_intent(AppIntent::DispatchCommand {
            command: "AI repeat forever".to_owned(),
            origin: ASSISTANT,
        });

        assert_eq!(app.active_workspace(), OVERVIEW);
    }

    struct TestWorkspace {
        id: WorkspaceId,
        hotkey: char,
    }

    impl Workspace for TestWorkspace {
        fn descriptor(&self) -> WorkspaceDescriptor {
            WorkspaceDescriptor {
                id: self.id,
                label: "TEST",
                hotkey: self.hotkey,
                commands: &["PORT"],
            }
        }

        fn render(&self, _frame: &mut Frame, _area: Rect) {}
    }
}
