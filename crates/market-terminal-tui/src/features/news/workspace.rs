//! The expanded, scrollable story card and wrapped-height clamping adapt the
//! article-card interaction from `makeev/alphai-tui` commit
//! `9143d2e1176d0a67a9f26960427cf370187fc2e6` (MIT, Copyright (c) 2026
//! Mikhail Makeev). The on-demand article extractor is an independent Market
//! Terminal implementation; see `THIRD_PARTY_NOTICES.md`.

use std::{cell::Cell as StateCell, collections::HashSet, sync::Arc};

use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Cell, Clear, List, ListItem, Paragraph, Row, Table, Wrap},
    Frame,
};

use crate::{
    app::{
        AppIntent, CommandInvocation, ViewRestoreReport, ViewValue, Workspace, WorkspaceAction,
        WorkspaceDescriptor, WorkspaceViewState,
    },
    ui::{
        components::terminal_block,
        contains, is_primary_click, list_row_at, scroll_key,
        theme::{AMBER, BG, CYAN, INK, MUTED, YELLOW},
    },
};

use super::{
    controls::{news_areas, pack_control_areas, NewsControl},
    ArticleBodyState, NewsArticleOpener, NewsFeed, NewsFilter, NewsStory, NewsWorkbench, ID,
};

pub struct NewsWorkspace {
    query: Arc<dyn NewsFeed>,
    article_opener: Option<Arc<dyn NewsArticleOpener>>,
    article_status: String,
    selected: usize,
    filter: NewsFilter,
    read: HashSet<String>,
    bookmarks: HashSet<String>,
    show_calendar: bool,
    detail_expanded: bool,
    detail_scroll: u16,
    detail_viewport: StateCell<(u16, u16)>,
    pending_intents: Vec<AppIntent>,
}

impl NewsWorkspace {
    pub fn new(query: Arc<dyn NewsFeed>) -> Self {
        Self {
            query,
            article_opener: None,
            article_status: "ENTER READS HERE · O OPENS THE PUBLISHER".to_owned(),
            selected: 0,
            filter: NewsFilter::default(),
            read: HashSet::new(),
            bookmarks: HashSet::new(),
            show_calendar: false,
            detail_expanded: false,
            detail_scroll: 0,
            detail_viewport: StateCell::new((0, 0)),
            pending_intents: Vec::new(),
        }
    }

    pub fn with_article_opener(
        query: Arc<dyn NewsFeed>,
        article_opener: Arc<dyn NewsArticleOpener>,
    ) -> Self {
        Self {
            article_opener: Some(article_opener),
            ..Self::new(query)
        }
    }

    fn visible_indices(&self, workbench: &NewsWorkbench) -> Vec<usize> {
        workbench
            .stories
            .iter()
            .enumerate()
            .filter_map(|(index, story)| {
                self.filter
                    .matches(
                        story,
                        self.read.contains(&story.id),
                        self.bookmarks.contains(&story.id),
                    )
                    .then_some(index)
            })
            .collect()
    }

