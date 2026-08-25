use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    #[default]
    Navigation,
    Command,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NavigationAction {
    Quit,
    OpenCommand,
    Hotkey(char),
    Delegate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CommandAction {
    Cancel,
    Delete,
    Execute,
    Append(char),
    None,
}

pub(super) fn is_force_quit(key: KeyEvent) -> bool {
    key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c')
}

pub(super) fn navigation_action(key: KeyEvent) -> NavigationAction {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => NavigationAction::Quit,
        KeyCode::Char('/') | KeyCode::Char(':') => NavigationAction::OpenCommand,
        KeyCode::Char(character) if character.is_ascii_alphabetic() => {
            NavigationAction::Hotkey(character.to_ascii_lowercase())
        }
        _ => NavigationAction::Delegate,
    }
}

pub(super) fn command_action(key: KeyEvent) -> CommandAction {
    match key.code {
        KeyCode::Esc => CommandAction::Cancel,
        KeyCode::Backspace => CommandAction::Delete,
        KeyCode::Enter => CommandAction::Execute,
        KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            CommandAction::Append(character)
        }
        _ => CommandAction::None,
    }
}
