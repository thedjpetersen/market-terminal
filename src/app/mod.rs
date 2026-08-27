mod desk;
mod events;
mod input;
mod settings;
mod workspace;

use std::{collections::BTreeMap, sync::Arc};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use crate::{
    features::persistence::{SessionState, SessionStateRepository},
    ui::{self, ShellClickTarget},
};

pub use desk::{DeskWorkspace, DESK_ID};
pub use events::{EventBus, EventEnvelope, EventTopic, SubscriptionId, SubscriptionMetrics};
pub use input::{CommandEditMode, InputMode};
pub use settings::RuntimeSettingsSummary;
pub use workspace::{
    AppIntent, CommandArgument, CommandInvocation, CommandParseError, ShellChrome, ShellContext,
    Workspace, WorkspaceDescriptor, WorkspaceId, WorkspaceNavigationItem, WorkspaceRegistry,
};

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
    pub(crate) tmux_prefix_pending: bool,
    assistant_workspace: Option<WorkspaceId>,
    assistant_drawer_visible: bool,
    pub(crate) ticks: u64,
    pub(crate) workspaces: WorkspaceRegistry,
    events: EventBus,
    persistence: Option<Arc<dyn SessionStateRepository>>,
    persistence_error: Option<String>,
    recent_commands: Vec<String>,
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
            tmux_prefix_pending: false,
            assistant_workspace,
            assistant_drawer_visible: false,
            ticks: 0,
            workspaces,
            events: EventBus::default(),
            persistence: None,
            persistence_error: None,
            recent_commands: Vec::new(),
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

    pub fn with_runtime_settings(mut self, settings: RuntimeSettingsSummary) -> Self {
        self.runtime_settings = settings;
        self
    }

    pub fn tmux_prefix_pending(&self) -> bool {
        self.tmux_prefix_pending
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
        self.workspaces.update_shell_context(self.active_workspace);
        let intents = self.workspaces.poll_intents();
        for intent in intents {
            self.apply_intent(intent);
        }
        self.workspaces.update_shell_context(self.active_workspace);
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
            match key.code {
                KeyCode::Esc | KeyCode::F(2) | KeyCode::Char('q' | 'Q') => {
                    self.close_settings();
                }
                KeyCode::F(1) => {
                    self.close_settings();
                    self.help_visible = true;
                }
                KeyCode::Char('/' | ':') => {
                    self.close_settings();
                    self.open_command();
                }
                _ => {}
            }
            return;
        }

        if self.help_visible {
            match key.code {
                KeyCode::Esc | KeyCode::F(1) | KeyCode::Char('q' | 'Q') => {
                    self.help_visible = false;
                }
                KeyCode::Char('/' | ':') => {
                    self.help_visible = false;
                    self.open_command();
                }
                KeyCode::F(2) => {
                    self.help_visible = false;
                    self.open_settings();
                }
                _ => {}
            }
            return;
        }

        if self.input_mode == InputMode::Command {
            self.handle_command_key(key);
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

        if self.tmux_prefix_pending {
            self.tmux_prefix_pending = false;
            self.handle_tmux_key(key);
            return;
        }
        if key.code == KeyCode::Char('b') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.tmux_prefix_pending = true;
            return;
        }

        if key.code == KeyCode::F(1) {
            self.help_visible = true;
            return;
        }
        if key.code == KeyCode::F(2) {
            self.open_settings();
            return;
        }

        // The active feature owns navigation-mode input first. This is essential
        // for editors and other modal workspaces whose keystrokes may overlap
        // application navigation and command-palette bindings.
        if self.workspaces.handle_key(self.active_workspace, key) {
            return;
        }

        match input::navigation_action(key) {
            input::NavigationAction::Quit => self.should_quit = true,
            input::NavigationAction::OpenCommand => self.open_command(),
            input::NavigationAction::Hotkey(character) => {
                if let Some(id) = self.workspaces.resolve_hotkey(character) {
                    if Some(id) == self.assistant_workspace {
                        self.toggle_assistant_drawer();
                    } else if self.active_workspace != id {
                        self.workspaces.on_blur(self.active_workspace);
                        self.active_workspace = id;
                        self.persist_session();
                    }
                }
            }
            input::NavigationAction::Delegate => {}
        }
    }

    /// Applies a terminal mouse event using the geometry of the last rendered frame.
    pub fn handle_mouse(&mut self, event: MouseEvent, frame_area: Rect) {
        self.tmux_prefix_pending = false;
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
                    if self.active_workspace != id {
                        self.workspaces.on_blur(self.active_workspace);
                        self.active_workspace = id;
                        self.persist_session();
                    }
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
            Some(ShellClickTarget::SettingsOverlay) => {}
            Some(ShellClickTarget::Quit) => self.should_quit = true,
            None => {}
        }
    }

    fn execute_command(&mut self) {
        let command = self.command.trim().to_owned();
        let opens_help = CommandInvocation::parse(&command)
            .is_some_and(|invocation| invocation.function == "HELP");
        let opens_settings = CommandInvocation::parse(&command).is_some_and(|invocation| {
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
        } else if let Some(id) = self.workspaces.dispatch_command(&command) {
            if Some(id) == self.assistant_workspace {
                self.open_assistant_drawer();
            } else {
                self.close_assistant_drawer();
            }
            if Some(id) != self.assistant_workspace && self.active_workspace != id {
                self.workspaces.on_blur(self.active_workspace);
                self.active_workspace = id;
            }
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

    fn open_command(&mut self) {
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
        self.workspaces.on_blur(self.active_workspace);
        self.active_workspace = target;
        self.persist_session();
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
                self.workspaces.on_blur(previous_active);
            }
            self.persist_session();
        }
    }

    fn persist_session(&mut self) {
        let Some(repository) = self.persistence.clone() else {
            return;
        };
        let state = SessionState::new(
            Some(self.active_workspace.as_str().to_owned()),
            self.workspaces
                .workspace_order()
                .into_iter()
                .map(|id| id.as_str().to_owned())
                .collect(),
            self.recent_commands.clone(),
            BTreeMap::new(),
        );
        self.persistence_error = match state {
            Ok(state) => repository.save(&state).err().map(|error| error.to_string()),
            Err(error) => Some(error.to_string()),
        };
    }
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
    use std::sync::Mutex;

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
    fn clicking_a_portfolio_position_opens_its_security() {
        let mut app = bootstrap::demo_app();
        app.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE));

        app.handle_mouse(left_click(2, 12), Rect::new(0, 0, 160, 48));
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

        let close =
            crate::ui::settings_close_area(crate::ui::ShellLayout::new(frame_area).workspace);
        app.handle_mouse(left_click(close.x + 1, close.y), frame_area);
        assert!(!app.settings_visible());
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