    fn selected_story<'a>(&self, workbench: &'a NewsWorkbench) -> Option<&'a NewsStory> {
        let visible = self.visible_indices(workbench);
        visible
            .get(self.selected.min(visible.len().saturating_sub(1)))
            .and_then(|index| workbench.stories.get(*index))
    }

    fn clamp_selection(&mut self) {
        let workbench = self.query.load_workbench();
        self.selected = self
            .selected
            .min(self.visible_indices(&workbench).len().saturating_sub(1));
    }

    fn select_story_identity(&mut self, workbench: &NewsWorkbench, story_id: &str) -> bool {
        let visible = self.visible_indices(workbench);
        let Some(ordinal) = visible.iter().position(|index| {
            workbench
                .stories
                .get(*index)
                .is_some_and(|story| story.id == story_id)
        }) else {
            self.selected = 0;
            self.detail_expanded = false;
            self.detail_scroll = 0;
            self.article_status = if visible.is_empty() {
                "SAVED STORY UNAVAILABLE · NO STORIES MATCH RESTORED FILTERS".to_owned()
            } else {
                "SAVED STORY UNAVAILABLE · USING FIRST VISIBLE STORY".to_owned()
            };
            return false;
        };
        self.selected = ordinal;
        true
    }

    fn control_label(&self, control: NewsControl, workbench: &NewsWorkbench) -> String {
        let selected = self.selected_story(workbench);
        match control {
            NewsControl::Reset => " 0 RESET ".to_owned(),
            NewsControl::RegionUs => " 1 US ".to_owned(),
            NewsControl::RegionEu => " 2 EU ".to_owned(),
            NewsControl::RegionAsia => " 3 AS ".to_owned(),
            NewsControl::Unread => " U UNREAD ".to_owned(),
            NewsControl::Saved => " M SAVED ".to_owned(),
            NewsControl::Calendar => {
                if self.show_calendar {
                    " E STORIES ".to_owned()
                } else {
                    " E EVENTS ".to_owned()
                }
            }
            NewsControl::ReadState => selected.map_or_else(
                || " R READ ".to_owned(),
                |story| {
                    if self.read.contains(&story.id) {
                        " R UNREAD ".to_owned()
                    } else {
                        " R READ ".to_owned()
                    }
                },
            ),
            NewsControl::Bookmark => selected.map_or_else(
                || " B SAVE ".to_owned(),
                |story| {
                    if self.bookmarks.contains(&story.id) {
                        " B UNSAVE ".to_owned()
                    } else {
                        " B SAVE ".to_owned()
                    }
                },
            ),
            NewsControl::Security => " S SEC ".to_owned(),
            NewsControl::InsertSheet => " A SHEET ".to_owned(),
            NewsControl::Refresh => " F9 REFRESH ".to_owned(),
        }
    }

    fn control_areas(&self, area: Rect, workbench: &NewsWorkbench) -> Vec<(NewsControl, Rect)> {
        pack_control_areas(
            area,
            NewsControl::ALL.into_iter().map(|control| {
                let width = self.control_label(control, workbench).chars().count() as u16;
                (control, width)
            }),
        )
    }

    fn control_enabled(&self, control: NewsControl, workbench: &NewsWorkbench) -> bool {
        let selected = self.selected_story(workbench);
        match control {
            NewsControl::Reset => self.filter.is_active() || self.show_calendar,
            NewsControl::ReadState | NewsControl::Bookmark => selected.is_some(),
            NewsControl::Security | NewsControl::InsertSheet => {
                selected.is_some_and(|story| !story.related_symbols.is_empty())
            }
            _ => true,
        }
    }

    fn control_active(&self, control: NewsControl, workbench: &NewsWorkbench) -> bool {
        match control {
            NewsControl::RegionUs => self.filter.region.as_deref() == Some("US"),
            NewsControl::RegionEu => self.filter.region.as_deref() == Some("EU"),
            NewsControl::RegionAsia => self.filter.region.as_deref() == Some("AS"),
            NewsControl::Unread => self.filter.unread_only,
            NewsControl::Saved => self.filter.bookmarked_only,
            NewsControl::Calendar => self.show_calendar,
            NewsControl::ReadState => self
                .selected_story(workbench)
                .is_some_and(|story| self.read.contains(&story.id)),
            NewsControl::Bookmark => self
                .selected_story(workbench)
                .is_some_and(|story| self.bookmarks.contains(&story.id)),
            NewsControl::Reset
            | NewsControl::Security
            | NewsControl::InsertSheet
            | NewsControl::Refresh => false,
        }
    }

    fn control_action_id(&self, control: NewsControl, workbench: &NewsWorkbench) -> String {
        if control.is_story_specific() {
            let identity = self
                .selected_story(workbench)
                .map_or(0, |story| story_identity(&story.id));
            format!("control:{}:{identity:016x}", control.key())
        } else {
            format!("control:{}", control.key())
        }
    }

    fn control_action_label(&self, control: NewsControl, workbench: &NewsWorkbench) -> String {
        let selected = self.selected_story(workbench);
        match control {
            NewsControl::Reset => "Reset all news filters".to_owned(),
            NewsControl::RegionUs => "Show United States news".to_owned(),
            NewsControl::RegionEu => "Show European news".to_owned(),
            NewsControl::RegionAsia => "Show Asian news".to_owned(),
            NewsControl::Unread => "Toggle unread-only news".to_owned(),
            NewsControl::Saved => "Toggle saved-only news".to_owned(),
            NewsControl::Calendar => {
                if self.show_calendar {
                    "Return to the selected story".to_owned()
                } else {
                    "Open the economic event calendar".to_owned()
                }
            }
            NewsControl::ReadState => selected.map_or_else(
                || "Toggle selected story read state".to_owned(),
                |story| format!("Toggle read state for {}", story.headline.title),
            ),
            NewsControl::Bookmark => selected.map_or_else(
                || "Toggle selected story bookmark".to_owned(),
                |story| format!("Toggle bookmark for {}", story.headline.title),
            ),
            NewsControl::Security => selected.map_or_else(
                || "Open linked security research".to_owned(),
                |story| {
                    format!(
                        "Open {} security research",
                        story
                            .related_symbols
                            .first()
                            .map_or("linked", String::as_str)
                    )
                },
            ),
            NewsControl::InsertSheet => selected.map_or_else(
                || "Insert linked security into Spreadsheet".to_owned(),
                |story| {
                    format!(
                        "Insert {} into Spreadsheet",
                        story
                            .related_symbols
                            .first()
                            .map_or("linked", String::as_str)
                    )
                },
            ),
            NewsControl::Refresh => "Refresh the live news feed".to_owned(),
        }
    }

    fn activate_control(
        &mut self,
        control: NewsControl,
        expected_identity: Option<u64>,
        workbench: &NewsWorkbench,
    ) -> bool {
        if !self.control_enabled(control, workbench) {
            return false;
        }
        let selected = self.selected_story(workbench);
        if control.is_story_specific()
            && expected_identity != selected.map(|story| story_identity(&story.id))
        {
            return false;
        }
        match control {
            NewsControl::Reset => {
                self.filter = NewsFilter::default();
                self.show_calendar = false;
                self.selected = 0;
            }
            NewsControl::RegionUs => {
                self.filter.region = Some("US".to_owned());
                self.selected = 0;
            }
            NewsControl::RegionEu => {
                self.filter.region = Some("EU".to_owned());
                self.selected = 0;
            }
            NewsControl::RegionAsia => {
                self.filter.region = Some("AS".to_owned());
                self.selected = 0;
            }
            NewsControl::Unread => {
                self.filter.unread_only = !self.filter.unread_only;
                self.selected = 0;
            }
            NewsControl::Saved => {
                self.filter.bookmarked_only = !self.filter.bookmarked_only;
                self.selected = 0;
            }
            NewsControl::Calendar => self.show_calendar = !self.show_calendar,
            NewsControl::ReadState => {
                let story = selected.expect("enabled story control has a selected story");
                if !self.read.insert(story.id.clone()) {
                    self.read.remove(&story.id);
                }
            }
            NewsControl::Bookmark => {
                let story = selected.expect("enabled story control has a selected story");
                if !self.bookmarks.insert(story.id.clone()) {
                    self.bookmarks.remove(&story.id);
                }
            }
            NewsControl::Security => {
                let symbol = selected
                    .and_then(|story| story.related_symbols.first())
                    .expect("enabled security control has a linked symbol");
                self.pending_intents.push(AppIntent::DispatchCommand {
                    command: format!("SEC {symbol} US"),
                    origin: ID,
                });
            }
            NewsControl::InsertSheet => {
                let symbol = selected
                    .and_then(|story| story.related_symbols.first())
                    .expect("enabled sheet control has a linked symbol");
                self.pending_intents.push(AppIntent::DispatchCommand {
                    command: format!("SHEET INSERT {symbol} US"),
                    origin: ID,
                });
            }
            NewsControl::Refresh => self.query.request_refresh(),
        }
        self.clamp_selection();
        true
    }

    fn activate_current_control(
        &mut self,
        control: NewsControl,
        workbench: &NewsWorkbench,
    ) -> bool {
        let identity = control.is_story_specific().then(|| {
            self.selected_story(workbench)
                .map_or(0, |story| story_identity(&story.id))
        });
        self.activate_control(control, identity, workbench)
    }

    fn header_actions(&self, area: Rect, workbench: &NewsWorkbench) -> Vec<WorkspaceAction> {
        let areas = news_areas(area);
        self.control_areas(areas.controls, workbench)
            .into_iter()
            .map(|(control, control_area)| {
                let mut action = WorkspaceAction::new(
                    self.control_action_id(control, workbench),
                    self.control_action_label(control, workbench),
                    control_area,
                );
                if !self.control_enabled(control, workbench) {
                    action = action.disabled();
                }
                if control == NewsControl::Calendar && self.show_calendar {
                    action = action.preferred();
                }
                action
            })
            .collect()
    }

    fn set_option(&mut self, option: &str) {
        let Some((name, value)) = option
            .strip_prefix("--")
            .and_then(|value| value.split_once('='))
        else {
            match option {
                "--unread" => self.filter.unread_only = true,
                "--bookmarked" => self.filter.bookmarked_only = true,
                "--events" => self.show_calendar = true,
                _ => {}
            }
            return;
        };
        match name.to_ascii_lowercase().as_str() {
            "region" => self.filter.region = Some(value.to_ascii_uppercase()),
            "topic" => self.filter.topic = Some(value.to_ascii_uppercase()),
            "symbol" => self.filter.symbol = Some(value.to_ascii_uppercase()),
            _ => {}
        }
    }

    fn open_selected_article(&mut self, workbench: &NewsWorkbench) {
        let Some(url) = self
            .selected_story(workbench)
            .and_then(|story| story.url.clone())
        else {
            self.article_status = "NO PUBLISHER LINK FOR THIS STORY".to_owned();
            return;
        };
        let Some(opener) = &self.article_opener else {
            self.article_status = "ARTICLE OPENING IS NOT AVAILABLE IN DEMO MODE".to_owned();
            return;
        };
        self.article_status = match opener.open(&url) {
            Ok(()) => "OPENED PUBLISHER ARTICLE IN YOUR BROWSER".to_owned(),
            Err(error) => format!("OPEN FAILED · {error}"),
        };
    }

    fn toggle_expanded_detail(&mut self) {
        self.detail_expanded = !self.detail_expanded;
        self.detail_scroll = 0;
    }

    fn open_selected_reader(&mut self, workbench: &NewsWorkbench) {
        let Some(story) = self.selected_story(workbench) else {
            self.article_status = "NO STORY IS SELECTED".to_owned();
            return;
        };
        let story_id = story.id.clone();
        let url = story.url.clone();
        let should_fetch = matches!(
            story.body_state,
            ArticleBodyState::ExcerptOnly | ArticleBodyState::Unavailable(_)
        );
        self.read.insert(story_id.clone());
        self.detail_expanded = true;
        self.detail_scroll = 0;
        self.article_status = "READER OPEN".to_owned();
        if should_fetch {
            self.article_status = match url {
                Some(url) if self.query.request_article(&story_id, &url) => {
                    "FETCHING ARTICLE TEXT IN THE BACKGROUND".to_owned()
                }
                Some(_) => "THIS NEWS ADAPTER CANNOT FETCH ARTICLE TEXT".to_owned(),
                None => "NO PUBLISHER LINK FOR THIS STORY".to_owned(),
            };
        }
    }

    fn scroll_expanded_detail(&mut self, workbench: &NewsWorkbench, direction: isize) {
        let Some(story) = self.selected_story(workbench) else {
            self.detail_scroll = 0;
            return;
        };
        let (width, height) = self.detail_viewport.get();
        let maximum =
            wrapped_height(&story_detail_lines(story, height < 28), width).saturating_sub(height);
        self.detail_scroll = if direction.is_negative() {
            self.detail_scroll
                .saturating_sub(direction.unsigned_abs() as u16)
        } else {
            self.detail_scroll
                .saturating_add(direction as u16)
                .min(maximum)
        };
    }
}

impl Workspace for NewsWorkspace {
    fn descriptor(&self) -> WorkspaceDescriptor {
        WorkspaceDescriptor {
            id: ID,
            label: "NEWS",
            hotkey: 'n',
            commands: &["NEWS", "TOP", "HEADLINES"],
        }
    }

