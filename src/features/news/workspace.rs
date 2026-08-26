use std::{collections::HashSet, sync::Arc};

use crossterm::event::{KeyCode, KeyEvent, MouseEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Cell, List, ListItem, Paragraph, Row, Table, Wrap},
};

use crate::{
    app::{AppIntent, CommandInvocation, Workspace, WorkspaceDescriptor},
    ui::{
        components::terminal_block,
        is_primary_click, list_row_at, scroll_key,
        theme::{AMBER, BG, CYAN, INK, MUTED, YELLOW},
    },
};

use super::{ID, NewsArticleOpener, NewsFeed, NewsFilter, NewsStory, NewsWorkbench};

pub struct NewsWorkspace {
    query: Arc<dyn NewsFeed>,
    article_opener: Option<Arc<dyn NewsArticleOpener>>,
    article_status: String,
    selected: usize,
    filter: NewsFilter,
    read: HashSet<String>,
    bookmarks: HashSet<String>,
    show_calendar: bool,
    pending_intents: Vec<AppIntent>,
}

impl NewsWorkspace {
    pub fn new(query: Arc<dyn NewsFeed>) -> Self {
        Self {
            query,
            article_opener: None,
            article_status: "O / ENTER OPENS THE PUBLISHER SOURCE".to_owned(),
            selected: 0,
            filter: NewsFilter::default(),
            read: HashSet::new(),
            bookmarks: HashSet::new(),
            show_calendar: false,
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
        let length = self.visible_indices(&workbench).len();
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                self.selected = (self.selected + 1).min(length.saturating_sub(1));
            }
            KeyCode::Up | KeyCode::Char('k') => self.selected = self.selected.saturating_sub(1),
            KeyCode::Enter | KeyCode::Char('o') => self.open_selected_article(&workbench),
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
            _ => return false,
        }
        self.clamp_selection();
        true
    }

    fn handle_mouse(&mut self, event: MouseEvent, area: Rect) -> bool {
        let workbench = self.query.load_workbench();
        let visible_count = self.visible_indices(&workbench).len();
        let rows = Layout::vertical([Constraint::Length(3), Constraint::Min(10)]).split(area);
        let columns = Layout::horizontal([
            Constraint::Percentage(39),
            Constraint::Percentage(43),
            Constraint::Percentage(18),
        ])
        .split(rows[1]);
        if !self.show_calendar && is_primary_click(event, story_open_area(columns[1])) {
            self.open_selected_article(&workbench);
            return true;
        }
        if let Some(index) = list_row_at(event, columns[0], visible_count) {
            self.selected = index;
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
                Span::styled(format!(" {} RESULTS  {unread} UNREAD  ", visible.len()), Style::new().bg(AMBER).fg(BG).bold()),
                Span::styled(filter_label, INK),
                Span::styled("  O OPEN · R READ · 0 RESET · 1/2/3 REGION · U UNREAD · M SAVED · E EVENTS · S SECURITY · F9 REFRESH  ", MUTED),
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
                            Style::new().bg(CYAN).fg(BG)
                        } else {
                            Style::new().fg(INK)
                        },
                    ),
                    Span::styled(format!(" {}", story.headline.region), MUTED),
                ])))
            })
            .collect::<Vec<_>>();
        frame.render_widget(
            List::new(items).block(terminal_block("TOP", "TOP NEWS")),
            columns[0],
        );

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
    Layout::vertical([Constraint::Min(6), Constraint::Length(4)]).split(area)[1]
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
        Line::styled(story.headline.title.as_str(), Style::new().fg(INK).bold()),
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
            Line::styled(
                " [ OPEN ARTICLE · O / ENTER ] ",
                Style::new().bg(AMBER).fg(BG).bold(),
            ),
            Line::styled(article_status, MUTED),
        ]
    } else {
        vec![
            Line::styled(" NO PUBLISHER LINK AVAILABLE ", Style::new().fg(MUTED)),
            Line::styled(article_status, MUTED),
        ]
    };
    frame.render_widget(
        Paragraph::new(action).block(terminal_block("WEB", "PUBLISHER SOURCE")),
        rows[1],
    );
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
    fn clicking_the_story_action_launches_the_selected_publisher_url() {
        let opener = Arc::new(RecordingOpener::default());
        let mut workspace =
            NewsWorkspace::with_article_opener(Arc::new(LinkedQuery), opener.clone());

        assert!(workspace.handle_mouse(click(50, 27), Rect::new(0, 0, 120, 30)));

        assert_eq!(opener.opened.lock().unwrap().len(), 1);
    }

    #[test]
    fn missing_publisher_url_does_not_call_the_opener() {
        let opener = Arc::new(RecordingOpener::default());
        let mut workspace = NewsWorkspace::with_article_opener(Arc::new(StubQuery), opener.clone());

        assert!(workspace.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));

        assert!(opener.opened.lock().unwrap().is_empty());
        assert_eq!(workspace.article_status, "NO PUBLISHER LINK FOR THIS STORY");
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
