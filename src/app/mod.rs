mod events;
mod input;
mod workspace;

use std::{collections::BTreeMap, sync::Arc};

use crossterm::event::KeyEvent;

use crate::features::persistence::{SessionState, SessionStateRepository};

pub use input::InputMode;
pub use events::{EventBus, EventEnvelope, EventTopic, SubscriptionId, SubscriptionMetrics};
pub use workspace::{
    AppIntent, CommandArgument, CommandInvocation, CommandParseError, Workspace,
    WorkspaceDescriptor, WorkspaceId,
    ShellContext, WorkspaceNavigationItem, WorkspaceRegistry,
};

pub struct App {
    pub(crate) active_workspace: WorkspaceId,
    pub(crate) command: String,
    pub(crate) input_mode: InputMode,
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
        workspaces.update_shell_context(initial_workspace);
        Self {
            active_workspace: initial_workspace,
            command: String::new(),
            input_mode: InputMode::Navigation,
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
    pub fn with_session_repository(
        mut self,
        repository: Arc<dyn SessionStateRepository>,
    ) -> Self {
        match repository.load() {
            Ok(Some(state)) => {
                self.workspaces.apply_workspace_order(state.workspace_order());
                if let Some(active) = state
                    .active_workspace()
                    .and_then(|target| self.workspaces.resolve_target(target))
                {
                    self.active_workspace = active;
                }
                self.recent_commands = state.recent_commands().to_vec();
            }
            Ok(None) => {}
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

    pub fn events(&self) -> &EventBus { &self.events }

    pub fn events_mut(&mut self) -> &mut EventBus { &mut self.events }

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

        if self.input_mode == InputMode::Command {
            match input::command_action(key) {
                input::CommandAction::Cancel => {
                    self.command.clear();
                    self.input_mode = InputMode::Navigation;
                }
                input::CommandAction::Delete => {
                    self.command.pop();
                }
                input::CommandAction::Execute => self.execute_command(),
                input::CommandAction::Append(character) => {
                    if self.command.len() + character.len_utf8() <= workspace::MAX_COMMAND_BYTES {
                        self.command.push(character.to_ascii_uppercase());
                    }
                }
                input::CommandAction::None => {}
            }
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
            input::NavigationAction::OpenCommand => self.input_mode = InputMode::Command,
            input::NavigationAction::Hotkey(character) => {
                if let Some(id) = self.workspaces.resolve_hotkey(character) {
                    self.active_workspace = id;
                    self.persist_session();
                }
            }
            input::NavigationAction::Delegate => {}
        }
    }

    fn execute_command(&mut self) {
        let command = self.command.trim().to_owned();
        if let Some(id) = self.workspaces.dispatch_command(&command) {
            self.active_workspace = id;
        }
        if !command.is_empty() && command.len() <= 512 {
            self.recent_commands.retain(|recent| recent != &command);
            self.recent_commands.insert(0, command);
            self.recent_commands.truncate(100);
        }
        self.command.clear();
        self.input_mode = InputMode::Navigation;
        self.persist_session();
    }

    fn apply_intent(&mut self, intent: AppIntent) {
        let previous_active = self.active_workspace;
        let previous_order = self.workspaces.workspace_order();
        match intent {
            AppIntent::ActivateWorkspace { target } => {
                if let Some(id) = self.workspaces.resolve_target(&target) {
                    self.active_workspace = id;
                }
            }
            AppIntent::BringWorkspaceForward { target } => {
                if let Some(id) = self.workspaces.resolve_target(&target) {
                    self.workspaces.bring_forward(id);
                    self.active_workspace = id;
                }
            }
            AppIntent::DispatchCommand { command, origin } => {
                let destination = self.workspaces.resolve_command(&command);
                if destination == Some(origin) {
                    return;
                }
                if let Some(id) = self.workspaces.dispatch_command(&command) {
                    self.active_workspace = id;
                }
            }
            AppIntent::RestoreWorkspaceOrder => self.workspaces.restore_order(),
        }
        if previous_active != self.active_workspace
            || previous_order != self.workspaces.workspace_order()
        {
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

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::{layout::Rect, Frame};

    use super::*;
    use crate::{
        bootstrap,
        features::{
            assistant::ID as ASSISTANT, news::ID as NEWS, overview::ID as OVERVIEW,
            persistence::{PersistenceError, SessionState},
            portfolio::ID as PORTFOLIO, security::ID as SECURITY,
        },
    };

    #[test]
    fn hotkeys_switch_workspaces() {
        let mut app = bootstrap::demo_app();
        app.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE));
        assert_eq!(app.active_workspace, PORTFOLIO);
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

        app.apply_intent(AppIntent::BringWorkspaceForward { target: "news".to_owned() });
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
            Box::new(TestWorkspace { id: PORTFOLIO, hotkey: 'p' }),
        ]);

        let mut app = App::new(registry, CAPTURING)
            .with_session_repository(repository.clone());

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

        app.apply_intent(AppIntent::ActivateWorkspace { target: "shell".to_owned() });

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