    fn handle_command(&mut self, invocation: &CommandInvocation) -> bool {
        if matches!(invocation.function.as_str(), "NEWS" | "TOP" | "HEADLINES") {
            self.filter = NewsFilter::default();
            self.show_calendar = false;
        }
        let mut subject_seen = false;
        for argument in &invocation.args {
            if argument.starts_with("--") {
                self.set_option(argument);
            } else if !subject_seen {
                let subject = argument.to_ascii_uppercase();
                if matches!(
                    subject.as_str(),
                    "TOP" | "FED" | "ECO" | "POL" | "CMD" | "TEC"
                ) {
                    self.filter.topic = Some(subject);
                } else {
                    self.filter.symbol = Some(subject);
                }
                subject_seen = true;
            }
        }
        self.selected = 0;
        self.clamp_selection();
        true
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        let workbench = self.query.load_workbench();
        if self.detail_expanded {
            match key.code {
                KeyCode::Esc | KeyCode::Char('v' | 'V') => self.toggle_expanded_detail(),
                KeyCode::Up | KeyCode::Char('k') => self.scroll_expanded_detail(&workbench, -1),
                KeyCode::Down | KeyCode::Char('j') => self.scroll_expanded_detail(&workbench, 1),
                KeyCode::PageUp | KeyCode::Char('u') => self.scroll_expanded_detail(&workbench, -5),
                KeyCode::PageDown | KeyCode::Enter | KeyCode::Char(' ' | 'd') => {
                    self.scroll_expanded_detail(&workbench, 5)
                }
                KeyCode::Home | KeyCode::Char('g') => self.detail_scroll = 0,
                KeyCode::End | KeyCode::Char('G') => {
                    self.scroll_expanded_detail(&workbench, isize::MAX)
                }
                KeyCode::Char('o' | 'O') => self.open_selected_article(&workbench),
                KeyCode::Char('r' | 'R') => self.open_selected_reader(&workbench),
                _ => return false,
            }
            return true;
        }
        let length = self.visible_indices(&workbench).len();
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                self.selected = (self.selected + 1).min(length.saturating_sub(1));
                self.detail_scroll = 0;
                self.article_status = "ENTER READS HERE · O OPENS THE PUBLISHER".to_owned();
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected = self.selected.saturating_sub(1);
                self.detail_scroll = 0;
                self.article_status = "ENTER READS HERE · O OPENS THE PUBLISHER".to_owned();
            }
            KeyCode::Enter | KeyCode::Char('v' | 'V') => self.open_selected_reader(&workbench),
            KeyCode::Char('o' | 'O') => self.open_selected_article(&workbench),
            KeyCode::Char('r') => {
                self.activate_current_control(NewsControl::ReadState, &workbench);
            }
            KeyCode::Char('b') => {
                self.activate_current_control(NewsControl::Bookmark, &workbench);
            }
            KeyCode::Char('u') => {
                self.activate_current_control(NewsControl::Unread, &workbench);
            }
            KeyCode::Char('m') => {
                self.activate_current_control(NewsControl::Saved, &workbench);
            }
            KeyCode::Char('e') => {
                self.activate_current_control(NewsControl::Calendar, &workbench);
            }
            KeyCode::F(9) => {
                self.activate_current_control(NewsControl::Refresh, &workbench);
            }
            KeyCode::Char('0') => {
                self.activate_current_control(NewsControl::Reset, &workbench);
            }
            KeyCode::Char('1') => {
                self.activate_current_control(NewsControl::RegionUs, &workbench);
            }
            KeyCode::Char('2') => {
                self.activate_current_control(NewsControl::RegionEu, &workbench);
            }
            KeyCode::Char('3') => {
                self.activate_current_control(NewsControl::RegionAsia, &workbench);
            }
            KeyCode::Char('s') => {
                self.activate_current_control(NewsControl::Security, &workbench);
            }
            KeyCode::Char('a') => {
                self.activate_current_control(NewsControl::InsertSheet, &workbench);
            }
            _ => return false,
        }
        self.clamp_selection();
        true
    }

    fn handle_mouse(&mut self, event: MouseEvent, area: Rect) -> bool {
        let workbench = self.query.load_workbench();
        if self.detail_expanded {
            match event.kind {
                MouseEventKind::ScrollUp => self.scroll_expanded_detail(&workbench, -3),
                MouseEventKind::ScrollDown => self.scroll_expanded_detail(&workbench, 3),
                MouseEventKind::Down(MouseButton::Left)
                    if crate::ui::contains(expanded_close_area(area), event.column, event.row) =>
                {
                    self.toggle_expanded_detail();
                }
                MouseEventKind::Down(MouseButton::Left)
                    if crate::ui::contains(expanded_open_area(area), event.column, event.row) =>
                {
                    self.open_selected_article(&workbench);
                }
                _ => {}
            }
            return true;
        }
        let visible_count = self.visible_indices(&workbench).len();
        let areas = news_areas(area);
        if is_primary_click(event, areas.header) {
            for (control, control_area) in self.control_areas(areas.controls, &workbench) {
                if contains(control_area, event.column, event.row) {
                    if self.control_enabled(control, &workbench) {
                        self.activate_current_control(control, &workbench);
                    }
                    return true;
                }
            }
            return true;
        }
        if areas.detail.is_none() {
            if self.show_calendar {
                return contains(areas.body, event.column, event.row);
            }
            if let Some(index) = list_row_at(event, areas.stories, visible_count) {
                self.selected = index;
                self.detail_scroll = 0;
                self.article_status = "ENTER READS HERE · O OPENS THE PUBLISHER".to_owned();
                return true;
            }
            if let Some(key) = scroll_key(event, areas.body) {
                return self.handle_key(key);
            }
            return false;
        }
        let detail = areas.detail.expect("wide news layout has detail area");
        let events = areas.events.expect("wide news layout has events area");
        if !self.show_calendar && is_primary_click(event, story_open_area(detail)) {
            self.open_selected_reader(&workbench);
            return true;
        }
        if !self.show_calendar && is_primary_click(event, story_expand_area(detail)) {
            self.open_selected_article(&workbench);
            return true;
        }
        if let Some(index) = list_row_at(event, areas.stories, visible_count) {
            self.selected = index;
            self.detail_scroll = 0;
            self.article_status = "ENTER READS HERE · O OPENS THE PUBLISHER".to_owned();
            return true;
        }
        if is_primary_click(event, events) {
            self.show_calendar = true;
            return true;
        }
        if let Some(key) = scroll_key(event, areas.body) {
            return self.handle_key(key);
        }
        false
    }

    fn actions(&self, area: Rect) -> Vec<WorkspaceAction> {
        let workbench = self.query.load_workbench();
        if self.detail_expanded {
            let Some(selected) = self.selected_story(&workbench) else {
                return vec![WorkspaceAction::new(
                    "modal:close:missing",
                    "Close unavailable news reader",
                    expanded_close_area(area),
                )
                .preferred()];
            };
            let selected_identity = story_identity(&selected.id);
            let mut actions = vec![WorkspaceAction::new(
                format!("modal:close:{selected_identity:016x}"),
                format!("Close reader for {}", short_title(&selected.headline.title)),
                expanded_close_area(area),
            )
            .preferred()];
            let mut open = WorkspaceAction::new(
                format!("modal:web:{selected_identity:016x}"),
                format!(
                    "Open publisher page for {}",
                    short_title(&selected.headline.title)
                ),
                expanded_open_area(area),
            );
            if selected.url.is_none() {
                open = open.disabled();
            }
            actions.push(open);
            return actions;
        }
        let Some(selected) = self.selected_story(&workbench) else {
            return self.header_actions(area, &workbench);
        };
        let selected_identity = story_identity(&selected.id);

        let areas = news_areas(area);
        let visible = self.visible_indices(&workbench);
        let visible_rows = usize::from(areas.stories.height.saturating_sub(2)).min(visible.len());
        let mut actions = self.header_actions(area, &workbench);
        if !self.show_calendar {
            actions.extend(visible.iter().take(visible_rows).enumerate().filter_map(
                |(ordinal, index)| {
                    let story = workbench.stories.get(*index)?;
                    let identity = story_identity(&story.id);
                    let action = WorkspaceAction::new(
                        format!("story:{ordinal}:{identity:016x}"),
                        format!("Read {}", short_title(&story.headline.title)),
                        news_story_row_area(areas.stories, ordinal)?,
                    );
                    Some(if ordinal == self.selected {
                        action.preferred()
                    } else {
                        action
                    })
                },
            ));
        }
        if let Some(detail) = areas.detail.filter(|_| !self.show_calendar) {
            actions.push(WorkspaceAction::new(
                format!("story-read:{selected_identity:016x}"),
                format!("Read {} in terminal", short_title(&selected.headline.title)),
                story_open_area(detail),
            ));
            let mut web = WorkspaceAction::new(
                format!("story-web:{selected_identity:016x}"),
                format!(
                    "Open publisher page for {}",
                    short_title(&selected.headline.title)
                ),
                story_expand_area(detail),
            );
            if selected.url.is_none() {
                web = web.disabled();
            }
            actions.push(web);
        }
        if let Some(events) = areas.events.filter(|_| !self.show_calendar) {
            actions.push(WorkspaceAction::new(
                "view:events",
                "Open the economic event calendar",
                events,
            ));
        }
        actions
    }

    fn activate_action(&mut self, id: &str) -> bool {
        let workbench = self.query.load_workbench();
        if let Some(identity) = id.strip_prefix("modal:close:") {
            let missing = identity == "missing" && self.selected_story(&workbench).is_none();
            if !self.detail_expanded
                || (!missing && !selected_identity_matches(self, &workbench, identity))
            {
                return false;
            }
            self.toggle_expanded_detail();
            return true;
        }
        if let Some(identity) = id.strip_prefix("modal:web:") {
            if !self.detail_expanded
                || !selected_identity_matches(self, &workbench, identity)
                || self
                    .selected_story(&workbench)
                    .is_none_or(|story| story.url.is_none())
            {
                return false;
            }
            self.open_selected_article(&workbench);
            return true;
        }
        if self.detail_expanded {
            return false;
        }
        if id == "view:events" {
            self.show_calendar = true;
            return true;
        }
        if let Some(identity) = id.strip_prefix("story-read:") {
            if self.show_calendar || !selected_identity_matches(self, &workbench, identity) {
                return false;
            }
            self.open_selected_reader(&workbench);
            return true;
        }
        if let Some(identity) = id.strip_prefix("story-web:") {
            if self.show_calendar
                || !selected_identity_matches(self, &workbench, identity)
                || self
                    .selected_story(&workbench)
                    .is_none_or(|story| story.url.is_none())
            {
                return false;
            }
            self.open_selected_article(&workbench);
            return true;
        }
        if let Some(row) = id.strip_prefix("story:") {
            let Some((ordinal, identity)) = row.split_once(':') else {
                return false;
            };
            let Ok(ordinal) = ordinal.parse::<usize>() else {
                return false;
            };
            let visible = self.visible_indices(&workbench);
            let Some(story) = visible
                .get(ordinal)
                .and_then(|index| workbench.stories.get(*index))
            else {
                return false;
            };
            if !identity_matches(&story.id, identity) {
                return false;
            }
            self.selected = ordinal;
            self.open_selected_reader(&workbench);
            return true;
        }
        let Some(control_id) = id.strip_prefix("control:") else {
            return false;
        };
        let (key, identity) = control_id
            .split_once(':')
            .map_or((control_id, None), |(key, identity)| (key, Some(identity)));
        let Some(control) = NewsControl::from_key(key) else {
            return false;
        };
        if control.is_story_specific() != identity.is_some() {
            return false;
        }
        let expected_identity = match identity {
            Some(identity) => {
                let Some(identity) = parse_identity(identity) else {
                    return false;
                };
                Some(identity)
            }
            None => None,
        };
        self.activate_control(control, expected_identity, &workbench)
    }

    fn is_modal_active(&self) -> bool {
        self.detail_expanded
    }

    fn poll_intents(&mut self) -> Vec<AppIntent> {
        std::mem::take(&mut self.pending_intents)
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let workbench = self.query.load_workbench();
        if self.detail_expanded {
            if let Some(story) = self.selected_story(&workbench) {
                render_expanded_story(
                    frame,
                    area,
                    story,
                    &self.article_status,
                    self.detail_scroll,
                    &self.detail_viewport,
                );
                return;
            }
            render_missing_expanded_story(frame, area);
            return;
        }
        let visible = self.visible_indices(&workbench);
        let unread = workbench
            .stories
            .iter()
            .filter(|story| !self.read.contains(&story.id))
            .count();
        let feed_status = self.query.status();
        let areas = news_areas(area);
        let filter_label = format!(
            "REGION {}  TOPIC {}  SYMBOL {}  {}{}",
            self.filter.region.as_deref().unwrap_or("ALL"),
            self.filter.topic.as_deref().unwrap_or("ALL"),
            self.filter.symbol.as_deref().unwrap_or("ALL"),
            if self.filter.unread_only {
                "UNREAD  "
            } else {
                ""
            },
            if self.filter.bookmarked_only {
                "BOOKMARKED"
            } else {
                ""
            },
        );
        frame.render_widget(terminal_block("NEWS", "FILTERS & WORKFLOW"), areas.header);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    format!(" {} RESULTS  {unread} UNREAD  ", visible.len()),
                    Style::new().bg(AMBER.into()).fg(BG.into()).bold(),
                ),
                Span::styled(filter_label, INK),
                Span::styled("  ", MUTED),
                Span::styled(feed_status, YELLOW),
            ])),
            areas.summary,
        );
        for (control, control_area) in self.control_areas(areas.controls, &workbench) {
            let style = if !self.control_enabled(control, &workbench) {
                Style::new().fg(MUTED.into())
            } else if self.control_active(control, &workbench) {
                Style::new().bg(CYAN.into()).fg(BG.into()).bold()
            } else {
                Style::new().fg(AMBER.into())
            };
            frame.render_widget(
                Paragraph::new(self.control_label(control, &workbench)).style(style),
                control_area,
            );
        }
        let items = visible
            .iter()
            .enumerate()
            .filter_map(|(ordinal, index)| {
                let story = workbench.stories.get(*index)?;
                let selected = ordinal == self.selected.min(visible.len().saturating_sub(1));
                let status = if self.bookmarks.contains(&story.id) {
                    "★"
                } else if self.read.contains(&story.id) {
                    " "
                } else {
                    "●"
                };
                Some(ListItem::new(Line::from(vec![
                    Span::styled(format!("{status} {} ", story.headline.time), MUTED),
                    Span::styled(format!("{:<4}", story.headline.topic), AMBER),
                    Span::styled(
                        story.headline.title.as_str(),
                        if selected {
                            Style::new().bg(CYAN.into()).fg(BG.into())
                        } else {
                            Style::new().fg(INK.into())
                        },
                    ),
                    Span::styled(format!(" {}", story.headline.region), MUTED),
                ])))
            })
            .collect::<Vec<_>>();
        let stories = List::new(items).block(terminal_block("TOP", "TOP NEWS"));
        if areas.detail.is_none() {
            if self.show_calendar {
                render_calendar(frame, areas.body, &workbench);
            } else {
                frame.render_widget(stories, areas.stories);
            }
            return;
        }
        frame.render_widget(stories, areas.stories);

        let detail = areas.detail.expect("wide news layout has detail area");
        let events = areas.events.expect("wide news layout has events area");

        if self.show_calendar {
            render_calendar(frame, detail, &workbench);
        } else if let Some(story) = self.selected_story(&workbench) {
            render_story(frame, detail, story, &self.article_status);
        } else {
            frame.render_widget(
                Paragraph::new("NO STORIES MATCH THE ACTIVE FILTER\n\nPRESS 0 TO RESET")
                    .style(MUTED)
                    .block(terminal_block("READ", "STORY")),
                detail,
            );
        }

        let mut upcoming = workbench
            .events
            .iter()
            .take(6)
            .map(|event| {
                Line::from(vec![
                    Span::styled(format!("{} ", event.time), AMBER),
                    Span::styled(format!("{} ", event.region), CYAN),
                    Span::styled(event.event.as_str(), INK),
                ])
            })
            .collect::<Vec<_>>();
        if upcoming.is_empty() {
            upcoming.push(Line::styled("NO LIVE CALENDAR SOURCE", MUTED));
        }
        frame.render_widget(
            Paragraph::new(upcoming)
                .wrap(Wrap { trim: true })
                .block(terminal_block("ECO", "EVENTS · E EXPAND")),
            events,
        );
    }

    fn capture_view(&self) -> WorkspaceViewState {
        let workbench = self.query.load_workbench();
        let mut state = WorkspaceViewState::new(ID.as_str())
            .with_field("unread_only", ViewValue::Boolean(self.filter.unread_only))
            .with_field(
                "bookmarked_only",
                ViewValue::Boolean(self.filter.bookmarked_only),
            )
            .with_field(
                "subview",
                ViewValue::Text(if self.show_calendar {
                    "events".to_owned()
                } else {
                    "stories".to_owned()
                }),
            );
        for (name, value) in [
            ("region", self.filter.region.as_ref()),
            ("topic", self.filter.topic.as_ref()),
            ("symbol", self.filter.symbol.as_ref()),
        ] {
            if let Some(value) = value {
                state = state.with_field(name, ViewValue::Text(value.clone()));
            }
        }
        if let Some(story) = self.selected_story(&workbench) {
            state = state.with_field("selected_story_id", ViewValue::Text(story.id.clone()));
        }
        state
    }

    fn restore_view(&mut self, state: &WorkspaceViewState) -> ViewRestoreReport {
        if !state.workspace.eq_ignore_ascii_case(ID.as_str()) {
            return ViewRestoreReport::warning(format!(
                "saved state belongs to {}, not news",
                state.workspace
            ));
        }

        self.filter = NewsFilter::default();
        self.show_calendar = false;
        self.selected = 0;
        self.detail_expanded = false;
        self.detail_scroll = 0;
        self.article_status = "ENTER READS HERE · O OPENS THE PUBLISHER".to_owned();

        let mut report = ViewRestoreReport::default();
        restore_filter_field(state, "region", 8, &mut self.filter.region, &mut report);
        restore_filter_field(state, "topic", 16, &mut self.filter.topic, &mut report);
        restore_filter_field(state, "symbol", 32, &mut self.filter.symbol, &mut report);
        restore_boolean_field(
            state,
            "unread_only",
            &mut self.filter.unread_only,
            &mut report,
        );
        restore_boolean_field(
            state,
            "bookmarked_only",
            &mut self.filter.bookmarked_only,
            &mut report,
        );
        if let Some(value) = state.fields.get("subview") {
            match value.as_text() {
                Some("stories") => report.restored_fields += 1,
                Some("events") => {
                    self.show_calendar = true;
                    report.restored_fields += 1;
                }
                _ => {
                    report.skipped_fields += 1;
                    report
                        .warnings
                        .push("news subview is unavailable".to_owned());
                }
            }
        }

        let workbench = self.query.load_workbench();
        if let Some(value) = state.fields.get("selected_story_id") {
            match value.as_text().filter(|value| valid_story_id(value)) {
                Some(story_id) if self.select_story_identity(&workbench, story_id) => {
                    report.restored_fields += 1;
                }
                Some(_) => {
                    report.skipped_fields += 1;
                    report
                        .warnings
                        .push("saved news story is no longer available".to_owned());
                }
                None => {
                    report.skipped_fields += 1;
                    report
                        .warnings
                        .push("saved news story identity is invalid".to_owned());
                }
            }
        } else {
            self.clamp_selection();
        }

        const KNOWN_FIELDS: [&str; 7] = [
            "region",
            "topic",
            "symbol",
            "unread_only",
            "bookmarked_only",
            "subview",
            "selected_story_id",
        ];
        let unknown = state
            .fields
            .keys()
            .filter(|field| !KNOWN_FIELDS.contains(&field.as_str()))
            .count();
        if unknown > 0 {
            report.skipped_fields += unknown;
            report
                .warnings
                .push(format!("ignored {unknown} future news field(s)"));
        }
        if !state.children.is_empty() {
            report.skipped_fields += state.children.len();
            report.warnings.push(format!(
                "ignored {} future news child state(s)",
                state.children.len()
            ));
        }
        report
    }
}

