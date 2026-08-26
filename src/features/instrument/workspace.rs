use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, MouseEvent};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Cell, Paragraph, Row, Table},
    Frame,
};

use crate::{
    app::{AppIntent, CommandInvocation, Workspace, WorkspaceDescriptor},
    ui::{
        components::terminal_block,
        scroll_key, table_row_at,
        theme::{AMBER, BG, CYAN, INK, MUTED},
    },
};

use super::{Instrument, InstrumentSearch, ID};

pub struct InstrumentSearchWorkspace {
    query: Arc<dyn InstrumentSearch>,
    search_term: String,
    results: Vec<Instrument>,
    selected: usize,
    pending_intents: Vec<AppIntent>,
    catalog_revision: u64,
}

impl InstrumentSearchWorkspace {
    pub fn new(query: Arc<dyn InstrumentSearch>) -> Self {
        let results = query.search("", 12);
        let catalog_revision = query.revision();
        Self {
            query,
            search_term: String::new(),
            results,
            selected: 0,
            pending_intents: Vec::new(),
            catalog_revision,
        }
    }

    fn refresh(&mut self, query: String) {
        self.search_term = query;
        self.results = self.query.search(&self.search_term, 12);
        self.selected = 0;
    }

    fn move_selection(&mut self, delta: isize) {
        if self.results.is_empty() {
            self.selected = 0;
            return;
        }
        self.selected = self
            .selected
            .saturating_add_signed(delta)
            .min(self.results.len() - 1);
    }
}

impl Workspace for InstrumentSearchWorkspace {
    fn descriptor(&self) -> WorkspaceDescriptor {
        WorkspaceDescriptor {
            id: ID,
            label: "FIND",
            hotkey: 'f',
            commands: &["FIND", "SEARCH"],
        }
    }

    fn is_favorite(&self) -> bool {
        true
    }

    fn handle_command(&mut self, invocation: &CommandInvocation) -> bool {
        self.refresh(invocation.args.join(" "));
        true
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_selection(-1);
                true
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_selection(1);
                true
            }
            KeyCode::Enter => {
                if let Some(instrument) = self.results.get(self.selected) {
                    self.pending_intents.push(AppIntent::DispatchCommand {
                        command: format!("SEC {}", instrument.terminal_subject()),
                        origin: ID,
                    });
                }
                true
            }
            KeyCode::F(9) => {
                self.query.request_refresh();
                true
            }
            _ => false,
        }
    }

    fn handle_mouse(&mut self, event: MouseEvent, area: Rect) -> bool {
        let rows = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(3),
        ])
        .split(area);
        if crate::ui::is_primary_click(event, rows[0]) {
            return self.handle_key(KeyEvent::new(
                KeyCode::F(9),
                crossterm::event::KeyModifiers::NONE,
            ));
        }
        if let Some(index) = table_row_at(event, rows[1], self.results.len()) {
            self.selected = index;
            return true;
        }
        if crate::ui::is_primary_click(event, rows[2]) {
            let open_start = rows[2]
                .x
                .saturating_add(" ↑↓/JK SELECT   ".chars().count() as u16);
            let open_width = " ENTER OPEN SECURITY   ".chars().count() as u16;
            if event.column >= open_start && event.column < open_start.saturating_add(open_width) {
                return self.handle_key(KeyEvent::new(
                    KeyCode::Enter,
                    crossterm::event::KeyModifiers::NONE,
                ));
            }
            return true;
        }
        if let Some(key) = scroll_key(event, rows[1]) {
            return self.handle_key(key);
        }
        false
    }

    fn poll_intents(&mut self) -> Vec<AppIntent> {
        let revision = self.query.revision();
        if revision != self.catalog_revision {
            self.catalog_revision = revision;
            self.results = self.query.search(&self.search_term, 12);
            self.selected = self.selected.min(self.results.len().saturating_sub(1));
        }
        std::mem::take(&mut self.pending_intents)
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let rows = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(3),
        ])
        .split(area);

        let query = if self.search_term.is_empty() {
            "RECENT AND MAJOR INSTRUMENTS"
        } else {
            self.search_term.as_str()
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" QUERY  ", Style::new().bg(AMBER).fg(BG).bold()),
                Span::styled(format!(" {query}"), INK),
                Span::styled(format!("   {} MATCHES", self.results.len()), MUTED),
                Span::styled(format!("   {}", self.query.status()), MUTED),
            ]))
            .block(terminal_block("FIND", "INSTRUMENT MASTER")),
            rows[0],
        );

        let result_rows = self.results.iter().enumerate().map(|(index, instrument)| {
            let style = if index == self.selected {
                Style::new().bg(CYAN).fg(BG).bold()
            } else {
                Style::new().fg(INK)
            };
            Row::new(vec![
                Cell::from(instrument.symbol.clone()),
                Cell::from(instrument.name.clone()),
                Cell::from(instrument.kind.label()),
                Cell::from(instrument.venue.clone()),
                Cell::from(instrument.currency.clone()),
                Cell::from(instrument.id.as_str().to_owned()),
            ])
            .style(style)
        });
        frame.render_widget(
            Table::new(
                result_rows,
                [
                    Constraint::Length(12),
                    Constraint::Percentage(31),
                    Constraint::Length(12),
                    Constraint::Length(10),
                    Constraint::Length(10),
                    Constraint::Min(20),
                ],
            )
            .header(
                Row::new(["SYMBOL", "NAME", "TYPE", "VENUE", "CCY", "CANONICAL ID"])
                    .style(Style::new().fg(AMBER).bold())
                    .bottom_margin(1),
            )
            .column_spacing(1)
            .block(terminal_block("RES", "SEARCH RESULTS")),
            rows[1],
        );

        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" ↑↓/JK ", AMBER),
                Span::styled("SELECT   ", MUTED),
                Span::styled(" ENTER ", AMBER),
                Span::styled("OPEN SECURITY   ", MUTED),
                Span::styled(" / FIND <QUERY> ", AMBER),
                Span::styled("NEW SEARCH   ", MUTED),
                Span::styled(" F9/CLICK HEADER ", AMBER),
                Span::styled("REFRESH LIVE MASTER", MUTED),
            ])),
            rows[2],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    struct StubSearch;

    impl InstrumentSearch for StubSearch {
        fn search(&self, query: &str, _limit: usize) -> Vec<Instrument> {
            vec![Instrument {
                id: super::super::InstrumentId::new("us:xnas:msft"),
                symbol: if query.is_empty() { "AAPL" } else { "MSFT" }.to_owned(),
                name: "Test".to_owned(),
                venue: "US".to_owned(),
                currency: "USD".to_owned(),
                kind: super::super::InstrumentKind::Equity,
            }]
        }
    }

    #[test]
    fn command_searches_and_enter_opens_security() {
        let mut workspace = InstrumentSearchWorkspace::new(Arc::new(StubSearch));
        workspace.handle_command(&CommandInvocation {
            function: "FIND".to_owned(),
            args: vec!["MICROSOFT".to_owned()],
        });
        assert!(workspace.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));
        assert_eq!(
            workspace.poll_intents(),
            vec![AppIntent::DispatchCommand {
                command: "SEC MSFT US".to_owned(),
                origin: ID,
            }]
        );
    }
}
