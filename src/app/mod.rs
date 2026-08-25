mod events;
mod input;
mod workspace;

use crossterm::event::KeyEvent;

pub use input::InputMode;
pub use events::{EventBus, EventEnvelope, EventTopic, SubscriptionId, SubscriptionMetrics};
pub use workspace::{
    AppIntent, CommandInvocation, Workspace, WorkspaceDescriptor, WorkspaceId,
    ShellContext, WorkspaceNavigationItem, WorkspaceRegistry,
};

pub struct App {
    pub(crate) active_workspace: WorkspaceId,
    pub(crate) command: String,
    pub(crate) input_mode: InputMode,
    pub(crate) ticks: u64,
    pub(crate) workspaces: WorkspaceRegistry,
    events: EventBus,
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
            should_quit: false,
        }
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
                    self.command.push(character.to_ascii_uppercase());
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
                }
            }
            input::NavigationAction::Delegate => {}
        }
    }

    fn execute_command(&mut self) {
        if let Some(id) = self.workspaces.dispatch_command(&self.command) {
            self.active_workspace = id;
        }
        self.command.clear();
        self.input_mode = InputMode::Navigation;
    }

    fn apply_intent(&mut self, intent: AppIntent) {
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
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::{layout::Rect, Frame};

    use super::*;
    use crate::{
        bootstrap,
        features::{
            assistant::ID as ASSISTANT, news::ID as NEWS, overview::ID as OVERVIEW,
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
