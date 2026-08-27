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
    app::{AppIntent, CommandInvocation, Workspace, WorkspaceDescriptor},
    ui::{
        components::terminal_block,
        is_primary_click, list_row_at, scroll_key,
        theme::{AMBER, BG, CYAN, INK, MUTED, YELLOW},
    },
};

use super::{
    ArticleBodyState, NewsArticleOpener, NewsFeed, NewsFilter, NewsStory, NewsWorkbench, ID,
};

const WIDE_NEWS_MIN_COLUMNS: u16 = 90;

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
        let maximum = wrapped_height(&story_detail_lines(story), width).saturating_sub(height);
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
                if let Some(story) = self.selected_story(&workbench) {
                    if !self.read.insert(story.id.clone()) {
                        self.read.remove(&story.id);
                    }
                }
            }
            KeyCode::Char('b') => {
                if let Some(story) = self.selected_story(&workbench) {
                    if !self.bookmarks.insert(story.id.clone()) {
                        self.bookmarks.remove(&story.id);
                    }
                }
            }
            KeyCode::Char('u') => {
                self.filter.unread_only = !self.filter.unread_only;
                self.selected = 0;
            }
            KeyCode::Char('m') => {
                self.filter.bookmarked_only = !self.filter.bookmarked_only;
                self.selected = 0;
            }
            KeyCode::Char('e') => self.show_calendar = !self.show_calendar,
            KeyCode::F(9) => self.query.request_refresh(),
            KeyCode::Char('0') => {
                self.filter = NewsFilter::default();
                self.show_calendar = false;
                self.selected = 0;
            }
            KeyCode::Char('1') => {
                self.filter.region = Some("US".into());
                self.selected = 0;
            }
            KeyCode::Char('2') => {
                self.filter.region = Some("EU".into());
                self.selected = 0;
            }
            KeyCode::Char('3') => {
                self.filter.region = Some("AS".into());
                self.selected = 0;
            }
            KeyCode::Char('s') => {
                if let Some(symbol) = self
                    .selected_story(&workbench)
                    .and_then(|story| story.related_symbols.first())
                {
                    self.pending_intents.push(AppIntent::DispatchCommand {
                        command: format!("SEC {symbol} US"),
                        origin: ID,
                    });
                }
            }
            KeyCode::Char('a') => {
                if let Some(symbol) = self
                    .selected_story(&workbench)
                    .and_then(|story| story.related_symbols.first())
                {
                    self.pending_intents.push(AppIntent::DispatchCommand {
                        command: format!("SHEET INSERT {symbol} US"),
                        origin: ID,
                    });
                }
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
        let rows = Layout::vertical([Constraint::Length(3), Constraint::Min(10)]).split(area);
        if rows[1].width < WIDE_NEWS_MIN_COLUMNS {
            if self.show_calendar {
                return crate::ui::contains(rows[1], event.column, event.row);
            }
            if let Some(index) = list_row_at(event, rows[1], visible_count) {
                self.selected = index;
                self.detail_scroll = 0;
                self.article_status = "ENTER READS HERE · O OPENS THE PUBLISHER".to_owned();
                return true;
            }
            if let Some(key) = scroll_key(event, rows[1]) {
                return self.handle_key(key);
            }
            return false;
        }
        let columns = Layout::horizontal([
            Constraint::Percentage(39),
            Constraint::Percentage(43),
            Constraint::Percentage(18),
        ])
        .split(rows[1]);
        if !self.show_calendar && is_primary_click(event, story_open_area(columns[1])) {
            self.open_selected_reader(&workbench);
            return true;
        }
        if !self.show_calendar && is_primary_click(event, story_expand_area(columns[1])) {
            self.open_selected_article(&workbench);
            return true;
        }
        if let Some(index) = list_row_at(event, columns[0], visible_count) {
            self.selected = index;
            self.detail_scroll = 0;
            self.article_status = "ENTER READS HERE · O OPENS THE PUBLISHER".to_owned();
            return true;
        }
        if is_primary_click(event, columns[2]) {
            self.show_calendar = true;
            return true;
        }
        if let Some(key) = scroll_key(event, rows[1]) {
            return self.handle_key(key);
        }
        false
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
        }
        let visible = self.visible_indices(&workbench);
        let unread = workbench
            .stories
            .iter()
            .filter(|story| !self.read.contains(&story.id))
            .count();
        let feed_status = self.query.status();
        let rows = Layout::vertical([Constraint::Length(3), Constraint::Min(10)]).split(area);
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
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(format!(" {} RESULTS  {unread} UNREAD  ", visible.len()), Style::new().bg(AMBER.into()).fg(BG.into()).bold()),
                Span::styled(filter_label, INK),
                Span::styled("  ENTER/V READ HERE · O WEB · R READ · 0 RESET · 1/2/3 REGION · U UNREAD · M SAVED · E EVENTS · S SECURITY · F9 REFRESH  ", MUTED),
                Span::styled(feed_status, YELLOW),
            ])).block(terminal_block("NEWS", "FILTERS & WORKFLOW")),
            rows[0],
        );

        let columns = Layout::horizontal([
            Constraint::Percentage(39),
            Constraint::Percentage(43),
            Constraint::Percentage(18),
        ])
        .split(rows[1]);
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
        if rows[1].width < WIDE_NEWS_MIN_COLUMNS {
            if self.show_calendar {
                render_calendar(frame, rows[1], &workbench);
            } else {
                frame.render_widget(stories, rows[1]);
            }
            return;
        }
        frame.render_widget(stories, columns[0]);

        if self.show_calendar {
            render_calendar(frame, columns[1], &workbench);
        } else if let Some(story) = self.selected_story(&workbench) {
            render_story(frame, columns[1], story, &self.article_status);
        } else {
            frame.render_widget(
                Paragraph::new("NO STORIES MATCH THE ACTIVE FILTER\n\nPRESS 0 TO RESET")
                    .style(MUTED)
                    .block(terminal_block("READ", "STORY")),
                columns[1],
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
            columns[2],
        );
    }
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
    let mut lines = vec![
        Line::styled(
            format!(
                "{} · {} · LIVE FEED",
                story.headline.topic, story.headline.time
            ),
            AMBER,
        ),
        Line::styled(story.byline.as_str(), MUTED),
        Line::raw(""),
        Line::styled(
            story.headline.title.as_str(),
            Style::new().fg(INK.into()).bold(),
        ),
        Line::raw(""),
        Line::styled(story.summary.as_str(), YELLOW),
        Line::raw(""),
    ];
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
        Paragraph::new(story_detail_lines(story))
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

fn story_detail_lines(story: &NewsStory) -> Vec<Line<'_>> {
    let mut lines = vec![
        Line::styled(
            story.headline.title.as_str(),
            Style::new().fg(INK.into()).bold(),
        ),
        Line::styled(
            format!(
                "{} · {} · {} · {}",
                story.headline.topic, story.headline.region, story.headline.time, story.byline
            ),
            MUTED,
        ),
        Line::raw(""),
        Line::styled(story.summary.as_str(), YELLOW),
    ];
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

        assert!(workspace.handle_mouse(click(2, 5), Rect::new(0, 0, 120, 30)));

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

        assert!(workspace.handle_mouse(click(70, 5), area));
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
