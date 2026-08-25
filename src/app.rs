use std::{io, time::Duration};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::DefaultTerminal;

use crate::ui;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    #[default]
    Overview,
    Markets,
    Security,
    Portfolio,
    News,
}

impl Screen {
    pub const ALL: [Self; 5] = [Self::Overview, Self::Markets, Self::Security, Self::Portfolio, Self::News];
    pub const fn label(self) -> &'static str { match self { Self::Overview => "OVERVIEW", Self::Markets => "MARKETS", Self::Security => "SECURITY", Self::Portfolio => "PORTFOLIO", Self::News => "NEWS" } }
    pub const fn key(self) -> char { match self { Self::Overview => 'g', Self::Markets => 'm', Self::Security => 's', Self::Portfolio => 'p', Self::News => 'n' } }
}

#[derive(Debug, Default)]
pub struct App {
    pub screen: Screen,
    pub command: String,
    pub selected_news: usize,
    pub selected_period: usize,
    pub ticks: u64,
    pub should_quit: bool,
}

impl App {
    pub fn run(mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        while !self.should_quit {
            terminal.draw(|frame| ui::render(frame, &self))?;
            if event::poll(Duration::from_millis(180))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press { self.on_key(key); }
                }
            }
            self.ticks = self.ticks.wrapping_add(1);
        }
        Ok(())
    }

    fn on_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') { self.should_quit = true; return; }
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('g') => self.screen = Screen::Overview,
            KeyCode::Char('m') => self.screen = Screen::Markets,
            KeyCode::Char('s') => self.screen = Screen::Security,
            KeyCode::Char('p') => self.screen = Screen::Portfolio,
            KeyCode::Char('n') => self.screen = Screen::News,
            KeyCode::Char(c @ '1'..='8') if self.screen == Screen::Overview => self.selected_period = c as usize - '1' as usize,
            KeyCode::Down | KeyCode::Char('j') if self.screen == Screen::News => self.selected_news = (self.selected_news + 1).min(9),
            KeyCode::Up | KeyCode::Char('k') if self.screen == Screen::News => self.selected_news = self.selected_news.saturating_sub(1),
            KeyCode::Backspace => { self.command.pop(); }
            KeyCode::Enter => self.execute_command(),
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => self.command.push(c.to_ascii_uppercase()),
            _ => {}
        }
    }

    fn execute_command(&mut self) {
        let value = self.command.as_str();
        self.screen = if value.contains("MARKET") { Screen::Markets } else if value.contains("AAPL") || value.contains("SEC") { Screen::Security } else if value.contains("PORT") { Screen::Portfolio } else if value.contains("NEWS") { Screen::News } else { self.screen };
        self.command.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn keyboard_switches_screens() { let mut app = App::default(); app.on_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE)); assert_eq!(app.screen, Screen::Portfolio); }
    #[test] fn commands_switch_screens() { let mut app = App { command: "AAPL US".into(), ..App::default() }; app.execute_command(); assert_eq!(app.screen, Screen::Security); assert!(app.command.is_empty()); }
}

