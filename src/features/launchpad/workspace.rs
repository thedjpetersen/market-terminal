use std::{
    cell::Cell,
    sync::{
        mpsc::{channel, sync_channel, Receiver, SyncSender, TrySendError},
        Arc,
    },
    thread::JoinHandle,
};

use crossterm::event::{KeyCode, KeyEvent, MouseEvent};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use crate::{
    app::{AppIntent, CommandInvocation, Workspace, WorkspaceAction, WorkspaceDescriptor},
    ui::{
        components::terminal_block,
        contains, is_primary_click,
        theme::{AMBER, BG, CYAN, GREEN, INK, MUTED, RED, YELLOW},
    },
};

use super::{LaunchpadState, LaunchpadStateError, LaunchpadStateStore, ID, MAX_LAUNCHPAD_TILES};

struct PersistResult {
    revision: u64,
    result: Result<(), LaunchpadStateError>,
}

pub struct LaunchpadWorkspace {
    state: LaunchpadState,
    selected: usize,
    status: String,
    persistence_status: String,
    persisted_revision: u64,
    persist_sender: Option<SyncSender<LaunchpadState>>,
    persist_receiver: Option<Receiver<PersistResult>>,
    pending_persist: Option<LaunchpadState>,
    persist_worker: Option<JoinHandle<()>>,
    pending_intents: Vec<AppIntent>,
    delete_armed: Option<u64>,
    visible_columns: Cell<usize>,
}

impl LaunchpadWorkspace {
    pub fn new() -> Self {
        Self::configured(None)
    }

    pub fn persistent(store: Arc<dyn LaunchpadStateStore>) -> Self {
        Self::configured(Some(store))
    }

    fn configured(store: Option<Arc<dyn LaunchpadStateStore>>) -> Self {
        let (state, persistence_status) = match store.as_ref().map(|store| store.load_launchpad()) {
            None => (LaunchpadState::seeded(), "PROCESS-LOCAL TILES".to_owned()),
            Some(Ok(Some(state))) => {
                let count = state.tiles.len();
                let revision = state.revision;
                (
                    state,
                    format!("DURABLE R{revision} · RESTORED {count} TILES"),
                )
            }
            Some(Ok(None)) => (
                LaunchpadState::seeded(),
                "DURABLE STATE READY · VERSIONED SEEDS".to_owned(),
            ),
            Some(Err(error)) => (
                LaunchpadState::seeded(),
                format!("DURABLE STATE LOAD ERROR · {error}"),
            ),
        };
        let persisted_revision = state.revision;
        let (persist_sender, persist_receiver, persist_worker) = if let Some(store) = store {
            let (sender, worker_receiver) = sync_channel::<LaunchpadState>(1);
            let (worker_sender, receiver) = channel::<PersistResult>();
            let worker = std::thread::Builder::new()
                .name("launchpad-state-writer".to_owned())
                .spawn(move || {
                    while let Ok(mut state) = worker_receiver.recv() {
                        while let Ok(newer) = worker_receiver.try_recv() {
                            state = newer;
                        }
                        let revision = state.revision;
                        let result = store.save_launchpad(&state);
                        if worker_sender
                            .send(PersistResult { revision, result })
                            .is_err()
                        {
                            break;
                        }
                    }
                })
                .expect("launchpad state writer should start");
            (Some(sender), Some(receiver), Some(worker))
        } else {
            (None, None, None)
        };
        Self {
            state,
            selected: 0,
            status: "SELECT A TILE · ENTER TO OPEN".to_owned(),
            persistence_status,
            persisted_revision,
            persist_sender,
            persist_receiver,
            pending_persist: None,
            persist_worker,
            pending_intents: Vec::new(),
            delete_armed: None,
            visible_columns: Cell::new(1),
        }
    }

    pub fn state(&self) -> &LaunchpadState {
        &self.state
    }

