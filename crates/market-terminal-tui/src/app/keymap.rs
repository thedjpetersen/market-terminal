//! Semantic shell key bindings adapted from `makeev/alphai-tui` commit
//! `9143d2e1176d0a67a9f26960427cf370187fc2e6` (MIT, Copyright (c) 2026
//! Mikhail Makeev). The environment format and Market Terminal action set are
//! specific to this shell; see `THIRD_PARTY_NOTICES.md`.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

const MAX_SPEC_BYTES: usize = 4_096;
const MAX_BINDINGS_PER_ACTION: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ShellAction {
    Quit,
    OpenCommand,
    NextPanel,
    PreviousPanel,
    Settings,
    Help,
    NextTheme,
    PreviousTheme,
    Refresh,
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Open,
}

const ACTIONS: &[(ShellAction, &str)] = &[
    (ShellAction::Quit, "quit"),
    (ShellAction::OpenCommand, "command"),
    (ShellAction::NextPanel, "next_panel"),
    (ShellAction::PreviousPanel, "previous_panel"),
    (ShellAction::Settings, "settings"),
    (ShellAction::Help, "help"),
    (ShellAction::NextTheme, "next_theme"),
    (ShellAction::PreviousTheme, "previous_theme"),
    (ShellAction::Refresh, "refresh"),
    (ShellAction::Up, "up"),
    (ShellAction::Down, "down"),
    (ShellAction::Left, "left"),
    (ShellAction::Right, "right"),
    (ShellAction::PageUp, "page_up"),
    (ShellAction::PageDown, "page_down"),
    (ShellAction::Open, "open"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct KeyCombo {
    code: KeyCode,
    modifiers: KeyModifiers,
}

const fn plain(code: KeyCode) -> KeyCombo {
    KeyCombo {
        code,
        modifiers: KeyModifiers::NONE,
    }
}

const fn character(character: char) -> KeyCombo {
    plain(KeyCode::Char(character))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BindingMatch {
    Action {
        action: ShellAction,
        customized: bool,
    },
    Disabled,
    Unmapped,
}

#[derive(Debug, Clone)]
pub(crate) struct Keymap {
    bindings: Vec<(ShellAction, Vec<KeyCombo>)>,
    disabled_defaults: Vec<KeyCombo>,
    custom_actions: Vec<ShellAction>,
}

impl Default for Keymap {
    fn default() -> Self {
        Self {
            bindings: ACTIONS
                .iter()
                .map(|(action, _)| (*action, default_keys(*action)))
                .collect(),
            disabled_defaults: Vec::new(),
            custom_actions: Vec::new(),
        }
    }
}

fn default_keys(action: ShellAction) -> Vec<KeyCombo> {
    match action {
        ShellAction::Quit => vec![character('q')],
        ShellAction::OpenCommand => vec![character('/'), character(':')],
        ShellAction::NextPanel => vec![KeyCombo {
            code: KeyCode::Char('n'),
            modifiers: KeyModifiers::CONTROL,
        }],
        ShellAction::PreviousPanel => vec![KeyCombo {
            code: KeyCode::Char('p'),
            modifiers: KeyModifiers::CONTROL,
        }],
        ShellAction::Settings => vec![plain(KeyCode::F(2))],
        ShellAction::Help => vec![plain(KeyCode::F(1))],
        ShellAction::NextTheme => vec![plain(KeyCode::F(3))],
        ShellAction::PreviousTheme => vec![KeyCombo {
            code: KeyCode::F(3),
            modifiers: KeyModifiers::SHIFT,
        }],
        ShellAction::Refresh => vec![plain(KeyCode::F(9))],
        ShellAction::Up => vec![plain(KeyCode::Up)],
        ShellAction::Down => vec![plain(KeyCode::Down)],
        ShellAction::Left => vec![plain(KeyCode::Left)],
        ShellAction::Right => vec![plain(KeyCode::Right)],
        ShellAction::PageUp => vec![plain(KeyCode::PageUp)],
        ShellAction::PageDown => vec![plain(KeyCode::PageDown)],
        ShellAction::Open => vec![plain(KeyCode::Enter)],
    }
}

impl Keymap {
    pub(crate) fn from_env() -> (Self, Vec<String>) {
        let Ok(specification) = std::env::var("MARKET_TERMINAL_KEYBINDINGS") else {
            return (Self::default(), Vec::new());
        };
        Self::from_spec(&specification)
    }

    /// Parses `action=key,key;action=key` bindings. Listed actions replace
    /// their defaults; unlisted actions keep theirs. Invalid entries warn and
    /// fall back instead of preventing startup.
    pub(crate) fn from_spec(specification: &str) -> (Self, Vec<String>) {
        let mut warnings = Vec::new();
        if specification.len() > MAX_SPEC_BYTES {
            warnings.push("keybindings: specification is too long; using defaults".to_owned());
            return (Self::default(), warnings);
        }

        let mut configured = Vec::<(ShellAction, Vec<KeyCombo>)>::new();
        for entry in specification
            .split(';')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
        {
            let Some((name, values)) = entry.split_once('=') else {
                warnings.push(format!("keybindings: missing '=' in \"{entry}\""));
                continue;
            };
            let name = name.trim().to_ascii_lowercase();
            let Some(action) = action_named(&name) else {
                warnings.push(format!("keybindings: unknown action \"{name}\""));
                continue;
            };
            if configured.iter().any(|(candidate, _)| *candidate == action) {
                warnings.push(format!("keybindings: duplicate action \"{name}\" ignored"));
                continue;
            }
            let mut keys = Vec::new();
            for value in values
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .take(MAX_BINDINGS_PER_ACTION)
            {
                match parse_key(value) {
                    Ok(key) if is_reserved(key) => {
                        warnings.push(format!("keybindings: {name} key \"{value}\" is reserved"))
                    }
                    Ok(key) if !keys.contains(&key) => keys.push(key),
                    Ok(_) => {}
                    Err(error) => warnings.push(format!("keybindings: {name} {error}")),
                }
            }
            if keys.is_empty() {
                warnings.push(format!(
                    "keybindings: {name} has no usable keys; keeping defaults"
                ));
            } else {
                configured.push((action, keys));
            }
        }

        let mut map = Self {
            custom_actions: configured.iter().map(|(action, _)| *action).collect(),
            ..Self::default()
        };
        for (action, keys) in &configured {
            if let Some((_, current)) = map
                .bindings
                .iter_mut()
                .find(|(candidate, _)| candidate == action)
            {
                *current = keys.clone();
            }
        }
        map.resolve_conflicts(&mut warnings);
        for (action, _) in configured {
            let active = map
                .bindings
                .iter()
                .find(|(candidate, _)| *candidate == action)
                .map(|(_, keys)| keys.as_slice())
                .unwrap_or_default();
            for default in default_keys(action) {
                if !active.contains(&default) && !map.disabled_defaults.contains(&default) {
                    map.disabled_defaults.push(default);
                }
            }
        }
        (map, warnings)
    }

    fn resolve_conflicts(&mut self, warnings: &mut Vec<String>) {
        let mut claimed = Vec::<(KeyCombo, &'static str)>::new();
        for (action, keys) in &mut self.bindings {
            let name = action_name(*action);
            keys.retain(|key| {
                let Some((_, owner)) = claimed.iter().find(|(candidate, _)| candidate == key)
                else {
                    return true;
                };
                warnings.push(format!(
                    "keybindings: {name} key \"{}\" is already used by {owner}",
                    combo_label(*key)
                ));
                false
            });
            if keys.is_empty() {
                let fallback = default_keys(*action)
                    .into_iter()
                    .filter(|key| claimed.iter().all(|(candidate, _)| candidate != key))
                    .collect::<Vec<_>>();
                if fallback.is_empty() {
                    warnings.push(format!("keybindings: {name} has no available fallback"));
                } else {
                    warnings.push(format!("keybindings: {name} fell back to defaults"));
                    *keys = fallback;
                }
            }
            claimed.extend(keys.iter().map(|key| (*key, name)));
        }
    }

    pub(crate) fn resolve(&self, event: KeyEvent) -> BindingMatch {
        let combo = normalize(event);
        if let Some(action) = self
            .bindings
            .iter()
            .find_map(|(action, keys)| keys.contains(&combo).then_some(*action))
        {
            BindingMatch::Action {
                action,
                customized: self.custom_actions.contains(&action),
            }
        } else if self.disabled_defaults.contains(&combo) {
            BindingMatch::Disabled
        } else {
            BindingMatch::Unmapped
        }
    }

    pub(crate) fn labels(&self, actions: &[ShellAction]) -> String {
        actions
            .iter()
            .filter_map(|action| {
                self.bindings
                    .iter()
                    .find(|(candidate, _)| candidate == action)
                    .and_then(|(_, keys)| keys.first())
                    .copied()
                    .map(combo_label)
            })
            .collect::<Vec<_>>()
            .join("/")
    }

    pub(crate) fn status(&self, warning_count: usize) -> String {
        if self.custom_actions.is_empty() {
            "DEFAULT · VIM + TMUX FIXED".to_owned()
        } else if warning_count == 0 {
            format!("{} CUSTOM ACTION(S)", self.custom_actions.len())
        } else {
            format!(
                "{} CUSTOM ACTION(S) · {} WARNING(S)",
                self.custom_actions.len(),
                warning_count
            )
        }
    }
}

fn action_named(name: &str) -> Option<ShellAction> {
    ACTIONS
        .iter()
        .find_map(|(action, candidate)| (*candidate == name).then_some(*action))
}

fn action_name(action: ShellAction) -> &'static str {
    ACTIONS
        .iter()
        .find_map(|(candidate, name)| (*candidate == action).then_some(*name))
        .expect("all shell actions have a configuration name")
}

fn parse_key(specification: &str) -> Result<KeyCombo, String> {
    let invalid =
        || format!("has invalid key \"{specification}\" (expected [ctrl-][alt-][shift-]key)");
    let mut rest = specification.trim();
    let mut modifiers = KeyModifiers::NONE;
    loop {
        let mut stripped = false;
        for (prefix, modifier) in [
            ("ctrl-", KeyModifiers::CONTROL),
            ("alt-", KeyModifiers::ALT),
            ("shift-", KeyModifiers::SHIFT),
        ] {
            if rest.len() > prefix.len()
                && rest
                    .get(..prefix.len())
                    .is_some_and(|candidate| candidate.eq_ignore_ascii_case(prefix))
            {
                modifiers |= modifier;
                rest = &rest[prefix.len()..];
                stripped = true;
            }
        }
        if !stripped {
            break;
        }
    }
    let mut characters = rest.chars();
    let code = match (characters.next(), characters.next()) {
        (Some(character), None) => KeyCode::Char(character),
        (Some(_), Some(_)) => named_key(rest).ok_or_else(invalid)?,
        (None, _) => return Err(invalid()),
    };
    Ok(normalize(KeyEvent::new(code, modifiers)))
}

fn named_key(name: &str) -> Option<KeyCode> {
    let name = name.to_ascii_lowercase();
    if let Some(number) = name
        .strip_prefix('f')
        .and_then(|number| number.parse::<u8>().ok())
        .filter(|number| (1..=12).contains(number))
    {
        return Some(KeyCode::F(number));
    }
    match name.as_str() {
        "esc" => Some(KeyCode::Esc),
        "enter" | "return" => Some(KeyCode::Enter),
        "tab" => Some(KeyCode::Tab),
        "backtab" => Some(KeyCode::BackTab),
        "space" => Some(KeyCode::Char(' ')),
        "up" => Some(KeyCode::Up),
        "down" => Some(KeyCode::Down),
        "left" => Some(KeyCode::Left),
        "right" => Some(KeyCode::Right),
        "home" => Some(KeyCode::Home),
        "end" => Some(KeyCode::End),
        "pgup" | "pageup" => Some(KeyCode::PageUp),
        "pgdn" | "pagedown" => Some(KeyCode::PageDown),
        "backspace" => Some(KeyCode::Backspace),
        "delete" | "del" => Some(KeyCode::Delete),
        "insert" => Some(KeyCode::Insert),
        _ => None,
    }
}

fn is_reserved(combo: KeyCombo) -> bool {
    combo.code == KeyCode::Esc
        || (combo.code == KeyCode::Char('c') && combo.modifiers.contains(KeyModifiers::CONTROL))
        || (combo.code == KeyCode::Char('b') && combo.modifiers.contains(KeyModifiers::CONTROL))
}

fn normalize(event: KeyEvent) -> KeyCombo {
    match event.code {
        KeyCode::Tab if event.modifiers.contains(KeyModifiers::SHIFT) => KeyCombo {
            code: KeyCode::BackTab,
            modifiers: event.modifiers - KeyModifiers::SHIFT,
        },
        KeyCode::Char(character) if event.modifiers.contains(KeyModifiers::SHIFT) => KeyCombo {
            code: KeyCode::Char(character.to_ascii_uppercase()),
            modifiers: event.modifiers - KeyModifiers::SHIFT,
        },
        code => KeyCombo {
            code,
            modifiers: event.modifiers,
        },
    }
}

fn combo_label(combo: KeyCombo) -> String {
    let mut label = String::new();
    if combo.modifiers.contains(KeyModifiers::CONTROL) {
        label.push_str("ctrl-");
    }
    if combo.modifiers.contains(KeyModifiers::ALT) {
        label.push_str("alt-");
    }
    if combo.modifiers.contains(KeyModifiers::SHIFT) {
        label.push_str("shift-");
    }
    match combo.code {
        KeyCode::Char(character) => label.push(character),
        KeyCode::Enter => label.push('⏎'),
        KeyCode::Up => label.push('↑'),
        KeyCode::Down => label.push('↓'),
        KeyCode::Left => label.push('←'),
        KeyCode::Right => label.push('→'),
        KeyCode::Tab => label.push_str("tab"),
        KeyCode::BackTab => label.push_str("shift-tab"),
        KeyCode::PageUp => label.push_str("pgup"),
        KeyCode::PageDown => label.push_str("pgdn"),
        KeyCode::F(number) => label.push_str(&format!("f{number}")),
        other => label.push_str(&format!("{other:?}").to_ascii_lowercase()),
    }
    label
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_bindings_replace_defaults_and_preserve_unlisted_actions() {
        let (map, warnings) = Keymap::from_spec("help=ctrl-h;up=w,k;open=space");
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(
            map.resolve(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::CONTROL)),
            BindingMatch::Action {
                action: ShellAction::Help,
                customized: true
            }
        );
        assert_eq!(
            map.resolve(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE)),
            BindingMatch::Disabled
        );
        assert_eq!(
            map.resolve(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
            BindingMatch::Action {
                action: ShellAction::Quit,
                customized: false
            }
        );
    }

    #[test]
    fn invalid_reserved_and_conflicting_entries_fall_back_safely() {
        let (map, warnings) = Keymap::from_spec(
            "quit=ctrl-c;help=alt-z;settings=alt-z;wat=nope;open=definitely-not-a-key",
        );
        assert!(warnings.len() >= 4, "{warnings:?}");
        assert_eq!(
            map.resolve(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
            BindingMatch::Action {
                action: ShellAction::Quit,
                customized: false
            }
        );
        assert_eq!(
            map.resolve(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::ALT)),
            BindingMatch::Action {
                action: ShellAction::Settings,
                customized: true
            }
        );
    }

    #[test]
    fn shifted_function_and_character_keys_normalize_like_terminal_events() {
        let (map, warnings) = Keymap::from_spec("previous_theme=shift-f4;help=shift-h");
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(
            map.resolve(KeyEvent::new(KeyCode::F(4), KeyModifiers::SHIFT)),
            BindingMatch::Action {
                action: ShellAction::PreviousTheme,
                customized: true
            }
        );
        assert_eq!(
            map.resolve(KeyEvent::new(KeyCode::Char('H'), KeyModifiers::SHIFT)),
            BindingMatch::Action {
                action: ShellAction::Help,
                customized: true
            }
        );
    }
}
