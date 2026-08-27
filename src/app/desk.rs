//! Responsive split-desk composition adapted from `makeev/alphai-tui`
//! commit `9143d2e1176d0a67a9f26960427cf370187fc2e6` (MIT, Copyright (c) 2026
//! Mikhail Makeev). The upstream split-view idea is reworked here as a generic
//! shell-level composition of existing Market Terminal workspaces; see
//! `THIRD_PARTY_NOTICES.md`.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::ui::{
    contains,
    theme::{AMBER, BG, CYAN, INK, MUTED, NAV_BG},
};

use super::{
    AppIntent, CommandInvocation, ShellContext, Workspace, WorkspaceDescriptor, WorkspaceId,
};

pub const DESK_ID: WorkspaceId = WorkspaceId::new("desk");
const NEWS_MIN_HEIGHT: u16 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeskPane {
    Monitor,
    Chart,
    News,
}

impl DeskPane {
    const fn index(self) -> usize {
        match self {
            Self::Monitor => 0,
            Self::Chart => 1,
            Self::News => 2,
        }
    }

    const fn from_index(index: usize) -> Self {
        match index % 3 {
            0 => Self::Monitor,
            1 => Self::Chart,
            _ => Self::News,
        }
    }

    const fn next(self, delta: isize) -> Self {
        Self::from_index((self.index() as isize + delta).rem_euclid(3) as usize)
    }
}

#[derive(Debug, Clone, Copy)]
struct PaneArea {
    frame: Rect,
    body: Rect,
}

#[derive(Debug, Clone, Copy)]
struct DeskAreas {
    monitor: PaneArea,
    chart: PaneArea,
    news: Option<PaneArea>,
}

pub struct DeskWorkspace {
    monitor: Box<dyn Workspace>,
    chart: Box<dyn Workspace>,
    news: Box<dyn Workspace>,
    focused: DeskPane,
}

impl DeskWorkspace {
    pub fn new(
        monitor: Box<dyn Workspace>,
        chart: Box<dyn Workspace>,
        news: Box<dyn Workspace>,
    ) -> Self {
        Self {
            monitor,
            chart,
            news,
            focused: DeskPane::Monitor,
        }
    }

    fn select(&mut self, pane: DeskPane) {
        if pane == self.focused {
            return;
        }
        self.focused_workspace().on_blur();
        self.focused = pane;
        self.focused_workspace().on_focus();
    }

    fn focused_workspace(&mut self) -> &mut dyn Workspace {
        match self.focused {
            DeskPane::Monitor => self.monitor.as_mut(),
            DeskPane::Chart => self.chart.as_mut(),
            DeskPane::News => self.news.as_mut(),
        }
    }

    fn route_mouse(&mut self, event: MouseEvent, areas: DeskAreas) -> bool {
        for (pane, pane_area) in [
            (DeskPane::Monitor, Some(areas.monitor)),
            (DeskPane::Chart, Some(areas.chart)),
            (DeskPane::News, areas.news),
        ] {
            let Some(pane_area) = pane_area else { continue };
            if !contains(pane_area.frame, event.column, event.row) {
                continue;
            }
            self.select(pane);
            if contains(pane_area.body, event.column, event.row) {
                return self.focused_workspace().handle_mouse(event, pane_area.body);
            }
            return true;
        }
        false
    }
}

impl Workspace for DeskWorkspace {
    fn descriptor(&self) -> WorkspaceDescriptor {
        WorkspaceDescriptor {
            id: DESK_ID,
            label: "DESK",
            hotkey: 'd',
            commands: &["DESK", "SPLIT", "DASHBOARD"],
        }
    }

    fn is_favorite(&self) -> bool {
        true
    }