fn restore_filter_field(
    state: &WorkspaceViewState,
    name: &str,
    maximum_bytes: usize,
    target: &mut Option<String>,
    report: &mut ViewRestoreReport,
) {
    let Some(value) = state.fields.get(name) else {
        return;
    };
    match value
        .as_text()
        .filter(|value| valid_filter_token(value, maximum_bytes))
    {
        Some(value) => {
            *target = Some(value.to_owned());
            report.restored_fields += 1;
        }
        None => {
            report.skipped_fields += 1;
            report
                .warnings
                .push(format!("news {name} filter is invalid"));
        }
    }
}

fn restore_boolean_field(
    state: &WorkspaceViewState,
    name: &str,
    target: &mut bool,
    report: &mut ViewRestoreReport,
) {
    let Some(value) = state.fields.get(name) else {
        return;
    };
    match value.as_boolean() {
        Some(value) => {
            *target = value;
            report.restored_fields += 1;
        }
        None => {
            report.skipped_fields += 1;
            report.warnings.push(format!("news {name} flag is invalid"));
        }
    }
}

fn valid_filter_token(value: &str, maximum_bytes: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum_bytes
        && value.bytes().all(|byte| {
            byte.is_ascii_uppercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'-' | b'.' | b'_' | b'^' | b'/')
        })
}