    fn handle_launch_command(&mut self, invocation: &CommandInvocation) {
        let Some(operation) = invocation
            .args
            .first()
            .map(|value| value.to_ascii_uppercase())
        else {
            self.status = "LAUNCHPAD READY · ADD, RENAME, MOVE, REMOVE, OR RESET".to_owned();
            return;
        };
        match operation.as_str() {
            "ADD" if invocation.args.len() >= 3 => {
                let label = invocation.args[1].clone();
                let command = invocation.args[2..].join(" ");
                match self.state.add(label, command) {
                    Ok(_) => {
                        self.selected = self.state.tiles.len().saturating_sub(1);
                        self.status = "TILE ADDED · DURABLE SAVE QUEUED".to_owned();
                        self.queue_persist();
                    }
                    Err(error) => self.status = format!("TILE NOT ADDED · {error}"),
                }
            }
            "RENAME" if invocation.args.len() >= 3 => {
                let Some(index) = parse_tile_index(&invocation.args[1], self.state.tiles.len())
                else {
                    self.status = "INVALID TILE NUMBER".to_owned();
                    return;
                };
                match self.state.rename(index, invocation.args[2..].join(" ")) {
                    Ok(()) => {
                        self.selected = index;
                        self.status = "TILE RENAMED · DURABLE SAVE QUEUED".to_owned();
                        self.queue_persist();
                    }
                    Err(error) => self.status = format!("TILE NOT RENAMED · {error}"),
                }
            }
            "MOVE" if invocation.args.len() == 3 => {
                let Some(from) = parse_tile_index(&invocation.args[1], self.state.tiles.len())
                else {
                    self.status = "INVALID SOURCE TILE NUMBER".to_owned();
                    return;
                };
                let Some(to) = parse_tile_index(&invocation.args[2], self.state.tiles.len()) else {
                    self.status = "INVALID DESTINATION TILE NUMBER".to_owned();
                    return;
                };
                if self.state.move_tile(from, to) {
                    self.selected = to;
                    self.status = "TILE MOVED · DURABLE SAVE QUEUED".to_owned();
                    self.queue_persist();
                } else {
                    self.status = "TILE MOVE MADE NO CHANGE".to_owned();
                }
            }
            "REMOVE" if invocation.args.len() == 2 => {
                let Some(index) = parse_tile_index(&invocation.args[1], self.state.tiles.len())
                else {
                    self.status = "INVALID TILE NUMBER".to_owned();
                    return;
                };
                self.remove_tile(index);
            }
            "RESET"
                if invocation
                    .args
                    .get(1)
                    .is_some_and(|value| value == "CONFIRM") =>
            {
                let revision = self.state.revision.saturating_add(1);
                self.state = LaunchpadState::seeded();
                self.state.revision = revision;
                self.selected = 0;
                self.status = "VERSIONED LAUNCHPAD SEEDS RESTORED".to_owned();
                self.queue_persist();
            }
            "RESET" => {
                self.status = "RESET REQUIRES: LAUNCH RESET CONFIRM".to_owned();
            }
            _ => {
                self.status = "USAGE · LAUNCH ADD <LABEL> <COMMAND> · RENAME <N> <LABEL> · MOVE <N> <N> · REMOVE <N> · RESET CONFIRM".to_owned();
            }
        }
    }

    fn activate_selected(&mut self) -> bool {
        let Some(tile) = self.state.tiles.get(self.selected) else {
            return false;
        };
        self.pending_intents.push(AppIntent::DispatchCommand {
            command: tile.command.clone(),
            origin: ID,
        });
        self.status = format!("ROUTING · {}", tile.command);
        self.delete_armed = None;
        true
    }

    fn move_selection(&mut self, amount: isize) {
        let last = self.state.tiles.len().saturating_sub(1);
        self.selected = if amount.is_negative() {
            self.selected.saturating_sub(amount.unsigned_abs())
        } else {
            self.selected.saturating_add(amount as usize).min(last)
        };
        self.delete_armed = None;
    }

    fn reorder_selected(&mut self, amount: isize) -> bool {
        if self.state.tiles.is_empty() {
            return false;
        }
        let target = if amount.is_negative() {
            self.selected.saturating_sub(amount.unsigned_abs())
        } else {
            self.selected
                .saturating_add(amount as usize)
                .min(self.state.tiles.len() - 1)
        };
        if self.state.move_tile(self.selected, target) {
            self.selected = target;
            self.status = "TILE REORDERED · DURABLE SAVE QUEUED".to_owned();
            self.queue_persist();
            true
        } else {
            false
        }
    }

    fn arm_or_remove_selected(&mut self) -> bool {
        let Some(tile) = self.state.tiles.get(self.selected) else {
            return false;
        };
        if self.delete_armed == Some(tile.id) {
            self.remove_tile(self.selected);
        } else {
            self.delete_armed = Some(tile.id);
            self.status = format!(
                "PRESS X AGAIN TO REMOVE {}",
                tile.label.to_ascii_uppercase()
            );
        }
        true
    }

