use std::collections::{HashMap, HashSet};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::{layout::Rect, Frame};

pub(super) const MAX_COMMAND_BYTES: usize = 4_096;
const MAX_COMMAND_TOKENS: usize = 64;
const MAX_COMMAND_TOKEN_BYTES: usize = 512;
pub(super) const MAX_WORKSPACE_ACTIONS: usize = 26 * 26;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WorkspaceId(&'static str);

impl WorkspaceId {
    pub const fn new(value: &'static str) -> Self {
        Self(value)
    }
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkspaceDescriptor {
    pub id: WorkspaceId,
    pub label: &'static str,
    /// Use `\0` for workspaces that do not have a direct navigation hotkey.
    ///
    /// This remains a `char` so existing feature descriptors stay source compatible.
    /// New code should read hotkeys through [`Workspace::hotkey`].
    pub hotkey: char,
    pub commands: &'static [&'static str],
}

/// A parsed terminal command such as `AAPL US EQUITY`.
///
/// The first token is the function used for exact alias resolution. Remaining
/// tokens are arguments for the target workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandInvocation {
    pub function: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandArgument {
    Positional(String),
    Option { name: String, value: Option<String> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandParseError {
    Empty,
    TooLong,
    TooManyTokens,
    TokenTooLong,
    UnterminatedQuote,
    TrailingEscape,
    InvalidFunction(String),
    InvalidOption(String),
}

impl CommandInvocation {
    pub fn parse(command: &str) -> Option<Self> {
        Self::try_parse(command).ok()
    }

    pub fn try_parse(command: &str) -> Result<Self, CommandParseError> {
        if command.len() > MAX_COMMAND_BYTES {
            return Err(CommandParseError::TooLong);
        }
        let mut tokens = tokenize_command(command)?;
        if tokens.is_empty() {
            return Err(CommandParseError::Empty);
        }
        let function = tokens.remove(0).to_ascii_uppercase();
        if function.is_empty()
            || !function.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '_' | '-')
            })
        {
            return Err(CommandParseError::InvalidFunction(function));
        }
        Ok(Self {
            function,
            args: tokens,
        })
    }

    /// Returns a typed view over positional values and GNU-style long options.
    ///
    /// Feature command handlers can adopt this without changing the stable raw
    /// argument representation used by existing workspaces.
    pub fn typed_arguments(&self) -> Result<Vec<CommandArgument>, CommandParseError> {
        self.args
            .iter()
            .map(|argument| {
                let Some(option) = argument.strip_prefix("--") else {
                    return Ok(CommandArgument::Positional(argument.clone()));
                };
                let (name, value) = option
                    .split_once('=')
                    .map_or((option, None), |(name, value)| {
                        (name, Some(value.to_owned()))
                    });
                if name.is_empty()
                    || !name.chars().all(|character| {
                        character.is_ascii_alphanumeric() || matches!(character, '_' | '-')
                    })
                {
                    return Err(CommandParseError::InvalidOption(argument.clone()));
                }
                Ok(CommandArgument::Option {
                    name: name.to_ascii_lowercase(),
                    value,
                })
            })
            .collect()
    }
}

fn tokenize_command(command: &str) -> Result<Vec<String>, CommandParseError> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;
    let mut token_started = false;

    for character in command.chars() {
        if escaped {
            current.push(character);
            escaped = false;
            token_started = true;
            continue;
        }
        if character == '\\' {
            escaped = true;
            token_started = true;
            continue;
        }
        if let Some(delimiter) = quote {
            if character == delimiter {
                quote = None;
            } else {
                current.push(character);
            }
            token_started = true;
            continue;
        }
        if matches!(character, '\'' | '"') {
            quote = Some(character);
            token_started = true;
        } else if character.is_whitespace() {
            if token_started {
                push_command_token(&mut tokens, &mut current)?;
                token_started = false;
            }
        } else {
            current.push(character);
            token_started = true;
        }
    }

    if escaped {
        return Err(CommandParseError::TrailingEscape);
    }
    if quote.is_some() {
        return Err(CommandParseError::UnterminatedQuote);
    }
    if token_started {
        push_command_token(&mut tokens, &mut current)?;
    }
    Ok(tokens)
}

