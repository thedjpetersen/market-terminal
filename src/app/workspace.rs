use std::collections::{HashMap, HashSet};

use crossterm::event::KeyEvent;
use ratatui::{layout::Rect, Frame};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WorkspaceId(&'static str);

impl WorkspaceId {
    pub const fn new(value: &'static str) -> Self { Self(value) }
    pub const fn as_str(self) -> &'static str { self.0 }
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

impl CommandInvocation {
    pub fn parse(command: &str) -> Option<Self> {
        let mut tokens = command.split_whitespace();
        let function = tokens.next()?.to_ascii_uppercase();
        let args = tokens.map(ToOwned::to_owned).collect();
        Some(Self { function, args })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkspaceNavigationItem {
    pub id: WorkspaceId,
    pub label: &'static str,
    pub hotkey: Option<char>,
}

/// The only application-level mutations a feature may request.
///
/// Keeping these intents small and validated at the registry boundary lets
/// features (including AI-backed features) coordinate the shell without
/// importing or mutating `App` directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppIntent {
    ActivateWorkspace { target: String },
    BringWorkspaceForward { target: String },
    DispatchCommand { command: String, origin: WorkspaceId },
    RestoreWorkspaceOrder,
}

pub trait Workspace: Send {
    fn descriptor(&self) -> WorkspaceDescriptor;
    fn render(&self, frame: &mut Frame, area: Rect);

    /// Gives the active feature first refusal on navigation-mode input.
    /// Returning `true` prevents application-level routing of the key.
    fn handle_key(&mut self, _key: KeyEvent) -> bool { false }

    /// Receives a command after its function alias resolves to this workspace.
    fn handle_command(&mut self, _invocation: &CommandInvocation) -> bool { false }

    /// Direct feature hotkeys are optional. Existing descriptors can opt out with `\0`.
    fn hotkey(&self) -> Option<char> {
        let hotkey = self.descriptor().hotkey;
        (hotkey != '\0').then_some(hotkey)
    }

    /// Favorites appear in the top navigation even when they have no hotkey.
    fn is_favorite(&self) -> bool { false }

    /// Polls asynchronous feature work and returns validated shell requests.
    fn poll_intents(&mut self) -> Vec<AppIntent> { Vec::new() }
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
        Self { entries, default_order, aliases, hotkeys }
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
            Some(WorkspaceNavigationItem { id: descriptor.id, label: descriptor.label, hotkey })
        })
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
        self.aliases.get(&invocation.function.to_ascii_uppercase()).copied()
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
        if let Some(workspace) = self.entries.iter().find(|entry| entry.descriptor().id == id) {
            workspace.render(frame, area);
        }
    }

    pub fn handle_key(&mut self, id: WorkspaceId, key: KeyEvent) -> bool {
        self.entries
            .iter_mut()
            .find(|entry| entry.descriptor().id == id)
            .is_some_and(|workspace| workspace.handle_key(key))
    }

    pub fn poll_intents(&mut self) -> Vec<AppIntent> {
        self.entries
            .iter_mut()
            .flat_map(|workspace| workspace.poll_intents())
            .collect()
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
        if let Some(index) = self.entries.iter().position(|entry| entry.descriptor().id == id) {
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

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    struct Stub {
        descriptor: WorkspaceDescriptor,
        hotkey: Option<char>,
        favorite: bool,
        invocation: Option<Arc<Mutex<Option<CommandInvocation>>>>,
    }

    impl Stub {
        fn new(descriptor: WorkspaceDescriptor) -> Self {
            Self {
                descriptor,
                hotkey: (descriptor.hotkey != '\0').then_some(descriptor.hotkey),
                favorite: true,
                invocation: None,
            }
        }
    }

    impl Workspace for Stub {
        fn descriptor(&self) -> WorkspaceDescriptor { self.descriptor }
        fn render(&self, _frame: &mut Frame, _area: Rect) {}
        fn hotkey(&self) -> Option<char> { self.hotkey }
        fn is_favorite(&self) -> bool { self.favorite }
        fn handle_command(&mut self, invocation: &CommandInvocation) -> bool {
            if let Some(captured) = &self.invocation {
                *captured.lock().expect("capture lock") = Some(invocation.clone());
            }
            true
        }
    }

    fn descriptor(
        id: WorkspaceId,
        hotkey: char,
        commands: &'static [&'static str],
    ) -> WorkspaceDescriptor {
        WorkspaceDescriptor { id, label: "TEST", hotkey, commands }
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
    #[should_panic(expected = "duplicate workspace id")]
    fn registry_rejects_duplicate_ids() {
        let entry = descriptor(WorkspaceId::new("duplicate"), 'd', &["DUP"]);
        let _ = WorkspaceRegistry::new(vec![
            Box::new(Stub::new(entry)),
            Box::new(Stub::new(WorkspaceDescriptor { hotkey: 'x', ..entry })),
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
