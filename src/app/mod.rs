mod input;
mod workspace;

use std::{io, time::Duration};

use crossterm::event::{self, Event, KeyEvent, KeyEventKind};
use ratatui::DefaultTerminal;

use crate::ui;

pub use input::InputMode;
pub use workspace::{Workspace, WorkspaceDescriptor, WorkspaceId, WorkspaceRegistry};

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

    pub fn run(mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        while !self.should_quit {
            terminal.draw(|frame| ui::render(frame, &self))?;
            if event::poll(Duration::from_millis(180))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        self.on_key(key);
                    }
                }
            }
            self.ticks = self.ticks.wrapping_add(1);
        }
        Ok(())
    }

    fn on_key(&mut self, key: KeyEvent) {
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

        match input::navigation_action(key) {
            input::NavigationAction::Quit => self.should_quit = true,
            input::NavigationAction::OpenCommand => self.input_mode = InputMode::Command,
            input::NavigationAction::Hotkey(character) => {
                if let Some(id) = self.workspaces.resolve_hotkey(character) {
                    self.active_workspace = id;
                } else {
                    self.workspaces.handle_key(self.active_workspace, key);
                }
            }
            input::NavigationAction::Delegate => {
                self.workspaces.handle_key(self.active_workspace, key);
            }
        }
    }

    fn execute_command(&mut self) {
        if let Some(id) = self.workspaces.resolve_command(&self.command) {
            self.active_workspace = id;
        }
        self.command.clear();
        self.input_mode = InputMode::Navigation;
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::*;
    use crate::{
        bootstrap,
        features::{portfolio::ID as PORTFOLIO, security::ID as SECURITY},
    };

    #[test]
    fn hotkeys_switch_workspaces() {
        let mut app = bootstrap::demo_app();
        app.on_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE));
        assert_eq!(app.active_workspace, PORTFOLIO);
    }

    #[test]
    fn command_palette_switches_workspaces() {
        let mut app = bootstrap::demo_app();
        app.on_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for character in "aapl us".chars() {
            app.on_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.active_workspace, SECURITY);
        assert!(app.command.is_empty());
        assert_eq!(app.input_mode, InputMode::Navigation);
    }
}