    fn handle_command(&mut self, invocation: &CommandInvocation) -> bool {
        if let Some(pane) = invocation.args.first() {
            match pane.to_ascii_uppercase().as_str() {
                "MON" | "MONITOR" | "WATCHLIST" => self.select(DeskPane::Monitor),
                "CHART" | "GRAPH" => self.select(DeskPane::Chart),
                "NEWS" | "HEADLINES" => self.select(DeskPane::News),
                _ => {}
            }
        }
        true
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Tab if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.select(self.focused.next(-1));
                true
            }
            KeyCode::Tab => {
                self.select(self.focused.next(1));
                true
            }
            KeyCode::BackTab => {
                self.select(self.focused.next(-1));
                true
            }
            KeyCode::Char('1') => {
                self.select(DeskPane::Monitor);
                true
            }
            KeyCode::Char('2') => {
                self.select(DeskPane::Chart);
                true
            }
            KeyCode::Char('3') => {
                self.select(DeskPane::News);
                true
            }
            _ => self.focused_workspace().handle_key(key),
        }
    }

    fn handle_mouse(&mut self, event: MouseEvent, area: Rect) -> bool {
        self.route_mouse(event, desk_areas(area))
    }

    fn on_focus(&mut self) {
        self.focused_workspace().on_focus();
    }

    fn on_blur(&mut self) {
        self.monitor.on_blur();
        self.chart.on_blur();
        self.news.on_blur();
    }

    fn poll_intents(&mut self) -> Vec<AppIntent> {
        let mut intents = self.monitor.poll_intents();
        intents.extend(self.chart.poll_intents());
        intents.extend(self.news.poll_intents());
        intents
    }

    fn update_shell_context(&mut self, context: &ShellContext) {
        self.monitor.update_shell_context(context);
        self.chart.update_shell_context(context);
        self.news.update_shell_context(context);
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let areas = desk_areas(area);
        render_pane(
            frame,
            areas.monitor,
            "1 MONITOR",
            self.focused == DeskPane::Monitor,
            self.monitor.as_ref(),
        );
        render_pane(
            frame,
            areas.chart,
            "2 CHART",
            self.focused == DeskPane::Chart,
            self.chart.as_ref(),
        );
        if let Some(news) = areas.news {
            render_pane(
                frame,
                news,
                "3 NEWS",
                self.focused == DeskPane::News,
                self.news.as_ref(),
            );
        } else {
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(" NEWS HIDDEN ", AMBER),
                    Span::styled("terminal is too short · use NEWS for full view", MUTED),
                ]))
                .style(Style::new().bg(NAV_BG)),
                Rect::new(
                    area.x,
                    area.y.saturating_add(area.height.saturating_sub(1)),
                    area.width,
                    1.min(area.height),
                ),
            );
        }
    }
}

fn desk_areas(area: Rect) -> DeskAreas {
    let (top, news) = if area.height >= NEWS_MIN_HEIGHT {
        let rows =
            Layout::vertical([Constraint::Percentage(55), Constraint::Percentage(45)]).split(area);
        (rows[0], Some(pane_area(rows[1])))
    } else {
        let top = Rect::new(area.x, area.y, area.width, area.height.saturating_sub(1));
        (top, None)
    };
    let columns =
        Layout::horizontal([Constraint::Percentage(45), Constraint::Percentage(55)]).split(top);
    DeskAreas {
        monitor: pane_area(columns[0]),
        chart: pane_area(columns[1]),
        news,
    }
}

fn pane_area(frame: Rect) -> PaneArea {
    PaneArea {
        frame,
        body: Rect::new(
            frame.x,
            frame.y.saturating_add(1),
            frame.width,
            frame.height.saturating_sub(1),
        ),
    }
}