    fn remove_tile(&mut self, index: usize) {
        let label = self.state.tiles[index].label.clone();
        self.state.remove(index);
        self.selected = self.selected.min(self.state.tiles.len().saturating_sub(1));
        self.delete_armed = None;
        self.status = format!(
            "{} REMOVED · DURABLE SAVE QUEUED",
            label.to_ascii_uppercase()
        );
        self.queue_persist();
    }

    fn queue_persist(&mut self) {
        let Some(_) = self.persist_sender else {
            return;
        };
        self.pending_persist = Some(self.state.clone());
        self.dispatch_pending_persist();
    }

    fn dispatch_pending_persist(&mut self) {
        let Some(state) = self.pending_persist.take() else {
            return;
        };
        let Some(sender) = &self.persist_sender else {
            return;
        };
        match sender.try_send(state) {
            Ok(()) => {}
            Err(TrySendError::Full(state)) => self.pending_persist = Some(state),
            Err(TrySendError::Disconnected(_)) => {
                self.persistence_status = "DURABLE STATE WRITER STOPPED".to_owned();
            }
        }
    }

    fn poll_persistence(&mut self) {
        if let Some(receiver) = &self.persist_receiver {
            while let Ok(result) = receiver.try_recv() {
                match result.result {
                    Ok(()) => {
                        self.persisted_revision = self.persisted_revision.max(result.revision);
                        self.persistence_status =
                            format!("DURABLE R{} · SAVED", self.persisted_revision);
                    }
                    Err(error) => {
                        self.persistence_status = format!("DURABLE STATE SAVE ERROR · {error}");
                    }
                }
            }
        }
        self.dispatch_pending_persist();
    }
}

impl Default for LaunchpadWorkspace {
    fn default() -> Self {
        Self::new()
    }
}

impl Workspace for LaunchpadWorkspace {
    fn descriptor(&self) -> WorkspaceDescriptor {
        WorkspaceDescriptor {
            id: ID,
            label: "LAUNCH",
            hotkey: 'l',
            commands: &["LAUNCH", "LAUNCHPAD"],
        }
    }