fn push_command_token(
    tokens: &mut Vec<String>,
    current: &mut String,
) -> Result<(), CommandParseError> {
    if current.len() > MAX_COMMAND_TOKEN_BYTES {
        return Err(CommandParseError::TokenTooLong);
    }
    if tokens.len() == MAX_COMMAND_TOKENS {
        return Err(CommandParseError::TooManyTokens);
    }
    tokens.push(std::mem::take(current));
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkspaceNavigationItem {
    pub id: WorkspaceId,
    pub label: &'static str,
    pub hotkey: Option<char>,
}

/// A visible, stable feature-owned destination for spatial focus and follow hints.
///
/// The shell treats the identifier as opaque. Features calculate rectangles from
/// the same area they render into and retain ownership of activation semantics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceAction {
    pub id: String,
    pub label: String,
    pub area: Rect,
    pub enabled: bool,
    /// The feature's best focus-restoration target for the current frame.
    ///
    /// The registry permits more than one preferred action but the shell always
    /// chooses the first valid one, keeping feature order as the deterministic
    /// tie-breaker.
    pub preferred: bool,
}

impl WorkspaceAction {
    pub fn new(id: impl Into<String>, label: impl Into<String>, area: Rect) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            area,
            enabled: true,
            preferred: false,
        }
    }

    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    pub fn preferred(mut self) -> Self {
        self.preferred = true;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellContext {
    pub active_workspace: WorkspaceId,
    pub workspace_order: Vec<WorkspaceId>,
}

/// Controls how much application chrome surrounds a workspace. Analytical
/// dashboards can opt into the full terminal while editors and navigational
/// workspaces retain the standard header, workspace rail, and footer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellChrome {
    Standard,
    Immersive,
}

/// The only application-level mutations a feature may request.
///
/// Keeping these intents small and validated at the registry boundary lets
/// features (including AI-backed features) coordinate the shell without
/// importing or mutating `App` directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppIntent {
    ActivateWorkspace {
        target: String,
    },
    BringWorkspaceForward {
        target: String,
    },
    DispatchCommand {
        command: String,
        origin: WorkspaceId,
    },
    RestoreWorkspaceOrder,
}

pub trait Workspace: Send {
    fn descriptor(&self) -> WorkspaceDescriptor;
    fn render(&self, frame: &mut Frame, area: Rect);

    /// Gives the active feature first refusal on navigation-mode input.
    /// Returning `true` prevents application-level routing of the key.
    fn handle_key(&mut self, _key: KeyEvent) -> bool {
        false
    }

    /// Gives a feature mouse input relative to the same area used to render it.
    /// Scroll wheels fall back to the feature's existing up/down navigation.
    fn handle_mouse(&mut self, event: MouseEvent, area: Rect) -> bool {
        if !crate::ui::contains(area, event.column, event.row) {
            return false;
        }
        let key = match event.kind {
            MouseEventKind::ScrollUp => KeyCode::Up,
            MouseEventKind::ScrollDown => KeyCode::Down,
            _ => return false,
        };
        self.handle_key(KeyEvent::new(key, KeyModifiers::NONE))
    }

    /// Releases feature-local input focus when the shell activates another target.
    fn on_blur(&mut self) {}

    /// Gives a feature focus after the shell opens it directly or in an overlay.
    fn on_focus(&mut self) {}

    /// Receives a command after its function alias resolves to this workspace.
    fn handle_command(&mut self, _invocation: &CommandInvocation) -> bool {
        false
    }

    /// Describes visible feature-local actions in the current render area.
    ///
    /// IDs must remain stable while follow mode is open. The registry fails
    /// closed on duplicate, empty, oversized, disabled, or out-of-bounds actions.
    fn actions(&self, _area: Rect) -> Vec<WorkspaceAction> {
        Vec::new()
    }

    /// Activates one action previously returned from [`Workspace::actions`].
    fn activate_action(&mut self, _id: &str) -> bool {
        false
    }

    /// Reports a feature-owned modal that must trap shell navigation.
    ///
    /// While active, the application routes keys and pointer events only to the
    /// owning workspace. Follow hints remain available, but contain only actions
    /// returned by that modal.
    fn is_modal_active(&self) -> bool {
        false
    }

    fn shell_chrome(&self) -> ShellChrome {
        ShellChrome::Standard
    }

    /// Direct feature hotkeys are optional. Existing descriptors can opt out with `\0`.
    fn hotkey(&self) -> Option<char> {
        let hotkey = self.descriptor().hotkey;
        (hotkey != '\0').then_some(hotkey)
    }

    /// Favorites appear in the top navigation even when they have no hotkey.
    fn is_favorite(&self) -> bool {
        false
    }

