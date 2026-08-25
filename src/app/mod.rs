mod input;
mod workspace;

use crossterm::event::KeyEvent;

pub use input::InputMode;
pub use workspace::{
    CommandInvocation, Workspace, WorkspaceDescriptor, WorkspaceId, WorkspaceNavigationItem,
    WorkspaceRegistry,
};

pub struct App {
    pub(crate) active_workspace: WorkspaceId,
    pub(crate) command: String,
    pub(crate) input_mode: InputMode,
    pub(crate) ticks: u64,
    pub(crate) workspaces: WorkspaceRegistry,
    should_quit: bool,
}

impl App {
    pub fn new(workspaces: WorkspaceRegistry, initial_workspace: WorkspaceId) -> Self {
        Self {
            active_workspace: initial_workspace,
            command: String::new(),
            input_mode: InputMode::Navigation,
            ticks: 0,
            workspaces,
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

    pub fn advance_tick(&mut self) {
        self.ticks = self.ticks.wrapping_add(1);
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
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::{layout::Rect, Frame};

    use super::*;
    use crate::{
        bootstrap,
        features::{portfolio::ID as PORTFOLIO, security::ID as SECURITY},
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
