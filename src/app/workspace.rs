use std::collections::HashSet;

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
    pub hotkey: char,
    pub commands: &'static [&'static str],
}

pub trait Workspace: Send {
    fn descriptor(&self) -> WorkspaceDescriptor;
    fn render(&self, frame: &mut Frame, area: Rect);
    fn handle_key(&mut self, _key: KeyEvent) -> bool { false }
}

pub struct WorkspaceRegistry {
    entries: Vec<Box<dyn Workspace>>,
}

impl WorkspaceRegistry {
    pub fn new(entries: Vec<Box<dyn Workspace>>) -> Self {
        let registry = Self { entries };
        registry.assert_valid();
        registry
    }

    pub fn descriptors(&self) -> impl Iterator<Item = WorkspaceDescriptor> + '_ {
        self.entries.iter().map(|workspace| workspace.descriptor())
    }

    pub fn resolve_hotkey(&self, hotkey: char) -> Option<WorkspaceId> {
        self.descriptors()
            .find(|descriptor| descriptor.hotkey.eq_ignore_ascii_case(&hotkey))
            .map(|descriptor| descriptor.id)
    }

    pub fn resolve_command(&self, command: &str) -> Option<WorkspaceId> {
        let normalized = command.trim().to_ascii_uppercase();
        self.descriptors()
            .find(|descriptor| {
                descriptor.commands.iter().any(|alias| normalized.contains(alias))
            })
            .map(|descriptor| descriptor.id)
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

    fn assert_valid(&self) {
        let mut ids = HashSet::new();
        let mut hotkeys = HashSet::new();
        for descriptor in self.descriptors() {
            assert!(ids.insert(descriptor.id), "duplicate workspace id: {}", descriptor.id.as_str());
            assert!(hotkeys.insert(descriptor.hotkey.to_ascii_lowercase()), "duplicate workspace hotkey: {}", descriptor.hotkey);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Stub(WorkspaceDescriptor);
    impl Workspace for Stub {
        fn descriptor(&self) -> WorkspaceDescriptor { self.0 }
        fn render(&self, _frame: &mut Frame, _area: Rect) {}
    }

    #[test]
    fn registry_resolves_commands_and_hotkeys() {
        let id = WorkspaceId::new("test");
        let registry = WorkspaceRegistry::new(vec![Box::new(Stub(WorkspaceDescriptor {
            id,
            label: "TEST",
            hotkey: 't',
            commands: &["TEST", "TST"],
        }))]);
        assert_eq!(registry.resolve_hotkey('T'), Some(id));
        assert_eq!(registry.resolve_command("tst go"), Some(id));
    }

    #[test]
    #[should_panic(expected = "duplicate workspace id")]
    fn registry_rejects_duplicate_ids() {
        let descriptor = WorkspaceDescriptor {
            id: WorkspaceId::new("duplicate"),
            label: "DUPLICATE",
            hotkey: 'd',
            commands: &["DUP"],
        };
        let _ = WorkspaceRegistry::new(vec![Box::new(Stub(descriptor)), Box::new(Stub(WorkspaceDescriptor { hotkey: 'x', ..descriptor }))]);
    }
}