    /// Polls asynchronous feature work and returns validated shell requests.
    fn poll_intents(&mut self) -> Vec<AppIntent> {
        Vec::new()
    }

    /// Supplies a read-only snapshot of shell focus and layout. Features may
    /// use this as model/view context but cannot mutate it directly.
    fn update_shell_context(&mut self, _context: &ShellContext) {}
}

pub struct WorkspaceRegistry {
    entries: Vec<Box<dyn Workspace>>,
    default_order: Vec<WorkspaceId>,
    aliases: HashMap<String, WorkspaceId>,
    hotkeys: HashMap<char, WorkspaceId>,
}

impl WorkspaceRegistry {
    pub fn new(entries: Vec<Box<dyn Workspace>>) -> Self {
        let (aliases, hotkeys) = Self::build_indexes(&entries);
        let default_order = entries.iter().map(|entry| entry.descriptor().id).collect();
        Self {
            entries,
            default_order,
            aliases,
            hotkeys,
        }
    }

    pub fn descriptors(&self) -> impl Iterator<Item = WorkspaceDescriptor> + '_ {
        self.entries.iter().map(|workspace| workspace.descriptor())
    }

    pub fn navigation_items(&self) -> impl Iterator<Item = WorkspaceNavigationItem> + '_ {
        self.entries.iter().filter_map(|workspace| {
            let hotkey = workspace.hotkey();
            if !workspace.is_favorite() && hotkey.is_none() {
                return None;
            }
            let descriptor = workspace.descriptor();
            Some(WorkspaceNavigationItem {
                id: descriptor.id,
                label: descriptor.label,
                hotkey,
            })
        })
    }

    pub fn workspace_order(&self) -> Vec<WorkspaceId> {
        self.entries
            .iter()
            .map(|entry| entry.descriptor().id)
            .collect()
    }

    pub fn command_aliases(&self) -> Vec<String> {
        self.entries
            .iter()
            .flat_map(|workspace| workspace.descriptor().commands.iter().copied())
            .map(str::to_owned)
            .collect()
    }

    pub fn resolve_hotkey(&self, hotkey: char) -> Option<WorkspaceId> {
        self.hotkeys.get(&hotkey.to_ascii_lowercase()).copied()
    }

    /// Resolves only the command's first token against an exact alias.
    ///
    /// Kept for compatibility with callers that only need the destination.
    pub fn resolve_command(&self, command: &str) -> Option<WorkspaceId> {
        let invocation = CommandInvocation::parse(command)?;
        self.resolve_invocation(&invocation)
    }

    pub fn resolve_invocation(&self, invocation: &CommandInvocation) -> Option<WorkspaceId> {
        self.aliases
            .get(&invocation.function.to_ascii_uppercase())
            .copied()
    }

    /// Resolves and dispatches a command to its owning feature.
    pub fn dispatch_command(&mut self, command: &str) -> Option<WorkspaceId> {
        let invocation = CommandInvocation::parse(command)?;
        let id = self.resolve_invocation(&invocation)?;
        if let Some(workspace) = self
            .entries
            .iter_mut()
            .find(|entry| entry.descriptor().id == id)
        {
            workspace.handle_command(&invocation);
        }
        Some(id)
    }

    pub fn render(&self, id: WorkspaceId, frame: &mut Frame, area: Rect) {
        if let Some(workspace) = self
            .entries
            .iter()
            .find(|entry| entry.descriptor().id == id)
        {
            workspace.render(frame, area);
        }
    }

    pub fn shell_chrome(&self, id: WorkspaceId) -> ShellChrome {
        self.entries
            .iter()
            .find(|entry| entry.descriptor().id == id)
            .map_or(ShellChrome::Standard, |workspace| workspace.shell_chrome())
    }

    pub fn handle_key(&mut self, id: WorkspaceId, key: KeyEvent) -> bool {
        self.entries
            .iter_mut()
            .find(|entry| entry.descriptor().id == id)
            .is_some_and(|workspace| workspace.handle_key(key))
    }

    pub fn handle_mouse(&mut self, id: WorkspaceId, event: MouseEvent, area: Rect) -> bool {
        self.entries
            .iter_mut()
            .find(|entry| entry.descriptor().id == id)
            .is_some_and(|workspace| workspace.handle_mouse(event, area))
    }

    pub fn actions(&self, id: WorkspaceId, area: Rect, limit: usize) -> Vec<WorkspaceAction> {
        let Some(workspace) = self
            .entries
            .iter()
            .find(|entry| entry.descriptor().id == id)
        else {
            return Vec::new();
        };
        sanitize_actions(workspace.actions(area), area, limit)
    }

    pub fn activate_action(&mut self, id: WorkspaceId, action_id: &str, area: Rect) -> bool {
        if !self
            .actions(id, area, MAX_WORKSPACE_ACTIONS)
            .iter()
            .any(|action| action.id == action_id)
        {
            return false;
        }
        self.entries
            .iter_mut()
            .find(|entry| entry.descriptor().id == id)
            .is_some_and(|workspace| workspace.activate_action(action_id))
    }

    pub fn is_modal_active(&self, id: WorkspaceId) -> bool {
        self.entries
            .iter()
            .find(|entry| entry.descriptor().id == id)
            .is_some_and(|workspace| workspace.is_modal_active())
    }

    pub fn on_blur(&mut self, id: WorkspaceId) {
        if let Some(workspace) = self
            .entries
            .iter_mut()
            .find(|entry| entry.descriptor().id == id)
        {
            workspace.on_blur();
        }
    }

    pub fn on_focus(&mut self, id: WorkspaceId) {
        if let Some(workspace) = self
            .entries
            .iter_mut()
            .find(|entry| entry.descriptor().id == id)
        {
            workspace.on_focus();
        }
    }

    pub fn poll_intents(&mut self) -> Vec<AppIntent> {
        self.entries
            .iter_mut()
            .flat_map(|workspace| workspace.poll_intents())
            .collect()
    }

    pub fn update_shell_context(&mut self, active_workspace: WorkspaceId) {
        let context = ShellContext {
            active_workspace,
            workspace_order: self
                .entries
                .iter()
                .map(|entry| entry.descriptor().id)
                .collect(),
        };
        for workspace in &mut self.entries {
            workspace.update_shell_context(&context);
        }
    }

    /// Resolves a model/user-facing target against IDs, labels, and exact
    /// command aliases. No fuzzy or substring matching is allowed here.
    pub fn resolve_target(&self, target: &str) -> Option<WorkspaceId> {
        let normalized = target.trim().to_ascii_uppercase();
        self.entries
            .iter()
            .find_map(|workspace| {
                let descriptor = workspace.descriptor();
                (descriptor.id.as_str().eq_ignore_ascii_case(&normalized)
                    || descriptor.label.eq_ignore_ascii_case(&normalized))
                .then_some(descriptor.id)
            })
            .or_else(|| self.aliases.get(&normalized).copied())
    }

    pub fn bring_forward(&mut self, id: WorkspaceId) {
        if let Some(index) = self
            .entries
            .iter()
            .position(|entry| entry.descriptor().id == id)
        {
            let workspace = self.entries.remove(index);
            self.entries.insert(0, workspace);
        }
    }

    pub fn restore_order(&mut self) {
        self.entries.sort_by_key(|entry| {
            self.default_order
                .iter()
                .position(|id| *id == entry.descriptor().id)
                .unwrap_or(usize::MAX)
        });
    }

    /// Applies a durable workspace order while preserving newly registered
    /// workspaces and ignoring stale identifiers from older installations.
    pub fn apply_workspace_order(&mut self, requested: &[String]) {
        let mut order = Vec::new();
        for persisted_id in requested {
            if let Some(id) = self.entries.iter().find_map(|entry| {
                let id = entry.descriptor().id;
                id.as_str().eq_ignore_ascii_case(persisted_id).then_some(id)
            }) {
                if !order.contains(&id) {
                    order.push(id);
                }
            }
        }
        self.entries.sort_by_key(|entry| {
            order
                .iter()
                .position(|id| *id == entry.descriptor().id)
                .unwrap_or(usize::MAX)
        });
    }

    fn build_indexes(
        entries: &[Box<dyn Workspace>],
    ) -> (HashMap<String, WorkspaceId>, HashMap<char, WorkspaceId>) {
        let mut ids = HashSet::new();
        let mut hotkeys = HashMap::new();
        let mut aliases = HashMap::new();
        for workspace in entries {
            let descriptor = workspace.descriptor();
            assert!(
                ids.insert(descriptor.id),
                "duplicate workspace id: {}",
                descriptor.id.as_str()
            );

            if let Some(hotkey) = workspace.hotkey() {
                let normalized = hotkey.to_ascii_lowercase();
                assert!(
                    hotkeys.insert(normalized, descriptor.id).is_none(),
                    "duplicate workspace hotkey: {hotkey}"
                );
            }

            for alias in descriptor.commands {
                let normalized = alias.trim().to_ascii_uppercase();
                assert!(
                    !normalized.is_empty(),
                    "empty command alias for {}",
                    descriptor.id.as_str()
                );
                assert!(
                    !normalized.chars().any(char::is_whitespace),
                    "command alias must be one token: {alias}"
                );
                assert!(
                    aliases.insert(normalized, descriptor.id).is_none(),
                    "duplicate command alias: {alias}"
                );
            }
        }
        (aliases, hotkeys)
    }
}

