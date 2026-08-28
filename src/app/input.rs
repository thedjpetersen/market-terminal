use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    #[default]
    Navigation,
    Command,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum CommandEditMode {
    #[default]
    Insert,
    Normal,
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
    Execute,
    EnterNormal,
    EnterInsert,
    AppendAfter,
    InsertAtStart,
    InsertAtEnd,
    Insert(char),
    Backspace,
    DeleteAt,
    DeleteToEnd,
    DeletePreviousWord,
    DeleteOperator,
    Clear,
    MoveLeft,
    MoveRight,
    MoveStart,
    MoveEnd,
    MoveWordForward,
    MoveWordBackward,
    HistoryPrevious,
    HistoryNext,
    None,
}

pub(super) fn is_force_quit(key: KeyEvent) -> bool {
    key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c')
}

pub(super) fn navigation_action(key: KeyEvent) -> NavigationAction {
    match key.code {
        KeyCode::Char('q') => NavigationAction::Quit,
        KeyCode::Char('/') | KeyCode::Char(':') => NavigationAction::OpenCommand,
        KeyCode::Char(character)
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            NavigationAction::Hotkey(character.to_ascii_lowercase())
        }
        _ => NavigationAction::Delegate,
    }
}

pub(super) fn command_action(key: KeyEvent, mode: CommandEditMode) -> CommandAction {
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return match key.code {
            KeyCode::Char('a' | 'b') => CommandAction::MoveStart,
            KeyCode::Char('e') => CommandAction::MoveEnd,
            KeyCode::Char('w') => CommandAction::DeletePreviousWord,
            KeyCode::Char('u') => CommandAction::Clear,
            KeyCode::Char('p') => CommandAction::HistoryPrevious,
            KeyCode::Char('n') => CommandAction::HistoryNext,
            _ => CommandAction::None,
        };
    }
    match mode {
        CommandEditMode::Insert => match key.code {
            KeyCode::Esc => CommandAction::EnterNormal,
            KeyCode::Enter => CommandAction::Execute,
            KeyCode::Backspace => CommandAction::Backspace,
            KeyCode::Delete => CommandAction::DeleteAt,
            KeyCode::Left => CommandAction::MoveLeft,
            KeyCode::Right => CommandAction::MoveRight,
            KeyCode::Home => CommandAction::MoveStart,
            KeyCode::End => CommandAction::MoveEnd,
            KeyCode::Up => CommandAction::HistoryPrevious,
            KeyCode::Down => CommandAction::HistoryNext,
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::ALT) => {
                CommandAction::Insert(character)
            }
            _ => CommandAction::None,
        },
        CommandEditMode::Normal => match key.code {
            KeyCode::Esc => CommandAction::Cancel,
            KeyCode::Enter => CommandAction::Execute,
            KeyCode::Left | KeyCode::Backspace | KeyCode::Char('h') => CommandAction::MoveLeft,
            KeyCode::Right | KeyCode::Char('l') => CommandAction::MoveRight,
            KeyCode::Home | KeyCode::Char('0') => CommandAction::MoveStart,
            KeyCode::End | KeyCode::Char('$') => CommandAction::MoveEnd,
            KeyCode::Char('w') => CommandAction::MoveWordForward,
            KeyCode::Char('b') => CommandAction::MoveWordBackward,
            KeyCode::Delete | KeyCode::Char('x') => CommandAction::DeleteAt,
            KeyCode::Char('D') => CommandAction::DeleteToEnd,
            KeyCode::Char('d') => CommandAction::DeleteOperator,
            KeyCode::Char('i') => CommandAction::EnterInsert,
            KeyCode::Char('a') => CommandAction::AppendAfter,
            KeyCode::Char('I') => CommandAction::InsertAtStart,
            KeyCode::Char('A') => CommandAction::InsertAtEnd,
            KeyCode::Up | KeyCode::Char('k') => CommandAction::HistoryPrevious,
            KeyCode::Down | KeyCode::Char('j') => CommandAction::HistoryNext,
            _ => CommandAction::None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_routes_registered_non_alphabetic_hotkeys() {
        let key = KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE);
        assert_eq!(navigation_action(key), NavigationAction::Hotkey('1'));
    }

    #[test]
    fn navigation_does_not_route_modified_characters_as_hotkeys() {
        let key = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::ALT);
        assert_eq!(navigation_action(key), NavigationAction::Delegate);
    }

    #[test]
    fn command_insert_and_normal_modes_have_distinct_bindings() {
        let plain_x = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE);

        assert_eq!(
            command_action(plain_x, CommandEditMode::Insert),
            CommandAction::Insert('x')
        );
        assert_eq!(
            command_action(plain_x, CommandEditMode::Normal),
            CommandAction::DeleteAt
        );
        assert_eq!(
            command_action(
                KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL),
                CommandEditMode::Insert,
            ),
            CommandAction::DeletePreviousWord
        );
    }
}