fn valid_story_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

fn story_identity(id: &str) -> u64 {
    id.as_bytes().iter().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

fn parse_identity(identity: &str) -> Option<u64> {
    (identity.len() == 16)
        .then(|| u64::from_str_radix(identity, 16).ok())
        .flatten()
}

fn identity_matches(story_id: &str, encoded: &str) -> bool {
    parse_identity(encoded) == Some(story_identity(story_id))
}

fn selected_identity_matches(
    workspace: &NewsWorkspace,
    workbench: &NewsWorkbench,
    encoded: &str,
) -> bool {
    workspace
        .selected_story(workbench)
        .is_some_and(|story| identity_matches(&story.id, encoded))
}

fn short_title(title: &str) -> String {
    const LIMIT: usize = 96;
    let mut characters = title.chars();
    let shortened = characters.by_ref().take(LIMIT).collect::<String>();
    if characters.next().is_some() {
        format!("{shortened}…")
    } else {
        shortened
    }
}

fn news_story_row_area(area: Rect, ordinal: usize) -> Option<Rect> {
    let y = area.y.saturating_add(1 + u16::try_from(ordinal).ok()?);
    (y < area.bottom().saturating_sub(1))
        .then(|| Rect::new(area.x.saturating_add(1), y, area.width.saturating_sub(2), 1))
}

fn story_open_area(area: Rect) -> Rect {
    let action = Layout::vertical([Constraint::Min(6), Constraint::Length(4)]).split(area)[1];
    Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).split(action)[0]
}

fn story_expand_area(area: Rect) -> Rect {
    let action = Layout::vertical([Constraint::Min(6), Constraint::Length(4)]).split(area)[1];
    Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).split(action)[1]
}

fn render_story(frame: &mut Frame, area: Rect, story: &NewsStory, article_status: &str) {
    let rows = Layout::vertical([Constraint::Min(6), Constraint::Length(4)]).split(area);
    let mut lines = vec![Line::styled(
        format!(
            "{} · {} · {}",
            story.headline.topic,
            story.headline.time,
            story.provenance.freshness.label()
        ),
        AMBER,
    )];
    lines.extend(story_provenance_lines(story));
    lines.extend(story_sentiment_lines(story, area.height < 28));
    lines.extend([
        Line::styled(format!("BY {}", story.byline), MUTED),
        Line::raw(""),
        Line::styled(
            story.headline.title.as_str(),
            Style::new().fg(INK.into()).bold(),
        ),
        Line::raw(""),
        Line::styled(story.summary.as_str(), YELLOW),
        Line::raw(""),
    ]);
    for paragraph in &story.body {
        lines.push(Line::raw(paragraph.as_str()));
        lines.push(Line::raw(""));
    }
    lines.push(Line::styled(
        format!("RELATED  {}", story.related_symbols.join("  ")),
        CYAN,
    ));
    if let Some(url) = &story.url {
        lines.push(Line::styled(format!("SOURCE   {url}"), MUTED));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(terminal_block("READ", "STORY DETAIL")),
        rows[0],
    );
    let action = if story.url.is_some() {
        vec![
            Line::from(vec![
                Span::styled(
                    " [ READ HERE · ENTER / V ] ",
                    Style::new().bg(AMBER.into()).fg(BG.into()).bold(),
                ),
                Span::styled(
                    " [ OPEN WEB · O ] ",
                    Style::new().bg(CYAN.into()).fg(BG.into()).bold(),
                ),
            ]),
            Line::styled(article_footer_status(story, article_status), MUTED),
        ]
    } else {
        vec![
            Line::from(vec![
                Span::styled(" NO PUBLISHER LINK AVAILABLE ", MUTED),
                Span::styled(
                    " [ READ HERE · ENTER / V ] ",
                    Style::new().bg(CYAN.into()).fg(BG.into()).bold(),
                ),
            ]),
            Line::styled(article_footer_status(story, article_status), MUTED),
        ]
    };
    frame.render_widget(
        Paragraph::new(action).block(terminal_block("WEB", "PUBLISHER SOURCE")),
        rows[1],
    );
}

fn expanded_panel_area(area: Rect) -> Rect {
    let horizontal = u16::from(area.width >= 60);
    let vertical = u16::from(area.height >= 16);
    Rect::new(
        area.x.saturating_add(horizontal),
        area.y.saturating_add(vertical),
        area.width.saturating_sub(horizontal * 2),
        area.height.saturating_sub(vertical * 2),
    )
}

fn expanded_close_area(area: Rect) -> Rect {
    let panel = expanded_panel_area(area);
    Rect::new(
        panel.x.saturating_add(panel.width.saturating_sub(15)),
        panel.y,
        14.min(panel.width),
        1.min(panel.height),
    )
}

fn expanded_open_area(area: Rect) -> Rect {
    let panel = expanded_panel_area(area);
    Rect::new(
        panel.x.saturating_add(2),
        panel.y.saturating_add(panel.height.saturating_sub(3)),
        31.min(panel.width.saturating_sub(2)),
        1.min(panel.height),
    )
}

fn render_missing_expanded_story(frame: &mut Frame, area: Rect) {
    let panel = expanded_panel_area(area);
    frame.render_widget(Clear, panel);
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled("THE SELECTED STORY IS NO LONGER IN THE LIVE FEED", YELLOW),
            Line::raw(""),
            Line::styled(
                "Close the reader to return to the refreshed headline list.",
                MUTED,
            ),
        ])
        .wrap(Wrap { trim: true })
        .block(terminal_block("READ", "STORY UNAVAILABLE")),
        panel,
    );
    frame.render_widget(
        Paragraph::new(" [ CLOSE · V / ESC ] ")
            .style(Style::new().bg(CYAN.into()).fg(BG.into()).bold()),
        expanded_close_area(area),
    );
}

