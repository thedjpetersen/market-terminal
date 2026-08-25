use std::{collections::HashSet, sync::Arc};

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Cell, List, ListItem, Paragraph, Row, Table, Wrap},
    Frame,
};

use crate::{
    app::{AppIntent, CommandInvocation, Workspace, WorkspaceDescriptor},
    ui::{
        components::terminal_block,
        theme::{AMBER, BG, CYAN, INK, MUTED, YELLOW},
    },
};

use super::{NewsFilter, NewsQuery, NewsStory, NewsWorkbench, ID};

pub struct NewsWorkspace {
    query: Arc<dyn NewsQuery>,
    selected: usize,
    filter: NewsFilter,
    read: HashSet<String>,
    bookmarks: HashSet<String>,
    show_calendar: bool,
    pending_intents: Vec<AppIntent>,
}

impl NewsWorkspace {
    pub fn new(query: Arc<dyn NewsQuery>) -> Self {
        Self {
            query,
            selected: 0,
            filter: NewsFilter::default(),
            read: HashSet::new(),
            bookmarks: HashSet::new(),
            show_calendar: false,
            pending_intents: Vec::new(),
        }
    }

    fn visible_indices(&self, workbench: &NewsWorkbench) -> Vec<usize> {
        workbench.stories.iter().enumerate().filter_map(|(index, story)| {
            self.filter.matches(
                story,
                self.read.contains(&story.id),
                self.bookmarks.contains(&story.id),
            ).then_some(index)
        }).collect()
    }

    fn selected_story<'a>(&self, workbench: &'a NewsWorkbench) -> Option<&'a NewsStory> {
        let visible = self.visible_indices(workbench);
        visible.get(self.selected.min(visible.len().saturating_sub(1)))
            .and_then(|index| workbench.stories.get(*index))
    }

    fn clamp_selection(&mut self) {
        let workbench = self.query.load_workbench();
        self.selected = self.selected.min(self.visible_indices(&workbench).len().saturating_sub(1));
    }

    fn set_option(&mut self, option: &str) {
        let Some((name, value)) = option.strip_prefix("--").and_then(|value| value.split_once('=')) else {
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
                if matches!(subject.as_str(), "TOP" | "FED" | "ECO" | "POL" | "CMD" | "TEC") {
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
            KeyCode::Enter | KeyCode::Char('r') => {
                if let Some(story) = self.selected_story(&workbench) {
                    if !self.read.insert(story.id.clone()) { self.read.remove(&story.id); }
                }
            }
            KeyCode::Char('b') => {
                if let Some(story) = self.selected_story(&workbench) {
                    if !self.bookmarks.insert(story.id.clone()) { self.bookmarks.remove(&story.id); }
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
            KeyCode::Char('0') => {
                self.filter = NewsFilter::default();
                self.show_calendar = false;
                self.selected = 0;
            }
            KeyCode::Char('1') => { self.filter.region = Some("US".into()); self.selected = 0; }
            KeyCode::Char('2') => { self.filter.region = Some("EU".into()); self.selected = 0; }
            KeyCode::Char('3') => { self.filter.region = Some("AS".into()); self.selected = 0; }
            KeyCode::Char('s') => {
                if let Some(symbol) = self.selected_story(&workbench)
                    .and_then(|story| story.related_symbols.first())
                {
                    self.pending_intents.push(AppIntent::DispatchCommand {
                        command: format!("SEC {symbol} US"), origin: ID,
                    });
                }
            }
            _ => return false,
        }
        self.clamp_selection();
        true
    }

    fn poll_intents(&mut self) -> Vec<AppIntent> { std::mem::take(&mut self.pending_intents) }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let workbench = self.query.load_workbench();
        let visible = self.visible_indices(&workbench);
        let unread = workbench.stories.iter().filter(|story| !self.read.contains(&story.id)).count();
        let rows = Layout::vertical([Constraint::Length(3), Constraint::Min(10)]).split(area);
        let filter_label = format!(
            "REGION {}  TOPIC {}  SYMBOL {}  {}{}",
            self.filter.region.as_deref().unwrap_or("ALL"),
            self.filter.topic.as_deref().unwrap_or("ALL"),
            self.filter.symbol.as_deref().unwrap_or("ALL"),
            if self.filter.unread_only { "UNREAD  " } else { "" },
            if self.filter.bookmarked_only { "BOOKMARKED" } else { "" },
        );
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(format!(" {} RESULTS  {unread} UNREAD  ", visible.len()), Style::new().bg(AMBER).fg(BG).bold()),
                Span::styled(filter_label, INK),
                Span::styled("  0 RESET · 1/2/3 REGION · U UNREAD · M SAVED · E EVENTS · S SECURITY", MUTED),
            ])).block(terminal_block("NEWS", "FILTERS & WORKFLOW")),
            rows[0],
        );

        let columns = Layout::horizontal([
            Constraint::Percentage(39), Constraint::Percentage(43), Constraint::Percentage(18),
        ]).split(rows[1]);
        let items = visible.iter().enumerate().filter_map(|(ordinal, index)| {
            let story = workbench.stories.get(*index)?;
            let selected = ordinal == self.selected.min(visible.len().saturating_sub(1));
            let status = if self.bookmarks.contains(&story.id) { "★" }
                else if self.read.contains(&story.id) { " " } else { "●" };
            Some(ListItem::new(Line::from(vec![
                Span::styled(format!("{status} {} ", story.headline.time), MUTED),
                Span::styled(format!("{:<4}", story.headline.topic), AMBER),
                Span::styled(story.headline.title, if selected {
                    Style::new().bg(CYAN).fg(BG)
                } else { Style::new().fg(INK) }),
                Span::styled(format!(" {}", story.headline.region), MUTED),
            ])))
        }).collect::<Vec<_>>();
        frame.render_widget(List::new(items).block(terminal_block("TOP", "TOP NEWS")), columns[0]);

        if self.show_calendar {
            render_calendar(frame, columns[1], &workbench);
        } else if let Some(story) = self.selected_story(&workbench) {
            render_story(frame, columns[1], story);
        } else {
            frame.render_widget(
                Paragraph::new("NO STORIES MATCH THE ACTIVE FILTER\n\nPRESS 0 TO RESET")
                    .style(MUTED).block(terminal_block("READ", "STORY")),
                columns[1],
            );
        }

        let upcoming = workbench.events.iter().take(6).map(|event| Line::from(vec![
            Span::styled(format!("{} ", event.time), AMBER),
            Span::styled(format!("{} ", event.region), CYAN),
            Span::styled(event.event, INK),
        ])).collect::<Vec<_>>();
        frame.render_widget(
            Paragraph::new(upcoming).wrap(Wrap { trim: true }).block(terminal_block("ECO", "EVENTS · E EXPAND")),
            columns[2],
        );
    }
}