pub(super) fn sanitize_actions(
    actions: impl IntoIterator<Item = WorkspaceAction>,
    area: Rect,
    limit: usize,
) -> Vec<WorkspaceAction> {
    let mut seen = HashSet::new();
    actions
        .into_iter()
        .filter(|action| {
            action.enabled
                && !action.id.is_empty()
                && action.id.len() <= 128
                && !action.label.is_empty()
                && action.label.len() <= 256
                && action.area.width > 0
                && action.area.height > 0
                && action.area.x >= area.x
                && action.area.y >= area.y
                && action.area.right() <= area.right()
                && action.area.bottom() <= area.bottom()
                && seen.insert(action.id.clone())
        })
        .take(limit.min(MAX_WORKSPACE_ACTIONS))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    struct Stub {
        descriptor: WorkspaceDescriptor,
        hotkey: Option<char>,
        favorite: bool,
        invocation: Option<Arc<Mutex<Option<CommandInvocation>>>>,
        actions: Vec<WorkspaceAction>,
        activated: Option<Arc<Mutex<Vec<String>>>>,
    }

    impl Stub {
        fn new(descriptor: WorkspaceDescriptor) -> Self {
            Self {
                descriptor,
                hotkey: (descriptor.hotkey != '\0').then_some(descriptor.hotkey),
                favorite: true,
                invocation: None,
                actions: Vec::new(),
                activated: None,
            }
        }
    }

    impl Workspace for Stub {
        fn descriptor(&self) -> WorkspaceDescriptor {
            self.descriptor
        }
        fn render(&self, _frame: &mut Frame, _area: Rect) {}
        fn hotkey(&self) -> Option<char> {
            self.hotkey
        }
        fn is_favorite(&self) -> bool {
            self.favorite
        }
        fn handle_command(&mut self, invocation: &CommandInvocation) -> bool {
            if let Some(captured) = &self.invocation {
                *captured.lock().expect("capture lock") = Some(invocation.clone());
            }
            true
        }
        fn actions(&self, _area: Rect) -> Vec<WorkspaceAction> {
            self.actions.clone()
        }
        fn activate_action(&mut self, id: &str) -> bool {
            let Some(activated) = &self.activated else {
                return false;
            };
            activated
                .lock()
                .expect("activation capture lock")
                .push(id.to_owned());
            true
        }
    }

    fn descriptor(
        id: WorkspaceId,
        hotkey: char,
        commands: &'static [&'static str],
    ) -> WorkspaceDescriptor {
        WorkspaceDescriptor {
            id,
            label: "TEST",
            hotkey,
            commands,
        }
    }

    #[test]
    fn registry_resolves_exact_commands_and_hotkeys() {
        let id = WorkspaceId::new("test");
        let registry = WorkspaceRegistry::new(vec![Box::new(Stub::new(descriptor(
            id,
            't',
            &["TEST", "TST"],
        )))]);
        assert_eq!(registry.resolve_hotkey('T'), Some(id));
        assert_eq!(registry.resolve_command("tst go"), Some(id));
        assert_eq!(registry.resolve_command("testing"), None);
    }

    #[test]
    fn dispatch_preserves_function_and_arguments() {
        let id = WorkspaceId::new("security");
        let captured = Arc::new(Mutex::new(None));
        let workspace = Stub {
            invocation: Some(Arc::clone(&captured)),
            ..Stub::new(descriptor(id, 's', &["AAPL"]))
        };
        let mut registry = WorkspaceRegistry::new(vec![Box::new(workspace)]);

        assert_eq!(registry.dispatch_command("aapl US Equity"), Some(id));
        assert_eq!(
            *captured.lock().expect("capture lock"),
            Some(CommandInvocation {
                function: "AAPL".to_owned(),
                args: vec!["US".to_owned(), "Equity".to_owned()],
            })
        );
    }

    #[test]
    fn command_parser_preserves_quoted_subjects_and_typed_options() {
        let invocation =
            CommandInvocation::try_parse(r#"chart "BRK B US" --period=1Y --normalize"#)
                .expect("valid command");

        assert_eq!(invocation.function, "CHART");
        assert_eq!(invocation.args, ["BRK B US", "--period=1Y", "--normalize"]);
        assert_eq!(
            invocation.typed_arguments().expect("valid options"),
            vec![
                CommandArgument::Positional("BRK B US".to_owned()),
                CommandArgument::Option {
                    name: "period".to_owned(),
                    value: Some("1Y".to_owned()),
                },
                CommandArgument::Option {
                    name: "normalize".to_owned(),
                    value: None
                },
            ]
        );
    }

    #[test]
    fn command_parser_rejects_malformed_or_unbounded_input() {
        assert_eq!(
            CommandInvocation::try_parse("CHART 'unterminated"),
            Err(CommandParseError::UnterminatedQuote)
        );
        assert_eq!(
            CommandInvocation::try_parse("CHART trailing\\"),
            Err(CommandParseError::TrailingEscape)
        );
        assert!(matches!(
            CommandInvocation::try_parse(&"A".repeat(MAX_COMMAND_BYTES + 1)),
            Err(CommandParseError::TooLong)
        ));
    }

    #[test]
    fn optional_hotkeys_and_favorites_drive_navigation() {
        let favorite = WorkspaceId::new("favorite");
        let hidden = WorkspaceId::new("hidden");
        let registry = WorkspaceRegistry::new(vec![
            Box::new(Stub {
                hotkey: None,
                ..Stub::new(descriptor(favorite, '\0', &["FAV"]))
            }),
            Box::new(Stub {
                hotkey: None,
                favorite: false,
                ..Stub::new(descriptor(hidden, '\0', &["HIDDEN"]))
            }),
        ]);

        let items = registry.navigation_items().collect::<Vec<_>>();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, favorite);
        assert_eq!(items[0].hotkey, None);
    }

    #[test]
    fn action_registry_bounds_validates_deduplicates_and_activates() {
        let id = WorkspaceId::new("actions");
        let activated = Arc::new(Mutex::new(Vec::new()));
        let area = Rect::new(10, 5, 40, 20);
        let workspace = Stub {
            actions: vec![
                WorkspaceAction::new("row:0", "Open first row", Rect::new(11, 8, 20, 1)),
                WorkspaceAction::new("row:0", "Duplicate", Rect::new(11, 9, 20, 1)),
                WorkspaceAction::new("disabled", "Disabled", Rect::new(11, 10, 20, 1)).disabled(),
                WorkspaceAction::new("outside", "Outside", Rect::new(0, 0, 2, 1)),
                WorkspaceAction::new("row:1", "Open second row", Rect::new(11, 11, 20, 1)),
            ],
            activated: Some(Arc::clone(&activated)),
            ..Stub::new(descriptor(id, 'a', &["ACTIONS"]))
        };
        let mut registry = WorkspaceRegistry::new(vec![Box::new(workspace)]);

        let actions = registry.actions(id, area, 1);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].id, "row:0");
        assert!(registry.activate_action(id, "row:0", area));
        assert!(!registry.activate_action(id, "disabled", area));
        assert_eq!(
            *activated.lock().expect("activation capture lock"),
            ["row:0"]
        );
    }

    #[test]
    #[should_panic(expected = "duplicate workspace id")]
    fn registry_rejects_duplicate_ids() {
        let entry = descriptor(WorkspaceId::new("duplicate"), 'd', &["DUP"]);
        let _ = WorkspaceRegistry::new(vec![
            Box::new(Stub::new(entry)),
            Box::new(Stub::new(WorkspaceDescriptor {
                hotkey: 'x',
                ..entry
            })),
        ]);
    }

    #[test]
    #[should_panic(expected = "duplicate command alias")]
    fn registry_rejects_duplicate_aliases_case_insensitively() {
        let _ = WorkspaceRegistry::new(vec![
            Box::new(Stub::new(descriptor(
                WorkspaceId::new("one"),
                '1',
                &["SHEET"],
            ))),
            Box::new(Stub::new(descriptor(
                WorkspaceId::new("two"),
                '2',
                &["sheet"],
            ))),
        ]);
    }
}