fn render_expanded_story(
    frame: &mut Frame,
    area: Rect,
    story: &NewsStory,
    article_status: &str,
    scroll: u16,
    viewport: &StateCell<(u16, u16)>,
) {
    let panel = expanded_panel_area(area);
    frame.render_widget(Clear, panel);
    let block = terminal_block("READ", expanded_story_title(story));
    let inner = block.inner(panel);
    frame.render_widget(block, panel);
    let rows = Layout::vertical([Constraint::Min(3), Constraint::Length(2)]).split(inner);
    viewport.set((rows[0].width, rows[0].height));
    frame.render_widget(
        Paragraph::new(story_detail_lines(story, rows[0].height < 28))
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0)),
        rows[0],
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled(
                    " [ OPEN PUBLISHER · O ] ",
                    if story.url.is_some() {
                        Style::new().bg(AMBER.into()).fg(BG.into()).bold()
                    } else {
                        Style::new().fg(MUTED.into())
                    },
                ),
                Span::styled("  J/K OR PGUP/PGDN SCROLL · V/ESC CLOSE", MUTED),
            ]),
            Line::styled(article_footer_status(story, article_status), MUTED),
        ]),
        rows[1],
    );
    frame.render_widget(
        Paragraph::new(" [ CLOSE · V / ESC ] ")
            .style(Style::new().bg(CYAN.into()).fg(BG.into()).bold()),
        expanded_close_area(area),
    );
}

fn story_detail_lines(story: &NewsStory, compact: bool) -> Vec<Line<'_>> {
    let mut lines = vec![
        Line::styled(
            story.headline.title.as_str(),
            Style::new().fg(INK.into()).bold(),
        ),
        Line::styled(
            format!(
                "{} · {} · {} · {}",
                story.headline.topic,
                story.headline.region,
                story.headline.time,
                story.provenance.freshness.label()
            ),
            MUTED,
        ),
    ];
    lines.extend(story_provenance_lines(story));
    lines.extend(story_sentiment_lines(story, compact));
    lines.extend([
        Line::styled(format!("BY {}", story.byline), MUTED),
        Line::raw(""),
        Line::styled(story.summary.as_str(), YELLOW),
    ]);
    for paragraph in &story.body {
        lines.push(Line::raw(""));
        lines.push(Line::raw(paragraph.as_str()));
    }
    lines.extend([
        Line::raw(""),
        Line::styled(
            format!("RELATED  {}", story.related_symbols.join("  ")),
            CYAN,
        ),
        Line::styled(
            story.url.as_ref().map_or_else(
                || "SOURCE   UNAVAILABLE".to_owned(),
                |url| format!("SOURCE   {url}"),
            ),
            MUTED,
        ),
        Line::raw(""),
        Line::styled(article_body_status(story), MUTED),
    ]);
    lines
}

fn story_provenance_lines(story: &NewsStory) -> Vec<Line<'static>> {
    let sources = if story.provenance.sources.is_empty() {
        "UNAVAILABLE".to_owned()
    } else {
        story.provenance.sources.join(" + ")
    };
    let published = story
        .provenance
        .published_at
        .as_deref()
        .unwrap_or("PUBLICATION TIME UNAVAILABLE");
    let retrieved = if story.provenance.retrieved_at.is_empty() {
        "RETRIEVAL TIME UNAVAILABLE"
    } else {
        story.provenance.retrieved_at.as_str()
    };
    let language = story
        .provenance
        .language
        .as_deref()
        .unwrap_or("UNAVAILABLE");
    let categories = story
        .provenance
        .categories
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(" / ");
    let mut lines = vec![
        Line::styled(format!("SOURCE  {sources}"), MUTED),
        Line::styled(format!("PUB     {published}"), MUTED),
        Line::styled(format!("FETCH   {retrieved} · LANG {language}"), MUTED),
    ];
    if !categories.is_empty() {
        lines.push(Line::styled(format!("TAGS    {categories}"), MUTED));
    }
    lines
}

fn story_sentiment_lines(story: &NewsStory, compact: bool) -> Vec<Line<'static>> {
    let sentiment = &story.sentiment;
    let evidence = if sentiment.evidence.is_empty() {
        "NONE IN BOUNDED LEXICON".to_owned()
    } else {
        sentiment
            .evidence
            .iter()
            .take(6)
            .map(|item| {
                let negated = if item.negated { " NOT" } else { "" };
                format!(
                    "{}{}{}({})",
                    item.polarity.label(),
                    negated,
                    item.term,
                    item.weight
                )
            })
            .collect::<Vec<_>>()
            .join("  ")
    };
    let mut lines = vec![
        Line::styled(
            format!(
                "TONE    {} · SCORE {} · EVIDENCE CONF {}{}",
                sentiment.label.label(),
                sentiment.score_label(),
                sentiment.evidence_confidence_label(),
                if compact { " · UNCALIBRATED" } else { "" }
            ),
            YELLOW,
        ),
        Line::styled(format!("EVID    {evidence}"), MUTED),
    ];
    if compact {
        lines.push(Line::styled(
            format!(
                "METHOD  {} · NOT FACT/FORECAST/SIGNAL",
                sentiment.method_version
            ),
            MUTED,
        ));
    } else {
        lines.extend([
            Line::styled(
                format!(
                    "METHOD  {} · {} · OBS {}",
                    sentiment.method_version, sentiment.input_scope, sentiment.observed_at
                ),
                MUTED,
            ),
            Line::styled(format!("CAL     {}", sentiment.calibration), MUTED),
            Line::styled(format!("NOTE    {}", sentiment.disclosure), MUTED),
        ]);
    }
    lines
}

fn expanded_story_title(story: &NewsStory) -> &'static str {
    match story.body_state {
        ArticleBodyState::Downloaded => "ARTICLE · IN-TERMINAL READER",
        ArticleBodyState::FeedProvided => "ARTICLE · PUBLISHER FEED CONTENT",
        ArticleBodyState::Loading => "ARTICLE · FETCHING TEXT",
        ArticleBodyState::ExcerptOnly | ArticleBodyState::Unavailable(_) => {
            "ARTICLE · PUBLISHER EXCERPT"
        }
    }
}

fn article_body_status(story: &NewsStory) -> String {
    match &story.body_state {
        ArticleBodyState::Downloaded => {
            "ARTICLE TEXT EXTRACTED ON DEMAND · O OPENS THE ORIGINAL PUBLISHER PAGE".to_owned()
        }
        ArticleBodyState::FeedProvided => {
            "PUBLISHER-PROVIDED FEED CONTENT · O OPENS THE ORIGINAL PAGE".to_owned()
        }
        ArticleBodyState::Loading => "FETCHING AND EXTRACTING ARTICLE TEXT…".to_owned(),
        ArticleBodyState::ExcerptOnly => {
            "FEED EXCERPT ONLY · PRESS R TO FETCH OR O TO OPEN THE PUBLISHER".to_owned()
        }
        ArticleBodyState::Unavailable(error) => {
            format!("FULL TEXT UNAVAILABLE · {error} · O OPENS THE PUBLISHER")
        }
    }
}

fn article_footer_status(story: &NewsStory, article_status: &str) -> String {
    if article_status.starts_with("OPENED")
        || article_status.starts_with("OPEN FAILED")
        || article_status.starts_with("NO PUBLISHER")
    {
        article_status.to_owned()
    } else {
        article_body_status(story)
    }
}