    fn handle_command(&mut self, invocation: &CommandInvocation) -> bool {
        self.handle_launch_command(invocation);
        true
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Left | KeyCode::Char('h') => {
                self.move_selection(-1);
                true
            }
            KeyCode::Right | KeyCode::Char('l') => {
                self.move_selection(1);
                true
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_selection(-(self.visible_columns.get() as isize));
                true
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_selection(self.visible_columns.get() as isize);
                true
            }
            KeyCode::Enter => self.activate_selected(),
            KeyCode::Char('<') => self.reorder_selected(-1),
            KeyCode::Char('>') => self.reorder_selected(1),
            KeyCode::Char('x' | 'X') => self.arm_or_remove_selected(),
            _ => false,
        }
    }

    fn handle_mouse(&mut self, event: MouseEvent, area: Rect) -> bool {
        if !is_primary_click(event, area) {
            return false;
        }
        for action in self.actions(area) {
            if action.enabled && contains(action.area, event.column, event.row) {
                return self.activate_action(&action.id);
            }
        }
        false
    }

    fn actions(&self, area: Rect) -> Vec<WorkspaceAction> {
        let (_, grid, _) = launchpad_regions(area);
        let (columns, tile_areas) = launchpad_tile_areas(grid, self.state.tiles.len());
        self.visible_columns.set(columns.max(1));
        self.state
            .tiles
            .iter()
            .zip(tile_areas)
            .enumerate()
            .map(|(index, (tile, area))| {
                let mut action = WorkspaceAction::new(
                    tile_action_id(tile.id, &tile.label, &tile.command),
                    format!("Open {} with {}", tile.label, tile.command),
                    area,
                );
                if index == self.selected {
                    action = action.preferred();
                }
                action
            })
            .collect()
    }

    fn activate_action(&mut self, id: &str) -> bool {
        let Some(encoded) = id.strip_prefix("tile:") else {
            return false;
        };
        let Some((tile_id, expected_digest)) = encoded.split_once(':') else {
            return false;
        };
        let Ok(tile_id) = tile_id.parse::<u64>() else {
            return false;
        };
        let Some(index) = self.state.tiles.iter().position(|tile| {
            tile.id == tile_id
                && format!("{:016x}", tile_digest(&tile.label, &tile.command)) == expected_digest
        }) else {
            return false;
        };
        self.selected = index;
        self.activate_selected()
    }

    fn poll_intents(&mut self) -> Vec<AppIntent> {
        self.poll_persistence();
        std::mem::take(&mut self.pending_intents)
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let (header, grid, footer) = launchpad_regions(area);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    " LAUNCHPAD ",
                    Style::new().bg(AMBER.into()).fg(BG.into()).bold(),
                ),
                Span::styled(
                    format!(
                        " {} / {MAX_LAUNCHPAD_TILES} TILES  ",
                        self.state.tiles.len()
                    ),
                    INK,
                ),
                Span::styled(format!("R{}  ", self.state.revision), GREEN),
                Span::styled(&self.status, YELLOW),
            ]))
            .block(terminal_block("GO", "SAVED WORK")),
            header,
        );

        let (columns, areas) = launchpad_tile_areas(grid, self.state.tiles.len());
        self.visible_columns.set(columns.max(1));
        if self.state.tiles.is_empty() {
            frame.render_widget(
                Paragraph::new(vec![
                    Line::styled(
                        "NO SAVED LAUNCH TILES",
                        Style::new().fg(MUTED.into()).bold(),
                    ),
                    Line::raw(""),
                    Line::raw("Use: LAUNCH ADD \"Apple Research\" SEC AAPL US"),
                    Line::raw("Reset seeds: LAUNCH RESET CONFIRM"),
                ])
                .wrap(Wrap { trim: true })
                .block(terminal_block("EMPTY", "CREATE YOUR FIRST TILE")),
                grid,
            );
        } else {
            for (index, (tile, tile_area)) in self.state.tiles.iter().zip(areas).enumerate() {
                let selected = index == self.selected;
                let armed = self.delete_armed == Some(tile.id);
                let border = if armed {
                    RED
                } else if selected {
                    CYAN
                } else {
                    MUTED
                };
                let style = if selected {
                    Style::new().bg(CYAN.into()).fg(BG.into()).bold()
                } else {
                    Style::new().fg(INK.into())
                };
                let block = Block::new()
                    .borders(Borders::ALL)
                    .border_style(border)
                    .title(Line::styled(format!(" {:02} ", index + 1), AMBER));
                frame.render_widget(
                    Paragraph::new(vec![
                        Line::styled(tile.label.to_ascii_uppercase(), style),
                        Line::styled(
                            tile.command.clone(),
                            if selected {
                                style
                            } else {
                                Style::new().fg(MUTED.into())
                            },
                        ),
                        Line::styled(
                            if armed {
                                "X AGAIN TO REMOVE"
                            } else {
                                "ENTER TO OPEN"
                            },
                            if armed { RED } else { AMBER },
                        ),
                    ])
                    .wrap(Wrap { trim: true })
                    .block(block),
                    tile_area,
                );
            }
        }

        frame.render_widget(
            Paragraph::new(vec![
                Line::from(vec![
                    Span::styled(" ARROWS/HJKL ", AMBER),
                    Span::raw("SELECT  "),
                    Span::styled("ENTER ", AMBER),
                    Span::raw("OPEN  "),
                    Span::styled("< / > ", AMBER),
                    Span::raw("MOVE  "),
                    Span::styled("X X ", RED),
                    Span::raw("REMOVE"),
                ]),
                Line::styled(
                    format!(
                        " {} · ADD/RENAME/MOVE/REMOVE THROUGH LAUNCH COMMANDS ",
                        self.persistence_status
                    ),
                    MUTED,
                ),
            ]),
            footer,
        );
    }
}

impl Drop for LaunchpadWorkspace {
    fn drop(&mut self) {
        if let (Some(sender), Some(state)) = (&self.persist_sender, self.pending_persist.take()) {
            let _ = sender.send(state);
        }
        self.persist_sender.take();
        if let Some(worker) = self.persist_worker.take() {
            let _ = worker.join();
        }
    }
}

fn parse_tile_index(value: &str, tile_count: usize) -> Option<usize> {
    value
        .parse::<usize>()
        .ok()
        .and_then(|index| index.checked_sub(1))
        .filter(|index| *index < tile_count)
}

