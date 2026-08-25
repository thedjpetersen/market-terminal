use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{List, ListItem, Paragraph, Wrap},
    Frame,
};

use crate::{
    app::{Workspace, WorkspaceDescriptor},
    ui::{
        components::{render_pairs, terminal_block},
        theme::{AMBER, BG, CYAN, INK, MUTED, YELLOW},
    },
};

use super::{NewsQuery, ID};

pub struct NewsWorkspace {
    query: Arc<dyn NewsQuery>,
    selected: usize,
}

impl NewsWorkspace {
    pub fn new(query: Arc<dyn NewsQuery>) -> Self { Self { query, selected: 0 } }
}

impl Workspace for NewsWorkspace {
    fn descriptor(&self) -> WorkspaceDescriptor {
        WorkspaceDescriptor { id: ID, label: "NEWS", hotkey: 'n', commands: &["NEWS", "TOP", "HEADLINES"] }
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        let length = self.query.load_news().headlines.len();
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                self.selected = (self.selected + 1).min(length.saturating_sub(1));
                true
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected = self.selected.saturating_sub(1);
                true
            }
            _ => false,
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let snapshot = self.query.load_news();
        if snapshot.headlines.is_empty() { return; }
        let selected = self.selected.min(snapshot.headlines.len() - 1);
        let columns = Layout::horizontal([
            Constraint::Percentage(39), Constraint::Percentage(43), Constraint::Percentage(18),
        ]).split(area);
        let items = snapshot.headlines.iter().enumerate().map(|(index, headline)| {
            ListItem::new(Line::from(vec![
                Span::styled(format!("{} ", headline.time), MUTED),
                Span::styled(format!("{:<4}", headline.topic), AMBER),
                Span::styled(
                    headline.title,
                    if index == selected { Style::new().bg(CYAN).fg(BG) } else { Style::new().fg(INK) },
                ),
                Span::styled(format!(" {}", headline.region), MUTED),
            ]))
        }).collect::<Vec<_>>();
        frame.render_widget(List::new(items).block(terminal_block("TOP", "TOP NEWS")), columns[0]);

        let headline = snapshot.headlines[selected];
        let story = vec![
            Line::styled(format!("{} · {} · AUG 25, 2026", headline.topic, headline.time), AMBER),
            Line::raw(""),
            Line::styled(headline.title, Style::new().fg(INK)),
            Line::raw(""),
            Line::raw("Markets moved decisively into positive territory as investors weighed resilient corporate earnings against a shifting interest-rate outlook."),
            Line::raw(""),
            Line::raw("Technology shares led the advance while market breadth remained constructive."),
            Line::raw(""),
            Line::styled("“The move is broader than a single theme.”", YELLOW),
            Line::raw(""),
            Line::raw("Attention now turns to inflation data and central-bank guidance."),
        ];
        frame.render_widget(
            Paragraph::new(story).wrap(Wrap { trim: true }).block(terminal_block("READ", "STORY")),
            columns[1],
        );

        let right = Layout::vertical([Constraint::Percentage(52), Constraint::Percentage(48)]).split(columns[2]);
        render_pairs(frame, right[0], "MOST", "MOST READ", &[
            ["1", "CHIP RALLY"], ["2", "FED PATH"], ["3", "OIL OUTLOOK"], ["4", "DOLLAR FALLS"],
        ]);
        render_pairs(frame, right[1], "MOV", "LIVE MOVERS", &[
            ["NVDA", "+4.21%"], ["META", "+2.85%"], ["AMZN", "+2.31%"], ["MRNA", "−4.32%"],
        ]);
    }
}