fn wrapped_height(lines: &[Line<'_>], width: u16) -> u16 {
    if width == 0 {
        return 0;
    }
    lines
        .iter()
        .map(|line| (line.width() as u16).div_ceil(width).max(1))
        .sum()
}

fn render_calendar(frame: &mut Frame, area: Rect, workbench: &NewsWorkbench) {
    if workbench.events.is_empty() {
        frame.render_widget(
            Paragraph::new("NO LIVE ECONOMIC CALENDAR PROVIDER IS CONFIGURED\n\nTHE NEWS FEED DOES NOT SUBSTITUTE FABRICATED EVENTS.")
                .style(MUTED)
                .block(terminal_block("ECO", "ECONOMIC EVENT CALENDAR")),
            area,
        );
        return;
    }
    if area.width < 72 {
        let rows = workbench.events.iter().map(|event| {
            Row::new(vec![
                Cell::from(event.time.clone()),
                Cell::from(event.region.clone()),
                Cell::from(event.importance.label()),
                Cell::from(event.event.clone()),
                Cell::from(event.survey.clone()),
            ])
        });
        let table = Table::new(
            rows,
            [
                Constraint::Length(6),
                Constraint::Length(4),
                Constraint::Length(6),
                Constraint::Min(14),
                Constraint::Length(9),
            ],
        )
        .header(Row::new(["TIME", "REG", "IMP", "EVENT", "SURVEY"]).style(AMBER))
        .column_spacing(1)
        .block(terminal_block("ECO", "ECONOMIC EVENT CALENDAR"));
        frame.render_widget(table, area);
        return;
    }
    let rows = workbench.events.iter().map(|event| {
        Row::new(vec![
            Cell::from(event.time.clone()),
            Cell::from(event.region.clone()),
            Cell::from(event.importance.label()),
            Cell::from(event.event.clone()),
            Cell::from(event.period.clone()),
            Cell::from(event.survey.clone()),
            Cell::from(event.prior.clone()),
        ])
    });
    let table = Table::new(
        rows,
        [
            Constraint::Length(6),
            Constraint::Length(4),
            Constraint::Length(6),
            Constraint::Min(24),
            Constraint::Length(8),
            Constraint::Length(9),
            Constraint::Length(9),
        ],
    )
    .header(Row::new(["TIME", "REG", "IMP", "EVENT", "PERIOD", "SURVEY", "PRIOR"]).style(AMBER))
    .column_spacing(1)
    .block(terminal_block("ECO", "ECONOMIC EVENT CALENDAR"));
    frame.render_widget(table, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::news::{Headline, NewsArticleOpenError, NewsSnapshot};
    use crossterm::event::{KeyModifiers, MouseButton, MouseEventKind};
    use std::sync::Mutex;

    fn headlines() -> Vec<Headline> {
        vec![
            Headline {
                time: "16:00".into(),
                topic: "TOP".into(),
                title: "Markets gain".into(),
                region: "US".into(),
            },
            Headline {
                time: "14:00".into(),
                topic: "TEC".into(),
                title: "Chip rally".into(),
                region: "AS".into(),
            },
        ]
    }
    struct StubQuery;
    impl NewsFeed for StubQuery {
        fn load_news(&self) -> NewsSnapshot {
            NewsSnapshot {
                headlines: headlines(),
            }
        }
    }

    struct LinkedQuery;
    impl NewsFeed for LinkedQuery {
        fn load_news(&self) -> NewsSnapshot {
            NewsSnapshot {
                headlines: headlines(),
            }
        }

        fn load_workbench(&self) -> NewsWorkbench {
            let mut workbench = NewsWorkbench::from_snapshot(self.load_news());
            workbench.stories[0].url = Some("https://example.com/markets-gain".to_owned());
            workbench
        }
    }

    struct ReorderedQuery;
    impl NewsFeed for ReorderedQuery {
        fn load_news(&self) -> NewsSnapshot {
            let mut headlines = headlines();
            headlines.reverse();
            NewsSnapshot { headlines }
        }
    }

    struct LongLinkedQuery;
    impl NewsFeed for LongLinkedQuery {
        fn load_news(&self) -> NewsSnapshot {
            NewsSnapshot {
                headlines: headlines(),
            }
        }

        fn load_workbench(&self) -> NewsWorkbench {
            let mut workbench = NewsWorkbench::from_snapshot(self.load_news());
            workbench.stories[0].url = Some("https://example.com/markets-gain".to_owned());
            workbench.stories[0].body = (1..=40)
                .map(|index| format!("Feed-supplied excerpt paragraph {index}."))
                .collect();
            workbench
        }
    }

    #[derive(Default)]
    struct RequestingQuery {
        requests: Mutex<Vec<(String, String)>>,
    }

    impl NewsFeed for RequestingQuery {
        fn load_news(&self) -> NewsSnapshot {
            NewsSnapshot {
                headlines: headlines(),
            }
        }

        fn load_workbench(&self) -> NewsWorkbench {
            let mut workbench = NewsWorkbench::from_snapshot(self.load_news());
            workbench.stories[0].url = Some("https://example.com/markets-gain".to_owned());
            workbench.stories[0].body.clear();
            workbench.stories[0].body_state = ArticleBodyState::ExcerptOnly;
            workbench
        }

        fn request_article(&self, story_id: &str, url: &str) -> bool {
            self.requests
                .lock()
                .unwrap()
                .push((story_id.to_owned(), url.to_owned()));
            true
        }
    }

    #[derive(Default)]
    struct RecordingOpener {
        opened: Mutex<Vec<String>>,
    }

    impl NewsArticleOpener for RecordingOpener {
        fn open(&self, url: &str) -> Result<(), NewsArticleOpenError> {
            self.opened.lock().unwrap().push(url.to_owned());
            Ok(())
        }
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
    fn command_options_scope_the_news_workbench() {
        let mut workspace = NewsWorkspace::new(Arc::new(StubQuery));
        workspace.handle_command(&CommandInvocation {
            function: "NEWS".into(),
            args: vec!["--region=AS".into(), "--topic=TEC".into()],
        });
        assert_eq!(
            workspace.visible_indices(&workspace.query.load_workbench()),
            vec![1]
        );
    }

    #[test]
    fn positional_subject_filters_by_linked_symbol() {
        let mut workspace = NewsWorkspace::new(Arc::new(StubQuery));
        workspace.handle_command(&CommandInvocation {
            function: "NEWS".into(),
            args: vec!["NVDA".into()],
        });
        assert_eq!(
            workspace.visible_indices(&workspace.query.load_workbench()),
            vec![1]
        );
    }

    #[test]
    fn typed_view_round_trips_all_filters_subview_and_story_identity() {
        let mut source = NewsWorkspace::new(Arc::new(StubQuery));
        source.filter = NewsFilter {
            region: Some("AS".to_owned()),
            topic: Some("TEC".to_owned()),
            symbol: Some("NVDA".to_owned()),
            unread_only: true,
            bookmarked_only: true,
        };
        source.bookmarks.insert("14:00:TEC:Chip rally".to_owned());
        source.show_calendar = true;
        source.detail_expanded = true;
        source.detail_scroll = 42;
        let state = source.capture_view();

        assert_eq!(state.fields.len(), 7);
        assert_eq!(
            state.fields.get("selected_story_id"),
            Some(&ViewValue::Text("14:00:TEC:Chip rally".to_owned()))
        );
        assert_eq!(
            state.fields.get("subview"),
            Some(&ViewValue::Text("events".to_owned()))
        );
        assert!(!state.fields.contains_key("reader_open"));
        assert!(!state.fields.contains_key("detail_scroll"));

        let mut restored = NewsWorkspace::new(Arc::new(StubQuery));
        restored.bookmarks.insert("14:00:TEC:Chip rally".to_owned());
        let report = restored.restore_view(&state);

        assert_eq!(report.restored_fields, 7);
        assert_eq!(report.skipped_fields, 0);
        assert!(report.warnings.is_empty());
        assert!(restored.show_calendar);
        assert!(!restored.detail_expanded);
        assert_eq!(restored.detail_scroll, 0);
        assert_eq!(restored.capture_view(), state);
    }

    #[test]
    fn typed_view_follows_story_identity_after_feed_reordering() {
        let mut source = NewsWorkspace::new(Arc::new(StubQuery));
        source.selected = 1;
        let state = source.capture_view();

        let mut restored = NewsWorkspace::new(Arc::new(ReorderedQuery));
        let report = restored.restore_view(&state);

        assert_eq!(report.restored_fields, 4);
        assert_eq!(report.skipped_fields, 0);
        assert_eq!(restored.selected, 0);
        assert_eq!(restored.capture_view(), state);
    }

    #[test]
    fn typed_view_degrades_invalid_future_and_missing_story_state() {
        let state = WorkspaceViewState::new(ID.as_str())
            .with_field("region", ViewValue::Text("asia".to_owned()))
            .with_field("topic", ViewValue::Text("TEC?admin=true".to_owned()))
            .with_field("symbol", ViewValue::Boolean(true))
            .with_field("unread_only", ViewValue::Text("yes".to_owned()))
            .with_field("bookmarked_only", ViewValue::Unsigned(1))
            .with_field("subview", ViewValue::Text("future".to_owned()))
            .with_field(
                "selected_story_id",
                ViewValue::Text("retired-provider-story".to_owned()),
            )
            .with_field("future_field", ViewValue::Boolean(true))
            .with_child(WorkspaceViewState::new("future-news-child"));
        let mut restored = NewsWorkspace::new(Arc::new(StubQuery));
        restored.filter.region = Some("US".to_owned());
        restored.show_calendar = true;
        restored.detail_expanded = true;

        let report = restored.restore_view(&state);

        assert_eq!(report.restored_fields, 0);
        assert_eq!(report.skipped_fields, 9);
        assert_eq!(report.warnings.len(), 9);
        assert_eq!(restored.filter, NewsFilter::default());
        assert!(!restored.show_calendar);
        assert!(!restored.detail_expanded);
        assert_eq!(restored.selected, 0);
        assert_eq!(
            restored.article_status,
            "SAVED STORY UNAVAILABLE · USING FIRST VISIBLE STORY"
        );
    }

    #[test]
    fn bookmark_and_read_state_are_independent() {
        let mut workspace = NewsWorkspace::new(Arc::new(StubQuery));
        workspace.handle_key(KeyEvent::new(
            KeyCode::Char('b'),
            crossterm::event::KeyModifiers::NONE,
        ));
        workspace.handle_key(KeyEvent::new(
            KeyCode::Char('r'),
            crossterm::event::KeyModifiers::NONE,
        ));
        let story = workspace
            .query
            .load_workbench()
            .stories
            .into_iter()
            .next()
            .unwrap();
        assert!(workspace.bookmarks.contains(&story.id));
        assert!(workspace.read.contains(&story.id));
    }

    #[test]
    fn clicking_a_headline_selects_its_story() {
        let mut workspace = NewsWorkspace::new(Arc::new(StubQuery));

        assert!(workspace.handle_mouse(click(2, 7), Rect::new(0, 0, 120, 30)));

        assert_eq!(workspace.selected, 1);
    }

    #[test]
    fn narrow_news_uses_full_width_for_clickable_list_and_calendar() {
        use ratatui::{backend::TestBackend, Terminal};

        let area = Rect::new(0, 0, 80, 24);
        let mut workspace = NewsWorkspace::new(Arc::new(StubQuery));
        let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
        terminal
            .draw(|frame| workspace.render(frame, area))
            .unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("TOP NEWS"));
        assert!(!rendered.contains("STORY DETAIL"));

        assert!(workspace.handle_mouse(click(70, 7), area));
        assert_eq!(workspace.selected, 1);
        assert!(workspace.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE)));
        terminal
            .draw(|frame| workspace.render(frame, area))
            .unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("ECONOMIC EVENT CALENDAR"));
    }

    #[test]
    fn compact_calendar_preserves_time_region_importance_and_survey_columns() {
        use ratatui::{backend::TestBackend, Terminal};

        let area = Rect::new(0, 0, 51, 20);
        let workbench = NewsWorkbench::from_snapshot(NewsSnapshot {
            headlines: headlines(),
        });
        let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
        terminal
            .draw(|frame| render_calendar(frame, area, &workbench))
            .unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();

        assert!(rendered.contains("TIME"));
        assert!(rendered.contains("REG"));
        assert!(rendered.contains("IMP"));
        assert!(rendered.contains("EVENT"));
        assert!(rendered.contains("SURVEY"));
        assert!(!rendered.contains("PRIOR"));
    }

    #[test]
    fn actions_share_geometry_revalidate_story_identity_and_trap_the_reader() {
        let area = Rect::new(5, 3, 120, 30);
        let mut workspace = NewsWorkspace::new(Arc::new(LinkedQuery));
        let actions = workspace.actions(area);
        let ids = actions
            .iter()
            .map(|action| action.id.as_str())
            .collect::<std::collections::HashSet<_>>();

        assert_eq!(ids.len(), actions.len());
        assert!(actions.iter().all(|action| {
            action.area.x >= area.x
                && action.area.y >= area.y
                && action.area.right() <= area.right()
                && action.area.bottom() <= area.bottom()
        }));
        assert!(actions
            .iter()
            .any(|action| action.id.starts_with("story:0:") && action.preferred));
        assert!(actions
            .iter()
            .any(|action| action.id.starts_with("story-read:")));
        assert!(actions
            .iter()
            .any(|action| action.id.starts_with("story-web:") && action.enabled));
        assert!(actions.iter().any(|action| action.id == "view:events"));

        let stale_read = actions
            .iter()
            .find(|action| action.id.starts_with("control:read-state:"))
            .unwrap()
            .id
            .clone();
        workspace.selected = 1;
        assert!(!workspace.activate_action(&stale_read));

        let second_story = workspace
            .actions(area)
            .into_iter()
            .find(|action| action.id.starts_with("story:1:"))
            .unwrap()
            .id;
        assert!(workspace.activate_action(&second_story));
        assert!(workspace.is_modal_active());
        assert_eq!(workspace.selected, 1);
        let modal = workspace.actions(area);
        assert_eq!(modal.len(), 2);
        assert!(modal
            .iter()
            .any(|action| action.id.starts_with("modal:close:") && action.preferred));
        assert!(modal
            .iter()
            .any(|action| action.id.starts_with("modal:web:") && !action.enabled));
        assert!(modal.iter().all(|action| action.id.starts_with("modal:")));
        let close = modal
            .iter()
            .find(|action| action.id.starts_with("modal:close:"))
            .unwrap()
            .id
            .clone();
        assert!(workspace.activate_action(&close));
        assert!(!workspace.is_modal_active());

        let workbench = workspace.query.load_workbench();
        let controls = news_areas(area).controls;
        let asia = workspace
            .control_areas(controls, &workbench)
            .into_iter()
            .find(|(control, _)| *control == NewsControl::RegionAsia)
            .unwrap()
            .1;
        assert!(workspace.handle_mouse(click(asia.x, asia.y), area));
        assert_eq!(workspace.filter.region.as_deref(), Some("AS"));
    }

    #[test]
    fn open_shortcut_launches_the_selected_publisher_url() {
        let opener = Arc::new(RecordingOpener::default());
        let mut workspace =
            NewsWorkspace::with_article_opener(Arc::new(LinkedQuery), opener.clone());

        assert!(workspace.handle_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE)));

        assert_eq!(
            *opener.opened.lock().unwrap(),
            vec!["https://example.com/markets-gain".to_owned()]
        );
        assert_eq!(
            workspace.article_status,
            "OPENED PUBLISHER ARTICLE IN YOUR BROWSER"
        );
    }

    #[test]
    fn enter_opens_the_terminal_reader_and_requests_missing_article_text() {
        let query = Arc::new(RequestingQuery::default());
        let mut workspace = NewsWorkspace::new(query.clone());

        assert!(workspace.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));

        assert!(workspace.detail_expanded);
        assert_eq!(query.requests.lock().unwrap().len(), 1);
        assert_eq!(
            workspace.article_status,
            "FETCHING ARTICLE TEXT IN THE BACKGROUND"
        );
    }

    #[test]
    fn clicking_the_story_action_opens_the_terminal_reader() {
        let query = Arc::new(RequestingQuery::default());
        let mut workspace = NewsWorkspace::new(query.clone());

        assert!(workspace.handle_mouse(click(50, 27), Rect::new(0, 0, 120, 30)));

        assert!(workspace.detail_expanded);
        assert_eq!(query.requests.lock().unwrap().len(), 1);
    }

    #[test]
    fn clicking_the_web_action_launches_the_selected_publisher_url() {
        let opener = Arc::new(RecordingOpener::default());
        let mut workspace =
            NewsWorkspace::with_article_opener(Arc::new(LinkedQuery), opener.clone());

        assert!(workspace.handle_mouse(click(80, 27), Rect::new(0, 0, 120, 30)));

        assert_eq!(opener.opened.lock().unwrap().len(), 1);
    }

    #[test]
    fn missing_publisher_url_does_not_call_the_opener() {
        let opener = Arc::new(RecordingOpener::default());
        let mut workspace = NewsWorkspace::with_article_opener(Arc::new(StubQuery), opener.clone());

        assert!(workspace.handle_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE)));

        assert!(opener.opened.lock().unwrap().is_empty());
        assert_eq!(workspace.article_status, "NO PUBLISHER LINK FOR THIS STORY");
    }

    #[test]
    fn expanded_story_scrolls_opens_and_closes_by_keyboard_and_mouse() {
        use ratatui::{backend::TestBackend, Terminal};

        let area = Rect::new(0, 0, 120, 30);
        let opener = Arc::new(RecordingOpener::default());
        let mut workspace =
            NewsWorkspace::with_article_opener(Arc::new(LongLinkedQuery), opener.clone());

        assert!(workspace.handle_key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE)));
        assert!(workspace.detail_expanded);
        let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
        terminal
            .draw(|frame| workspace.render(frame, area))
            .unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("PUBLISHER FEED CONTENT"));

        assert!(workspace.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE)));
        assert!(workspace.detail_scroll > 0);
        let open = expanded_open_area(area);
        assert!(workspace.handle_mouse(click(open.x + 1, open.y), area));
        assert_eq!(opener.opened.lock().unwrap().len(), 1);
        let close = expanded_close_area(area);
        assert!(workspace.handle_mouse(click(close.x + 1, close.y), area));
        assert!(!workspace.detail_expanded);

        workspace.handle_key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE));
        assert!(workspace.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
        assert!(!workspace.detail_expanded);
    }

    #[test]
    fn security_shortcut_dispatches_linked_instrument() {
        let mut workspace = NewsWorkspace::new(Arc::new(StubQuery));
        workspace.handle_key(KeyEvent::new(
            KeyCode::Char('s'),
            crossterm::event::KeyModifiers::NONE,
        ));
        assert_eq!(
            workspace.poll_intents(),
            vec![AppIntent::DispatchCommand {
                command: "SEC SPY US".into(),
                origin: ID,
            }]
        );
    }
}