fn render_story(frame: &mut Frame, area: Rect, story: &NewsStory) {
    let mut lines = vec![
        Line::styled(format!("{} · {} · AUG 25, 2026", story.headline.topic, story.headline.time), AMBER),
        Line::styled(story.byline, MUTED),
        Line::raw(""),
        Line::styled(story.headline.title, Style::new().fg(INK).bold()),
        Line::raw(""),
        Line::styled(story.summary, YELLOW),
        Line::raw(""),
    ];
    for paragraph in story.body {
        lines.push(Line::raw(*paragraph));
        lines.push(Line::raw(""));
    }
    lines.push(Line::styled(format!("RELATED  {}", story.related_symbols.join("  ")), CYAN));
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: true }).block(terminal_block("READ", "STORY DETAIL")),
        area,
    );
}

fn render_calendar(frame: &mut Frame, area: Rect, workbench: &NewsWorkbench) {
    let rows = workbench.events.iter().map(|event| Row::new(vec![
        Cell::from(event.time), Cell::from(event.region), Cell::from(event.importance.label()),
        Cell::from(event.event), Cell::from(event.period), Cell::from(event.survey), Cell::from(event.prior),
    ]));
    let table = Table::new(rows, [
        Constraint::Length(6), Constraint::Length(4), Constraint::Length(6),
        Constraint::Min(24), Constraint::Length(8), Constraint::Length(9), Constraint::Length(9),
    ]).header(Row::new(["TIME", "REG", "IMP", "EVENT", "PERIOD", "SURVEY", "PRIOR"]).style(AMBER))
        .column_spacing(1).block(terminal_block("ECO", "ECONOMIC EVENT CALENDAR"));
    frame.render_widget(table, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::news::{Headline, NewsSnapshot};

    const HEADLINES: [Headline; 2] = [
        Headline { time: "16:00", topic: "TOP", title: "Markets gain", region: "US" },
        Headline { time: "14:00", topic: "TEC", title: "Chip rally", region: "AS" },
    ];
    struct StubQuery;
    impl NewsQuery for StubQuery {
        fn load_news(&self) -> NewsSnapshot { NewsSnapshot { headlines: &HEADLINES } }
    }

    #[test]
    fn command_options_scope_the_news_workbench() {
        let mut workspace = NewsWorkspace::new(Arc::new(StubQuery));
        workspace.handle_command(&CommandInvocation {
            function: "NEWS".into(), args: vec!["--region=AS".into(), "--topic=TEC".into()],
        });
        assert_eq!(workspace.visible_indices(&workspace.query.load_workbench()), vec![1]);
    }

    #[test]
    fn positional_subject_filters_by_linked_symbol() {
        let mut workspace = NewsWorkspace::new(Arc::new(StubQuery));
        workspace.handle_command(&CommandInvocation {
            function: "NEWS".into(), args: vec!["NVDA".into()],
        });
        assert_eq!(workspace.visible_indices(&workspace.query.load_workbench()), vec![1]);
    }

    #[test]
    fn bookmark_and_read_state_are_independent() {
        let mut workspace = NewsWorkspace::new(Arc::new(StubQuery));
        workspace.handle_key(KeyEvent::new(KeyCode::Char('b'), crossterm::event::KeyModifiers::NONE));
        workspace.handle_key(KeyEvent::new(KeyCode::Enter, crossterm::event::KeyModifiers::NONE));
        let story = workspace.query.load_workbench().stories.into_iter().next().unwrap();
        assert!(workspace.bookmarks.contains(&story.id));
        assert!(workspace.read.contains(&story.id));
    }

    #[test]
    fn security_shortcut_dispatches_linked_instrument() {
        let mut workspace = NewsWorkspace::new(Arc::new(StubQuery));
        workspace.handle_key(KeyEvent::new(KeyCode::Char('s'), crossterm::event::KeyModifiers::NONE));
        assert_eq!(workspace.poll_intents(), vec![AppIntent::DispatchCommand {
            command: "SEC SPY US".into(), origin: ID,
        }]);
    }
}