fn launchpad_regions(area: Rect) -> (Rect, Rect, Rect) {
    let rows = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(6),
        Constraint::Length(2),
    ])
    .split(area);
    (rows[0], rows[1], rows[2])
}

fn launchpad_tile_areas(area: Rect, tile_count: usize) -> (usize, Vec<Rect>) {
    let columns = if area.width >= 132 {
        4
    } else if area.width >= 92 {
        3
    } else {
        2
    };
    let tile_height = 5_u16;
    let visible_rows = usize::from(area.height / tile_height).max(1);
    let visible = tile_count.min(visible_rows * columns);
    let width = area.width / columns as u16;
    let areas = (0..visible)
        .map(|index| {
            let column = index % columns;
            let row = index / columns;
            let x = area.x.saturating_add(width.saturating_mul(column as u16));
            let y = area
                .y
                .saturating_add(tile_height.saturating_mul(row as u16));
            let tile_width = if column + 1 == columns {
                area.right().saturating_sub(x)
            } else {
                width
            };
            Rect::new(
                x,
                y,
                tile_width,
                tile_height.min(area.bottom().saturating_sub(y)),
            )
        })
        .collect();
    (columns, areas)
}

fn tile_action_id(id: u64, label: &str, command: &str) -> String {
    format!("tile:{id}:{:016x}", tile_digest(label, command))
}

fn tile_digest(label: &str, command: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in label.bytes().chain([0]).chain(command.bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    #[derive(Default)]
    struct MemoryStore(Mutex<Option<LaunchpadState>>);

    impl LaunchpadStateStore for MemoryStore {
        fn load_launchpad(&self) -> Result<Option<LaunchpadState>, LaunchpadStateError> {
            Ok(self.0.lock().unwrap().clone())
        }

        fn save_launchpad(&self, state: &LaunchpadState) -> Result<(), LaunchpadStateError> {
            *self.0.lock().unwrap() = Some(state.clone());
            Ok(())
        }
    }

    fn command(function: &str, args: &[&str]) -> CommandInvocation {
        CommandInvocation {
            function: function.to_owned(),
            args: args.iter().map(|value| (*value).to_owned()).collect(),
        }
    }

    #[test]
    fn commands_edit_reorder_and_remove_tiles() {
        let mut workspace = LaunchpadWorkspace::new();
        workspace.handle_command(&command("LAUNCH", &["ADD", "Apple", "SEC", "AAPL", "US"]));
        assert_eq!(workspace.state.tiles.last().unwrap().command, "SEC AAPL US");
        workspace.handle_command(&command("LAUNCH", &["RENAME", "9", "Apple", "Research"]));
        assert_eq!(workspace.state.tiles[8].label, "Apple Research");
        workspace.handle_command(&command("LAUNCH", &["MOVE", "9", "1"]));
        assert_eq!(workspace.state.tiles[0].label, "Apple Research");
        workspace.handle_command(&command("LAUNCH", &["REMOVE", "1"]));
        assert_eq!(workspace.state.tiles.len(), 8);
    }

    #[test]
    fn action_identity_is_revalidated_before_routing() {
        let mut workspace = LaunchpadWorkspace::new();
        let area = Rect::new(0, 0, 120, 30);
        let action = workspace.actions(area)[0].id.clone();
        workspace.state.tiles[0].command = "NEWS".to_owned();
        assert!(!workspace.activate_action(&action));
        let fresh = workspace.actions(area)[0].id.clone();
        assert!(workspace.activate_action(&fresh));
        assert!(matches!(
            workspace.poll_intents().as_slice(),
            [AppIntent::DispatchCommand { command, origin }] if command == "NEWS" && *origin == ID
        ));
    }

    #[test]
    fn durable_edits_survive_workspace_restart() {
        let store = Arc::new(MemoryStore::default());
        {
            let mut workspace = LaunchpadWorkspace::persistent(store.clone());
            workspace.handle_command(&command("LAUNCH", &["ADD", "Apple", "SEC", "AAPL", "US"]));
        }
        let restored = LaunchpadWorkspace::persistent(store);
        assert_eq!(restored.state.tiles.last().unwrap().label, "Apple");
        assert_eq!(restored.state.tiles.last().unwrap().command, "SEC AAPL US");
    }
}