fn render_pane(
    frame: &mut Frame,
    area: PaneArea,
    label: &'static str,
    focused: bool,
    workspace: &dyn Workspace,
) {
    let style = if focused {
        Style::new().bg(CYAN).fg(BG).bold()
    } else {
        Style::new().bg(NAV_BG).fg(INK)
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(format!(" {label} "), style),
            Span::styled(" TAB CYCLES PANES ", MUTED),
        ]))
        .style(Style::new().bg(NAV_BG)),
        Rect::new(area.frame.x, area.frame.y, area.frame.width, 1),
    );
    workspace.render(frame, area.body);
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use crossterm::event::{MouseButton, MouseEventKind};
    use ratatui::{backend::TestBackend, Terminal};

    use super::*;

    struct StubWorkspace {
        label: &'static str,
        keys: Arc<Mutex<Vec<KeyCode>>>,
        mouse: Arc<Mutex<usize>>,
    }

    type StubParts = (
        Box<dyn Workspace>,
        Arc<Mutex<Vec<KeyCode>>>,
        Arc<Mutex<usize>>,
    );

    impl Workspace for StubWorkspace {
        fn descriptor(&self) -> WorkspaceDescriptor {
            WorkspaceDescriptor {
                id: WorkspaceId::new("stub"),
                label: self.label,
                hotkey: '\0',
                commands: &[],
            }
        }

        fn handle_key(&mut self, key: KeyEvent) -> bool {
            self.keys.lock().expect("keys lock").push(key.code);
            true
        }

        fn handle_mouse(&mut self, _event: MouseEvent, _area: Rect) -> bool {
            *self.mouse.lock().expect("mouse lock") += 1;
            true
        }

        fn render(&self, frame: &mut Frame, area: Rect) {
            frame.render_widget(Paragraph::new(self.label), area);
        }
    }

    fn stub(label: &'static str) -> StubParts {
        let keys = Arc::new(Mutex::new(Vec::new()));
        let mouse = Arc::new(Mutex::new(0));
        (
            Box::new(StubWorkspace {
                label,
                keys: keys.clone(),
                mouse: mouse.clone(),
            }),
            keys,
            mouse,
        )
    }

    fn click(column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    #[test]
    fn tab_and_number_keys_change_which_child_receives_input() {
        let (monitor, monitor_keys, _) = stub("MONITOR CHILD");
        let (chart, chart_keys, _) = stub("CHART CHILD");
        let (news, news_keys, _) = stub("NEWS CHILD");
        let mut desk = DeskWorkspace::new(monitor, chart, news);

        desk.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        desk.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        desk.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
        desk.handle_key(KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE));
        desk.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));

        assert_eq!(*monitor_keys.lock().unwrap(), [KeyCode::Char('j')]);
        assert_eq!(*chart_keys.lock().unwrap(), [KeyCode::Char('c')]);
        assert_eq!(*news_keys.lock().unwrap(), [KeyCode::Char('n')]);
    }

    #[test]
    fn clicking_a_pane_focuses_and_routes_to_its_real_body() {
        let (monitor, _, monitor_mouse) = stub("MONITOR CHILD");
        let (chart, _, chart_mouse) = stub("CHART CHILD");
        let (news, _, news_mouse) = stub("NEWS CHILD");
        let mut desk = DeskWorkspace::new(monitor, chart, news);
        let area = Rect::new(0, 0, 160, 42);
        let areas = desk_areas(area);

        assert!(desk.handle_mouse(click(areas.chart.body.x + 2, areas.chart.body.y + 2), area));
        assert!(desk.handle_mouse(
            click(
                areas.news.unwrap().body.x + 2,
                areas.news.unwrap().body.y + 2
            ),
            area
        ));

        assert_eq!(*monitor_mouse.lock().unwrap(), 0);
        assert_eq!(*chart_mouse.lock().unwrap(), 1);
        assert_eq!(*news_mouse.lock().unwrap(), 1);
        assert_eq!(desk.focused, DeskPane::News);
    }

    #[test]
    fn desk_renders_all_three_children_at_supported_size() {
        let (monitor, _, _) = stub("MONITOR CHILD");
        let (chart, _, _) = stub("CHART CHILD");
        let (news, _, _) = stub("NEWS CHILD");
        let desk = DeskWorkspace::new(monitor, chart, news);
        let mut terminal = Terminal::new(TestBackend::new(160, 42)).unwrap();

        terminal
            .draw(|frame| desk.render(frame, frame.area()))
            .unwrap();

        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("MONITOR CHILD"));
        assert!(rendered.contains("CHART CHILD"));
        assert!(rendered.contains("NEWS CHILD"));
    }
}
