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
    contains, is_primary_click,
    theme::{AMBER, BG, CYAN, INK, MUTED, NAV_BG},
};

use super::{
    workspace::{sanitize_actions, MAX_WORKSPACE_ACTIONS},
    AppIntent, CommandInvocation, ShellContext, ViewRestoreReport, ViewValue, Workspace,
    WorkspaceAction, WorkspaceDescriptor, WorkspaceId, WorkspaceViewState,
};

pub const DESK_ID: WorkspaceId = WorkspaceId::new("desk");
const NEWS_MIN_HEIGHT: u16 = 30;
const DEFAULT_MONITOR_PERCENT: u16 = 45;
const DEFAULT_TOP_PERCENT: u16 = 55;
const MIN_MONITOR_PERCENT: u16 = 30;
const MAX_MONITOR_PERCENT: u16 = 70;
const MIN_TOP_PERCENT: u16 = 40;
const MAX_TOP_PERCENT: u16 = 75;
const RESIZE_STEP: i16 = 5;
const CONTROL_STRIP_WIDTH: u16 = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeskPane {
    Monitor,
    Chart,
    News,
}

impl DeskPane {
    const ALL: [Self; 3] = [Self::Monitor, Self::Chart, Self::News];

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

    const fn action_id(self) -> &'static str {
        match self {
            Self::Monitor => "pane:monitor",
            Self::Chart => "pane:chart",
            Self::News => "pane:news",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Monitor => "Monitor",
            Self::Chart => "Chart",
            Self::News => "News",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|pane| pane.label().eq_ignore_ascii_case(value))
    }

    fn child_action_prefix(self) -> String {
        format!("{}/", self.action_id())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DeskGeometry {
    monitor_percent: u16,
    top_percent: u16,
}

impl Default for DeskGeometry {
    fn default() -> Self {
        Self {
            monitor_percent: DEFAULT_MONITOR_PERCENT,
            top_percent: DEFAULT_TOP_PERCENT,
        }
    }
}

impl DeskGeometry {
    fn new(monitor_percent: u16, top_percent: u16) -> Option<Self> {
        (MIN_MONITOR_PERCENT..=MAX_MONITOR_PERCENT)
            .contains(&monitor_percent)
            .then_some(())?;
        (MIN_TOP_PERCENT..=MAX_TOP_PERCENT)
            .contains(&top_percent)
            .then_some(Self {
                monitor_percent,
                top_percent,
            })
    }

    fn adjust_monitor(&mut self, delta: i16) -> bool {
        adjust_percent(
            &mut self.monitor_percent,
            delta,
            MIN_MONITOR_PERCENT,
            MAX_MONITOR_PERCENT,
        )
    }

    fn adjust_top(&mut self, delta: i16) -> bool {
        adjust_percent(
            &mut self.top_percent,
            delta,
            MIN_TOP_PERCENT,
            MAX_TOP_PERCENT,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeskResize {
    MonitorLess,
    MonitorMore,
    TopLess,
    TopMore,
}

impl DeskResize {
    const fn action_id(self) -> &'static str {
        match self {
            Self::MonitorLess => "resize:columns:less",
            Self::MonitorMore => "resize:columns:more",
            Self::TopLess => "resize:rows:less",
            Self::TopMore => "resize:rows:more",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::MonitorLess => "Give Chart more width",
            Self::MonitorMore => "Give Monitor more width",
            Self::TopLess => "Give News more height",
            Self::TopMore => "Give market panes more height",
        }
    }

    fn parse(id: &str) -> Option<Self> {
        [
            Self::MonitorLess,
            Self::MonitorMore,
            Self::TopLess,
            Self::TopMore,
        ]
        .into_iter()
        .find(|control| control.action_id() == id)
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
    geometry: DeskGeometry,
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
            geometry: DeskGeometry::default(),
        }
    }

    fn resize(&mut self, control: DeskResize) -> bool {
        match control {
            DeskResize::MonitorLess => self.geometry.adjust_monitor(-RESIZE_STEP),
            DeskResize::MonitorMore => self.geometry.adjust_monitor(RESIZE_STEP),
            DeskResize::TopLess => self.geometry.adjust_top(-RESIZE_STEP),
            DeskResize::TopMore => self.geometry.adjust_top(RESIZE_STEP),
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
        self.workspace_mut(self.focused)
    }

    fn workspace(&self, pane: DeskPane) -> &dyn Workspace {
        match pane {
            DeskPane::Monitor => self.monitor.as_ref(),
            DeskPane::Chart => self.chart.as_ref(),
            DeskPane::News => self.news.as_ref(),
        }
    }

    fn workspace_mut(&mut self, pane: DeskPane) -> &mut dyn Workspace {
        match pane {
            DeskPane::Monitor => self.monitor.as_mut(),
            DeskPane::Chart => self.chart.as_mut(),
            DeskPane::News => self.news.as_mut(),
        }
    }

    fn route_mouse(&mut self, event: MouseEvent, areas: DeskAreas) -> bool {
        for (control, control_area) in desk_resize_control_areas(areas) {
            if is_primary_click(event, control_area) {
                self.resize(control);
                return true;
            }
        }
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
        let Some(operation) = invocation.args.first() else {
            return true;
        };
        match operation.to_ascii_uppercase().as_str() {
            "MON" | "MONITOR" | "WATCHLIST" if invocation.args.len() == 1 => {
                self.select(DeskPane::Monitor)
            }
            "CHART" | "GRAPH" if invocation.args.len() == 1 => self.select(DeskPane::Chart),
            "NEWS" | "HEADLINES" if invocation.args.len() == 1 => self.select(DeskPane::News),
            "COLUMNS" | "WIDTH" if invocation.args.len() == 2 => {
                if let Some(percent) = parse_percent(
                    &invocation.args[1],
                    MIN_MONITOR_PERCENT,
                    MAX_MONITOR_PERCENT,
                ) {
                    self.geometry.monitor_percent = percent;
                }
            }
            "ROWS" | "HEIGHT" if invocation.args.len() == 2 => {
                if let Some(percent) =
                    parse_percent(&invocation.args[1], MIN_TOP_PERCENT, MAX_TOP_PERCENT)
                {
                    self.geometry.top_percent = percent;
                }
            }
            "LAYOUT" if invocation.args.len() == 3 => {
                if let (Ok(monitor), Ok(top)) = (
                    invocation.args[1].parse::<u16>(),
                    invocation.args[2].parse::<u16>(),
                ) {
                    if let Some(geometry) = DeskGeometry::new(monitor, top) {
                        self.geometry = geometry;
                    }
                }
            }
            "RESET" if invocation.args.len() == 1 => self.geometry = DeskGeometry::default(),
            _ => {}
        }
        true
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Left if key.modifiers.contains(KeyModifiers::ALT) => {
                self.resize(DeskResize::MonitorLess);
                true
            }
            KeyCode::Right if key.modifiers.contains(KeyModifiers::ALT) => {
                self.resize(DeskResize::MonitorMore);
                true
            }
            KeyCode::Up if key.modifiers.contains(KeyModifiers::ALT) => {
                self.resize(DeskResize::TopLess);
                true
            }
            KeyCode::Down if key.modifiers.contains(KeyModifiers::ALT) => {
                self.resize(DeskResize::TopMore);
                true
            }
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
        self.route_mouse(event, desk_areas_with_geometry(area, self.geometry))
    }

    fn actions(&self, area: Rect) -> Vec<WorkspaceAction> {
        let areas = desk_areas_with_geometry(area, self.geometry);
        let visible = [
            (DeskPane::Monitor, Some(areas.monitor)),
            (DeskPane::Chart, Some(areas.chart)),
            (DeskPane::News, areas.news),
        ]
        .into_iter()
        .filter_map(|(pane, area)| area.map(|area| (pane, area)))
        .collect::<Vec<_>>();
        let preferred = visible
            .iter()
            .find_map(|(pane, _)| (*pane == self.focused).then_some(*pane))
            .unwrap_or(DeskPane::Monitor);
        let mut actions = Vec::new();

        for (pane, pane_area) in &visible {
            // The pane destination owns its complete header so Left/Right keeps
            // the direct pane-to-pane route. Resize controls are nested targets:
            // mouse routing checks them first and follow hints still expose them.
            let header = Rect::new(
                pane_area.frame.x,
                pane_area.frame.y,
                pane_area.frame.width,
                1,
            );
            let mut pane_action = WorkspaceAction::new(
                pane.action_id(),
                format!("Focus {} pane", pane.label()),
                header,
            );
            if *pane == preferred {
                pane_action = pane_action.preferred();
            }
            actions.push(pane_action);
        }

        actions.extend(
            desk_resize_control_areas(areas)
                .into_iter()
                .filter(|(control, _)| resize_is_available(self.geometry, *control))
                .map(|(control, area)| {
                    WorkspaceAction::new(control.action_id(), control.label(), area)
                }),
        );

        for (pane, pane_area) in visible {
            let prefix = pane.child_action_prefix();
            let remaining = MAX_WORKSPACE_ACTIONS.saturating_sub(actions.len());
            let child_actions = self
                .workspace(pane)
                .actions(pane_area.body)
                .into_iter()
                .map(|mut action| {
                    action.id = format!("{prefix}{}", action.id);
                    action.label = format!("{} · {}", pane.label(), action.label);
                    action.preferred = false;
                    action
                });
            actions.extend(sanitize_actions(child_actions, pane_area.body, remaining));
            if actions.len() == MAX_WORKSPACE_ACTIONS {
                break;
            }
        }
        actions
    }

    fn activate_action(&mut self, id: &str) -> bool {
        if let Some(control) = DeskResize::parse(id) {
            return self.resize(control);
        }
        for pane in DeskPane::ALL {
            if id == pane.action_id() {
                self.select(pane);
                return true;
            }
            let prefix = pane.child_action_prefix();
            let Some(child_id) = id.strip_prefix(&prefix) else {
                continue;
            };
            let activated = self.workspace_mut(pane).activate_action(child_id);
            if activated {
                self.select(pane);
            }
            return activated;
        }
        false
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
        let areas = desk_areas_with_geometry(area, self.geometry);
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
                .style(Style::new().bg(NAV_BG.into())),
                Rect::new(
                    area.x,
                    area.y.saturating_add(area.height.saturating_sub(1)),
                    area.width,
                    1.min(area.height),
                ),
            );
        }
        render_resize_controls(frame, areas, self.geometry);
    }

    fn capture_view(&self) -> WorkspaceViewState {
        WorkspaceViewState::new(DESK_ID.as_str())
            .with_field(
                "focused_pane",
                ViewValue::Text(self.focused.label().to_owned()),
            )
            .with_field(
                "monitor_percent",
                ViewValue::Unsigned(u64::from(self.geometry.monitor_percent)),
            )
            .with_field(
                "top_percent",
                ViewValue::Unsigned(u64::from(self.geometry.top_percent)),
            )
            .with_child(self.monitor.capture_view())
            .with_child(self.chart.capture_view())
            .with_child(self.news.capture_view())
    }

    fn restore_view(&mut self, state: &WorkspaceViewState) -> ViewRestoreReport {
        if !state.workspace.eq_ignore_ascii_case(DESK_ID.as_str()) {
            return ViewRestoreReport::warning(format!(
                "saved state belongs to {}, not desk",
                state.workspace
            ));
        }
        let mut report = ViewRestoreReport::default();
        if let Some(value) = state.fields.get("focused_pane") {
            match value.as_text().and_then(DeskPane::parse) {
                Some(pane) => {
                    self.select(pane);
                    report.restored_fields += 1;
                }
                None => {
                    report.skipped_fields += 1;
                    report
                        .warnings
                        .push("desk focus target is unavailable".to_owned());
                }
            }
        }
        self.geometry = DeskGeometry::default();
        restore_geometry_field(
            state,
            "monitor_percent",
            MIN_MONITOR_PERCENT,
            MAX_MONITOR_PERCENT,
            &mut self.geometry.monitor_percent,
            &mut report,
        );
        restore_geometry_field(
            state,
            "top_percent",
            MIN_TOP_PERCENT,
            MAX_TOP_PERCENT,
            &mut self.geometry.top_percent,
            &mut report,
        );
        for child in &state.children {
            let child_report = DeskPane::ALL
                .into_iter()
                .find(|pane| {
                    self.workspace(*pane)
                        .descriptor()
                        .id
                        .as_str()
                        .eq_ignore_ascii_case(&child.workspace)
                })
                .map_or_else(
                    || {
                        ViewRestoreReport::warning(format!(
                            "desk child {} is unavailable",
                            child.workspace
                        ))
                    },
                    |pane| self.workspace_mut(pane).restore_view(child),
                );
            report.merge(child_report);
        }
        report
    }
}

fn adjust_percent(value: &mut u16, delta: i16, minimum: u16, maximum: u16) -> bool {
    let adjusted =
        (i32::from(*value) + i32::from(delta)).clamp(i32::from(minimum), i32::from(maximum)) as u16;
    if adjusted == *value {
        return false;
    }
    *value = adjusted;
    true
}

fn parse_percent(value: &str, minimum: u16, maximum: u16) -> Option<u16> {
    value
        .parse::<u16>()
        .ok()
        .filter(|percent| (minimum..=maximum).contains(percent))
}

fn restore_geometry_field(
    state: &WorkspaceViewState,
    field: &str,
    minimum: u16,
    maximum: u16,
    destination: &mut u16,
    report: &mut ViewRestoreReport,
) {
    let Some(value) = state.fields.get(field) else {
        return;
    };
    let restored = value
        .as_unsigned()
        .and_then(|value| u16::try_from(value).ok())
        .filter(|value| (minimum..=maximum).contains(value));
    if let Some(value) = restored {
        *destination = value;
        report.restored_fields += 1;
    } else {
        report.skipped_fields += 1;
        report.warnings.push(format!(
            "desk {field} must be between {minimum} and {maximum} percent"
        ));
    }
}

fn desk_areas_with_geometry(area: Rect, geometry: DeskGeometry) -> DeskAreas {
    let (top, news) = if area.height >= NEWS_MIN_HEIGHT {
        let rows = Layout::vertical([
            Constraint::Percentage(geometry.top_percent),
            Constraint::Percentage(100 - geometry.top_percent),
        ])
        .split(area);
        (rows[0], Some(pane_area(rows[1])))
    } else {
        let top = Rect::new(area.x, area.y, area.width, area.height.saturating_sub(1));
        (top, None)
    };
    let columns = Layout::horizontal([
        Constraint::Percentage(geometry.monitor_percent),
        Constraint::Percentage(100 - geometry.monitor_percent),
    ])
    .split(top);
    DeskAreas {
        monitor: pane_area(columns[0]),
        chart: pane_area(columns[1]),
        news,
    }
}

fn resize_control_strip(frame: Rect) -> Option<Rect> {
    (frame.width >= 24).then(|| {
        Rect::new(
            frame.right().saturating_sub(CONTROL_STRIP_WIDTH),
            frame.y,
            CONTROL_STRIP_WIDTH,
            1,
        )
    })
}

fn desk_resize_control_areas(areas: DeskAreas) -> Vec<(DeskResize, Rect)> {
    let mut controls = Vec::with_capacity(4);
    if let Some(strip) = resize_control_strip(areas.monitor.frame) {
        controls.push((
            DeskResize::MonitorLess,
            Rect::new(strip.x.saturating_add(6), strip.y, 3, 1),
        ));
        controls.push((
            DeskResize::MonitorMore,
            Rect::new(strip.x.saturating_add(9), strip.y, 3, 1),
        ));
    }
    if let Some(strip) = areas.news.and_then(|news| resize_control_strip(news.frame)) {
        controls.push((
            DeskResize::TopLess,
            Rect::new(strip.x.saturating_add(6), strip.y, 3, 1),
        ));
        controls.push((
            DeskResize::TopMore,
            Rect::new(strip.x.saturating_add(9), strip.y, 3, 1),
        ));
    }
    controls
}

fn resize_is_available(geometry: DeskGeometry, control: DeskResize) -> bool {
    match control {
        DeskResize::MonitorLess => geometry.monitor_percent > MIN_MONITOR_PERCENT,
        DeskResize::MonitorMore => geometry.monitor_percent < MAX_MONITOR_PERCENT,
        DeskResize::TopLess => geometry.top_percent > MIN_TOP_PERCENT,
        DeskResize::TopMore => geometry.top_percent < MAX_TOP_PERCENT,
    }
}

fn render_resize_controls(frame: &mut Frame, areas: DeskAreas, geometry: DeskGeometry) {
    if let Some(strip) = resize_control_strip(areas.monitor.frame) {
        render_resize_control_strip(
            frame,
            strip,
            'W',
            geometry.monitor_percent,
            resize_is_available(geometry, DeskResize::MonitorLess),
            resize_is_available(geometry, DeskResize::MonitorMore),
        );
    }
    if let Some(strip) = areas.news.and_then(|news| resize_control_strip(news.frame)) {
        render_resize_control_strip(
            frame,
            strip,
            'T',
            geometry.top_percent,
            resize_is_available(geometry, DeskResize::TopLess),
            resize_is_available(geometry, DeskResize::TopMore),
        );
    }
}

fn render_resize_control_strip(
    frame: &mut Frame,
    area: Rect,
    axis: char,
    percent: u16,
    less_enabled: bool,
    more_enabled: bool,
) {
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(format!(" {axis}{percent:02}% "), MUTED),
            Span::styled("[-]", if less_enabled { AMBER } else { MUTED }),
            Span::styled("[+]", if more_enabled { AMBER } else { MUTED }),
        ]))
        .style(Style::new().bg(NAV_BG.into())),
        area,
    );
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
        Style::new().bg(CYAN.into()).fg(BG.into()).bold()
    } else {
        Style::new().bg(NAV_BG.into()).fg(INK.into())
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(format!(" {label} "), style),
            Span::styled(" TAB CYCLES PANES ", MUTED),
        ]))
        .style(Style::new().bg(NAV_BG.into())),
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

    struct ActionWorkspace {
        label: &'static str,
        activations: Arc<Mutex<Vec<String>>>,
    }

    struct FloodWorkspace;

    impl Workspace for ActionWorkspace {
        fn descriptor(&self) -> WorkspaceDescriptor {
            WorkspaceDescriptor {
                id: WorkspaceId::new("action-stub"),
                label: self.label,
                hotkey: '\0',
                commands: &[],
            }
        }

        fn actions(&self, area: Rect) -> Vec<WorkspaceAction> {
            vec![WorkspaceAction::new(
                "open",
                "Open child row",
                Rect::new(
                    area.x.saturating_add(1),
                    area.y.saturating_add(1),
                    area.width.saturating_sub(2),
                    1,
                ),
            )
            .preferred()]
        }

        fn activate_action(&mut self, id: &str) -> bool {
            if id != "open" {
                return false;
            }
            self.activations.lock().unwrap().push(id.to_owned());
            true
        }

        fn render(&self, frame: &mut Frame, area: Rect) {
            frame.render_widget(Paragraph::new(self.label), area);
        }
    }

    impl Workspace for FloodWorkspace {
        fn descriptor(&self) -> WorkspaceDescriptor {
            WorkspaceDescriptor {
                id: WorkspaceId::new("flood-stub"),
                label: "FLOOD",
                hotkey: '\0',
                commands: &[],
            }
        }

        fn actions(&self, area: Rect) -> Vec<WorkspaceAction> {
            (0..MAX_WORKSPACE_ACTIONS)
                .map(|index| {
                    WorkspaceAction::new(
                        format!("control:{index}"),
                        format!("Control {index}"),
                        Rect::new(area.x, area.y, 1, 1),
                    )
                })
                .collect()
        }

        fn render(&self, frame: &mut Frame, area: Rect) {
            frame.render_widget(Paragraph::new("FLOOD"), area);
        }
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
        let areas = desk_areas_with_geometry(area, DeskGeometry::default());

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

    #[test]
    fn actions_expose_only_visible_panes_and_restore_the_focused_pane() {
        let (monitor, _, _) = stub("MONITOR CHILD");
        let (chart, _, _) = stub("CHART CHILD");
        let (news, _, _) = stub("NEWS CHILD");
        let mut desk = DeskWorkspace::new(monitor, chart, news);
        let full = Rect::new(0, 0, 160, 42);

        let actions = desk.actions(full);
        assert_eq!(
            actions
                .iter()
                .take(7)
                .map(|action| &*action.id)
                .collect::<Vec<_>>(),
            [
                "pane:monitor",
                "pane:chart",
                "pane:news",
                "resize:columns:less",
                "resize:columns:more",
                "resize:rows:less",
                "resize:rows:more",
            ]
        );
        assert!(actions[0].preferred);
        assert!(desk.activate_action("pane:chart"));
        let actions = desk.actions(full);
        assert!(
            actions
                .iter()
                .find(|action| action.id == "pane:chart")
                .unwrap()
                .preferred
        );

        let short = desk.actions(Rect::new(0, 0, 160, 20));
        assert!(!short.iter().any(|action| action.id == "pane:news"));
        assert!(!short
            .iter()
            .any(|action| action.id.starts_with("resize:rows:")));
        assert!(
            short
                .iter()
                .find(|action| action.id == "pane:chart")
                .unwrap()
                .preferred
        );
    }

    #[test]
    fn commands_and_alt_arrows_resize_within_bounded_geometry() {
        let (monitor, _, _) = stub("MONITOR CHILD");
        let (chart, _, _) = stub("CHART CHILD");
        let (news, _, _) = stub("NEWS CHILD");
        let mut desk = DeskWorkspace::new(monitor, chart, news);

        desk.handle_command(&CommandInvocation {
            function: "DESK".to_owned(),
            args: vec!["LAYOUT".to_owned(), "60".to_owned(), "65".to_owned()],
        });
        assert_eq!(desk.geometry, DeskGeometry::new(60, 65).unwrap());

        desk.handle_command(&CommandInvocation {
            function: "DESK".to_owned(),
            args: vec!["LAYOUT".to_owned(), "10".to_owned(), "90".to_owned()],
        });
        assert_eq!(desk.geometry, DeskGeometry::new(60, 65).unwrap());

        desk.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::ALT));
        desk.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT));
        assert_eq!(desk.geometry, DeskGeometry::new(55, 60).unwrap());

        for _ in 0..20 {
            desk.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::ALT));
            desk.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::ALT));
        }
        assert_eq!(desk.geometry, DeskGeometry::new(70, 75).unwrap());
        assert!(!desk.activate_action("resize:columns:more"));
        assert!(!desk.activate_action("resize:rows:more"));

        desk.handle_command(&CommandInvocation {
            function: "DESK".to_owned(),
            args: vec!["RESET".to_owned()],
        });
        assert_eq!(desk.geometry, DeskGeometry::default());
    }

    #[test]
    fn resize_controls_share_mouse_focus_and_render_geometry() {
        let (monitor, _, _) = stub("MONITOR CHILD");
        let (chart, _, _) = stub("CHART CHILD");
        let (news, _, _) = stub("NEWS CHILD");
        let mut desk = DeskWorkspace::new(monitor, chart, news);
        let area = Rect::new(0, 0, 160, 42);
        let before = desk_areas_with_geometry(area, desk.geometry);
        let actions = desk.actions(area);
        let pane = actions
            .iter()
            .find(|action| action.id == "pane:monitor")
            .unwrap();
        let wider = actions
            .iter()
            .find(|action| action.id == "resize:columns:more")
            .unwrap();
        assert!(contains(pane.area, wider.area.x, wider.area.y));

        assert!(desk.handle_mouse(click(wider.area.x + 1, wider.area.y), area));
        assert_eq!(desk.geometry.monitor_percent, 50);
        let after = desk_areas_with_geometry(area, desk.geometry);
        assert!(after.monitor.frame.width > before.monitor.frame.width);
        assert_eq!(after.chart.frame.x, after.monitor.frame.right());

        assert!(desk.activate_action("resize:rows:less"));
        assert_eq!(desk.geometry.top_percent, 50);
    }

    #[test]
    fn saved_view_restores_geometry_and_degrades_invalid_percentages() {
        let (monitor, _, _) = stub("MONITOR CHILD");
        let (chart, _, _) = stub("CHART CHILD");
        let (news, _, _) = stub("NEWS CHILD");
        let mut source = DeskWorkspace::new(monitor, chart, news);
        source.geometry = DeskGeometry::new(65, 70).unwrap();
        source.select(DeskPane::Chart);
        let state = source.capture_view();

        let (monitor, _, _) = stub("MONITOR CHILD");
        let (chart, _, _) = stub("CHART CHILD");
        let (news, _, _) = stub("NEWS CHILD");
        let mut restored = DeskWorkspace::new(monitor, chart, news);
        let report = restored.restore_view(&state);
        assert_eq!(restored.geometry, source.geometry);
        assert_eq!(restored.focused, DeskPane::Chart);
        assert_eq!(report.restored_fields, 3);
        assert_eq!(report.skipped_fields, 0);

        let invalid = WorkspaceViewState::new(DESK_ID.as_str())
            .with_field("monitor_percent", ViewValue::Unsigned(10))
            .with_field("top_percent", ViewValue::Text("wide".to_owned()));
        let report = restored.restore_view(&invalid);
        assert_eq!(restored.geometry, DeskGeometry::default());
        assert_eq!(report.skipped_fields, 2);
        assert_eq!(report.warnings.len(), 2);

        let legacy = WorkspaceViewState::new(DESK_ID.as_str());
        restored.geometry = DeskGeometry::new(60, 65).unwrap();
        let report = restored.restore_view(&legacy);
        assert_eq!(restored.geometry, DeskGeometry::default());
        assert_eq!(report.skipped_fields, 0);
    }

    #[test]
    fn child_actions_are_namespaced_and_activate_the_owning_pane() {
        let (monitor, _, _) = stub("MONITOR CHILD");
        let activations = Arc::new(Mutex::new(Vec::new()));
        let chart = Box::new(ActionWorkspace {
            label: "CHART CHILD",
            activations: activations.clone(),
        });
        let (news, _, _) = stub("NEWS CHILD");
        let mut desk = DeskWorkspace::new(monitor, chart, news);
        let area = Rect::new(0, 0, 160, 42);
        let child = desk
            .actions(area)
            .into_iter()
            .find(|action| action.id == "pane:chart/open")
            .unwrap();

        assert!(contains(
            desk_areas_with_geometry(area, DeskGeometry::default())
                .chart
                .body,
            child.area.x,
            child.area.y
        ));
        assert!(!child.preferred);
        assert!(desk.activate_action("pane:chart/open"));
        assert_eq!(desk.focused, DeskPane::Chart);
        assert_eq!(*activations.lock().unwrap(), ["open"]);
    }

    #[test]
    fn pane_destinations_cannot_be_starved_by_child_action_volume() {
        let monitor = Box::new(FloodWorkspace);
        let (chart, _, _) = stub("CHART CHILD");
        let (news, _, _) = stub("NEWS CHILD");
        let desk = DeskWorkspace::new(monitor, chart, news);
        let actions = desk.actions(Rect::new(0, 0, 160, 42));

        assert_eq!(actions.len(), MAX_WORKSPACE_ACTIONS);
        assert_eq!(
            actions
                .iter()
                .take(3)
                .map(|action| &*action.id)
                .collect::<Vec<_>>(),
            ["pane:monitor", "pane:chart", "pane:news"]
        );
    }
}
