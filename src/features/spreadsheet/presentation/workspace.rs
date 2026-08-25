use std::{cell::Cell as StateCell, collections::HashMap, sync::Arc};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell as TableCell, Paragraph, Row, Table},
    Frame,
};

use crate::{
    app::{CommandInvocation, Workspace, WorkspaceDescriptor},
    ui::{
        components::terminal_block,
        theme::{AMBER, BG, CYAN, FOOTER_BG, INK, MUTED, NAV_BG, RED},
    },
};

use super::super::{
    domain::{CellAddress, CellValue, MAX_COLUMNS, MAX_ROWS},
    MarketDataRequest, Spreadsheet, SpreadsheetMarketData, ID,
};

const CELL_WIDTH: u16 = 12;
const ROW_HEADER_WIDTH: u16 = 5;

#[derive(Debug)]
struct EditSession {
    characters: Vec<char>,
    cursor: usize,
}

impl EditSession {
    fn new(value: &str) -> Self {
        let characters = value.chars().collect::<Vec<_>>();
        let cursor = characters.len();
        Self { characters, cursor }
    }

    fn text(&self) -> String { self.characters.iter().collect() }

    fn insert(&mut self, character: char) {
        self.characters.insert(self.cursor, character);
        self.cursor += 1;
    }

    fn backspace(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.characters.remove(self.cursor);
        }
    }

    fn delete(&mut self) {
        if self.cursor < self.characters.len() {
            self.characters.remove(self.cursor);
        }
    }
}

pub struct SpreadsheetWorkspace {
    spreadsheet: Spreadsheet,
    market_data: Arc<dyn SpreadsheetMarketData>,
    cursor: CellAddress,
    first_column: u8,
    first_row: u16,
    visible_columns: StateCell<u8>,
    visible_rows: StateCell<u16>,
    edit: Option<EditSession>,
    status: String,
}

impl SpreadsheetWorkspace {
    pub fn new(market_data: Arc<dyn SpreadsheetMarketData>) -> Self {
        let mut workspace = Self {
            spreadsheet: Spreadsheet::new(),
            market_data,
            cursor: CellAddress::new(1, 1).expect("A1 is in bounds"),
            first_column: 1,
            first_row: 1,
            visible_columns: StateCell::new(8),
            visible_rows: StateCell::new(18),
            edit: None,
            status: String::new(),
        };
        workspace.seed_demo_workbook();
        workspace
    }

    fn seed_demo_workbook(&mut self) {
        for (address, raw) in [
            ("A1", "SECURITY"),
            ("B1", "LAST PRICE"),
            ("C1", "DAY %"),
            ("D1", "SHARES"),
            ("E1", "MARKET VALUE"),
            ("A7", "PORTFOLIO VALUE"),
            ("E7", "=SUM(E2:E5)"),
            ("A9", "MODEL INPUTS"),
            ("A10", "Revenue"),
            ("B10", "1250"),
            ("A11", "Growth"),
            ("B11", "0.12"),
            ("A12", "Forward revenue"),
            ("B12", "=B10*(1+B11)"),
        ] {
            self.spreadsheet
                .set_cell(address, raw)
                .expect("demo seed addresses are valid");
        }
        self.refresh_market_data();
        self.spreadsheet.clear_history();
        self.status = "READY · LIVE FIELDS LOADED".to_owned();
    }

    fn refresh_market_data(&mut self) {
        let securities = [
            ("SPY US Equity", 250.0),
            ("QQQ US Equity", 180.0),
            ("AVGO US Equity", 120.0),
            ("NVDA US Equity", 300.0),
        ];
        let requests = securities
            .iter()
            .flat_map(|(security, _)| {
                [
                    MarketDataRequest::new(*security, "PX_LAST"),
                    MarketDataRequest::new(*security, "CHG_PCT_1D"),
                ]
            })
            .collect::<Vec<_>>();
        let values = self
            .market_data
            .load_batch(&requests)
            .into_iter()
            .map(|point| ((point.request.security, point.request.field), point.value))
            .collect::<HashMap<_, _>>();

        let mut cells = Vec::new();
        for (index, (security, shares)) in securities.into_iter().enumerate() {
            let row = index + 2;
            let price = values
                .get(&(security.to_owned(), "PX_LAST".to_owned()))
                .copied()
                .unwrap_or_default();
            let change = values
                .get(&(security.to_owned(), "CHG_PCT_1D".to_owned()))
                .copied()
                .unwrap_or_default();
            for (column, raw) in [
                ('A', security.to_owned()),
                ('B', price.to_string()),
                ('C', change.to_string()),
                ('D', shares.to_string()),
                ('E', format!("=B{row}*D{row}")),
            ] {
                cells.push((format!("{column}{row}"), raw));
            }
        }
        self.spreadsheet
            .set_cells(cells)
            .expect("market-data seed addresses are valid");
    }

    fn selected_address(&self) -> String { self.cursor.to_string() }

    fn selected_raw(&self) -> String {
        self.spreadsheet
            .cell(&self.selected_address())
            .map(|cell| cell.raw)
            .unwrap_or_default()
    }

    fn begin_edit(&mut self, initial: Option<&str>) {
        let value = initial.map(ToOwned::to_owned).unwrap_or_else(|| self.selected_raw());
        self.edit = Some(EditSession::new(&value));
        self.status = "EDIT · ENTER TO COMMIT · ESC TO CANCEL".to_owned();
    }

    fn commit_edit(&mut self) {
        let Some(edit) = self.edit.take() else { return };
        let address = self.selected_address();
        match self.spreadsheet.set_cell(&address, edit.text()) {
            Ok(value) => self.status = format!("{address} = {value}"),
            Err(error) => self.status = format!("ERROR · {error}"),
        }
    }

    fn cancel_edit(&mut self) {
        self.edit = None;
        self.status = "EDIT CANCELLED".to_owned();
    }

    fn move_cursor(&mut self, column_delta: i8, row_delta: i8) {
        let column = (i16::from(self.cursor.column()) + i16::from(column_delta))
            .clamp(1, i16::from(MAX_COLUMNS)) as u8;
        let row = (i32::from(self.cursor.row()) + i32::from(row_delta))
            .clamp(1, i32::from(MAX_ROWS)) as u16;
        self.cursor = CellAddress::new(column, row).expect("clamped cursor is in bounds");
        self.keep_cursor_visible();
        self.status = format!("SELECTED {}", self.cursor);
    }

    fn keep_cursor_visible(&mut self) {
        let columns = self.visible_columns.get().max(1);
        let rows = self.visible_rows.get().max(1);
        if self.cursor.column() < self.first_column {
            self.first_column = self.cursor.column();
        } else if self.cursor.column() >= self.first_column.saturating_add(columns) {
            self.first_column = self.cursor.column().saturating_sub(columns - 1).max(1);
        }
        if self.cursor.row() < self.first_row {
            self.first_row = self.cursor.row();
        } else if self.cursor.row() >= self.first_row.saturating_add(rows) {
            self.first_row = self.cursor.row().saturating_sub(rows - 1).max(1);
        }
    }

    fn handle_edit_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.cancel_edit(),
            KeyCode::Enter => self.commit_edit(),
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(edit) = &mut self.edit { edit.insert(character); }
            }
            KeyCode::Backspace => {
                if let Some(edit) = &mut self.edit { edit.backspace(); }
            }
            KeyCode::Delete => {
                if let Some(edit) = &mut self.edit { edit.delete(); }
            }
            KeyCode::Left => {
                if let Some(edit) = &mut self.edit { edit.cursor = edit.cursor.saturating_sub(1); }
            }
            KeyCode::Right => {
                if let Some(edit) = &mut self.edit {
                    edit.cursor = (edit.cursor + 1).min(edit.characters.len());
                }
            }
            KeyCode::Home => {
                if let Some(edit) = &mut self.edit { edit.cursor = 0; }
            }
            KeyCode::End => {
                if let Some(edit) = &mut self.edit { edit.cursor = edit.characters.len(); }
            }
            _ => {}
        }
    }

    fn clear_selected(&mut self) {
        let address = self.selected_address();
        if let Err(error) = self.spreadsheet.clear_cell(&address) {
            self.status = format!("ERROR · {error}");
        } else {
            self.status = format!("CLEARED {address}");
        }
    }

    fn undo(&mut self) {
        self.status = if self.spreadsheet.undo() {
            format!("UNDID CHANGE · {}", self.spreadsheet.workbook().active_sheet().name())
        } else {
            "NOTHING TO UNDO".to_owned()
        };
    }

    fn redo(&mut self) {
        self.status = if self.spreadsheet.redo() {
            format!("REDID CHANGE · {}", self.spreadsheet.workbook().active_sheet().name())
        } else {
            "NOTHING TO REDO".to_owned()
        };
    }

    fn select_sheet(&mut self, name: &str) {
        match self.spreadsheet.select_sheet(name) {
            Ok(()) => {
                self.cursor = CellAddress::new(1, 1).expect("A1 is in bounds");
                self.first_column = 1;
                self.first_row = 1;
                self.status = format!("SELECTED SHEET {}", self.spreadsheet.workbook().active_sheet().name());
            }
            Err(error) => self.status = format!("ERROR · {error}"),
        }
    }

    fn next_sheet_name(&self) -> String {
        let mut number = self.spreadsheet.workbook().sheet_count() + 1;
        loop {
            let name = format!("Sheet{number}");
            if self.spreadsheet.workbook().sheet(&name).is_none() {
                return name;
            }
            number += 1;
        }
    }

    fn add_sheet(&mut self, requested_name: Option<String>) {
        let name = requested_name
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| self.next_sheet_name());
        match self.spreadsheet.add_sheet(name.clone()) {
            Ok(_) => self.select_sheet(&name),
            Err(error) => self.status = format!("ERROR · {error}"),
        }
    }

    fn select_adjacent_sheet(&mut self, next: bool) {
        if next {
            self.spreadsheet.select_next_sheet();
        } else {
            self.spreadsheet.select_previous_sheet();
        }
        self.cursor = CellAddress::new(1, 1).expect("A1 is in bounds");
        self.first_column = 1;
        self.first_row = 1;
        self.status = format!("SELECTED SHEET {}", self.spreadsheet.workbook().active_sheet().name());
    }

    fn render_formula_bar(&self, frame: &mut Frame, area: Rect) {
        let raw = self.edit.as_ref().map(EditSession::text).unwrap_or_else(|| self.selected_raw());
        let editing = self.edit.is_some();
        let cursor = if let Some(edit) = &self.edit {
            let before = edit.characters.iter().take(edit.cursor).collect::<String>();
            format!("{before}▌{}", edit.characters.iter().skip(edit.cursor).collect::<String>())
        } else {
            raw
        };
        let border = if editing { CYAN } else { AMBER };
        let line = Line::from(vec![
            Span::styled(format!(" {:<5} ", self.cursor), Style::new().bg(AMBER).fg(BG).bold()),
            Span::styled(" ƒx  ", CYAN),
            Span::styled(cursor, if editing { Style::new().fg(CYAN) } else { Style::new().fg(INK) }),
        ]);
        frame.render_widget(
            Paragraph::new(line).block(Block::new().borders(Borders::ALL).border_style(border)),
            area,
        );
    }

    fn render_grid(&self, frame: &mut Frame, area: Rect) {
        let available_width = area.width.saturating_sub(ROW_HEADER_WIDTH + 3);
        let columns = (available_width / (CELL_WIDTH + 1))
            .max(1)
            .min(u16::from(MAX_COLUMNS - self.first_column + 1)) as u8;
        let rows = area.height.saturating_sub(3).max(1).min(MAX_ROWS - self.first_row + 1);
        self.visible_columns.set(columns);
        self.visible_rows.set(rows);
        let visible_values = self
            .spreadsheet
            .visible_region(self.first_column, self.first_row, columns, rows)
            .expect("clamped viewport is in bounds")
            .into_iter()
            .map(|cell| (cell.address, cell.value))
            .collect::<HashMap<_, _>>();

        let mut widths = vec![Constraint::Length(ROW_HEADER_WIDTH)];
        widths.extend((0..columns).map(|_| Constraint::Length(CELL_WIDTH)));

        let mut header_cells = vec![TableCell::from("").style(Style::new().bg(NAV_BG))];
        for column in self.first_column..self.first_column + columns {
            let name = char::from(b'A' + column - 1).to_string();
            let style = if column == self.cursor.column() {
                Style::new().bg(AMBER).fg(BG).bold()
            } else {
                Style::new().fg(AMBER).add_modifier(Modifier::BOLD)
            };
            header_cells.push(TableCell::from(name).style(style));
        }
        let header = Row::new(header_cells).style(Style::new().bg(NAV_BG));

        let table_rows = (self.first_row..self.first_row + rows)
            .map(|row| {
                let row_style = if row == self.cursor.row() {
                    Style::new().fg(AMBER).add_modifier(Modifier::BOLD)
                } else {
                    Style::new().fg(MUTED)
                };
                let mut cells = vec![TableCell::from(format!("{row:>4}")).style(row_style)];
                for column in self.first_column..self.first_column + columns {
                    let address = CellAddress::new(column, row).expect("render region is in bounds");
                    let value = visible_values
                        .get(&address)
                        .map(format_value)
                        .unwrap_or_default();
                    let selected = address == self.cursor;
                    let style = if selected {
                        Style::new().bg(CYAN).fg(BG).add_modifier(Modifier::BOLD)
                    } else {
                        value_style(&value)
                    };
                    cells.push(TableCell::from(truncate(&value, CELL_WIDTH as usize)).style(style));
                }
                Row::new(cells).style(Style::new().bg(BG))
            })
            .collect::<Vec<_>>();

        let table = Table::new(table_rows, widths)
            .header(header)
            .column_spacing(1)
            .block(terminal_block("XL", "WORKBOOK · 26 × 100"));
        frame.render_widget(table, area);
    }

    fn render_tabs(&self, frame: &mut Frame, area: Rect) {
        let workbook = self.spreadsheet.workbook();
        let mut tabs = vec![Span::styled(" + ", Style::new().fg(AMBER).bold())];
        for (index, sheet) in workbook.sheets().iter().enumerate() {
            let style = if index == workbook.active_sheet_index() {
                Style::new().bg(CYAN).fg(BG).bold()
            } else {
                Style::new().fg(MUTED)
            };
            tabs.push(Span::styled(format!(" {}:{} ", index + 1, sheet.name()), style));
            tabs.push(Span::raw(" "));
        }
        frame.render_widget(
            Paragraph::new(Line::from(tabs)).style(Style::new().bg(NAV_BG)),
            area,
        );
    }

    fn render_status(&self, frame: &mut Frame, area: Rect) {
        let mode = if self.edit.is_some() { "EDIT" } else { "NAV" };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(format!(" {mode} "), Style::new().bg(AMBER).fg(BG).bold()),
                Span::styled(format!(" {}   ", self.status), INK),
                Span::styled("CTRL-Z/Y UNDO/REDO  CTRL-PGUP/DN SHEETS  SHIFT-F11 NEW  F2 EDIT", MUTED),
            ]))
            .style(Style::new().bg(FOOTER_BG)),
            area,
        );
    }
}

impl Workspace for SpreadsheetWorkspace {
    fn descriptor(&self) -> WorkspaceDescriptor {
        WorkspaceDescriptor {
            id: ID,
            label: "SHEET",
            hotkey: '\0',
            commands: &["SHEET", "WORKBOOK", "XL"],
        }
    }

    fn hotkey(&self) -> Option<char> { None }

    fn is_favorite(&self) -> bool { true }

    fn handle_command(&mut self, invocation: &CommandInvocation) -> bool {
        let Some(first) = invocation.args.first() else { return true };
        if let Ok(address) = first.parse::<CellAddress>() {
            self.cursor = address;
            self.first_column = address.column();
            self.first_row = address.row();
            self.status = format!("SELECTED {address}");
            return true;
        }

        let operation = first.to_ascii_uppercase();
        let name = invocation.args.get(1..).unwrap_or_default().join(" ");
        match operation.as_str() {
            "ADD" | "NEW" => self.add_sheet((!name.is_empty()).then_some(name)),
            "NEXT" => self.select_adjacent_sheet(true),
            "PREV" | "PREVIOUS" => self.select_adjacent_sheet(false),
            "SELECT" if name.is_empty() => self.status = "ERROR · SHEET SELECT REQUIRES A NAME".to_owned(),
            "SELECT" => self.select_sheet(&name),
            "RENAME" if name.is_empty() => self.status = "ERROR · SHEET RENAME REQUIRES A NAME".to_owned(),
            "RENAME" => match self.spreadsheet.rename_active_sheet(name) {
                Ok(()) => {
                    self.status = format!("RENAMED SHEET {}", self.spreadsheet.workbook().active_sheet().name());
                }
                Err(error) => self.status = format!("ERROR · {error}"),
            },
            "DELETE" | "REMOVE" => match self.spreadsheet.remove_active_sheet() {
                Ok(()) => {
                    self.status = format!("REMOVED SHEET · NOW ON {}", self.spreadsheet.workbook().active_sheet().name());
                }
                Err(error) => self.status = format!("ERROR · {error}"),
            },
            _ => self.select_sheet(&invocation.args.join(" ")),
        }
        true
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        if self.edit.is_some() {
            self.handle_edit_key(key);
            return true;
        }
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        match key.code {
            KeyCode::Char('z') if control && shift => self.redo(),
            KeyCode::Char('Z') if control && shift => self.redo(),
            KeyCode::Char('z') if control => self.undo(),
            KeyCode::Char('y') if control => self.redo(),
            KeyCode::PageDown if control => self.select_adjacent_sheet(true),
            KeyCode::PageUp if control => self.select_adjacent_sheet(false),
            KeyCode::F(11) if shift => self.add_sheet(None),
            KeyCode::Up | KeyCode::Char('k') => self.move_cursor(0, -1),
            KeyCode::Down | KeyCode::Char('j') => self.move_cursor(0, 1),
            KeyCode::Left | KeyCode::Char('h') => self.move_cursor(-1, 0),
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Tab => self.move_cursor(1, 0),
            KeyCode::BackTab => self.move_cursor(-1, 0),
            KeyCode::Enter | KeyCode::F(2) => self.begin_edit(None),
            KeyCode::Char('=') => self.begin_edit(Some("=")),
            KeyCode::Delete => self.clear_selected(),
            KeyCode::F(9) => {
                self.refresh_market_data();
                self.status = "MARKET DATA REFRESHED".to_owned();
            }
            _ => return false,
        }
        true
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let regions = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(area);
        self.render_formula_bar(frame, regions[0]);
        self.render_grid(frame, regions[1]);
        self.render_tabs(frame, regions[2]);
        self.render_status(frame, regions[3]);
    }
}

fn format_value(value: &CellValue) -> String {
    match value {
        CellValue::Number(number) => {
            if number.fract().abs() < f64::EPSILON {
                format!("{number:.0}")
            } else {
                format!("{number:.2}")
            }
        }
        _ => value.to_string(),
    }
}

fn value_style(value: &str) -> Style {
    if value.starts_with('#') {
        Style::new().fg(RED)
    } else {
        Style::new().fg(INK)
    }
}

fn truncate(value: &str, width: usize) -> String {
    let max = width.saturating_sub(1);
    let mut characters = value.chars();
    let shortened = characters.by_ref().take(max).collect::<String>();
    if characters.next().is_some() && max > 0 {
        format!("{}…", shortened.chars().take(max.saturating_sub(1)).collect::<String>())
    } else {
        shortened
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::spreadsheet::MarketDataPoint;

    struct StubMarketData;

    impl SpreadsheetMarketData for StubMarketData {
        fn load_batch(&self, requests: &[MarketDataRequest]) -> Vec<MarketDataPoint> {
            requests
                .iter()
                .filter_map(|request| {
                    let value = match (request.security.as_str(), request.field.as_str()) {
                        ("SPY US Equity", "PX_LAST") => 530.47,
                        ("QQQ US Equity", "PX_LAST") => 455.18,
                        ("AVGO US Equity", "PX_LAST") => 176.42,
                        ("NVDA US Equity", "PX_LAST") => 119.31,
                        (_, "CHG_PCT_1D") => 1.0,
                        _ => return None,
                    };
                    Some(MarketDataPoint {
                        request: request.clone(),
                        value,
                    })
                })
                .collect()
        }
    }

    fn workspace() -> SpreadsheetWorkspace {
        SpreadsheetWorkspace::new(Arc::new(StubMarketData))
    }

    fn key(code: KeyCode) -> KeyEvent { KeyEvent::new(code, KeyModifiers::NONE) }

    fn modified_key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn seeds_market_data_and_formulas() {
        let workspace = workspace();
        assert_eq!(workspace.spreadsheet.cell("A2").unwrap().raw, "SPY US Equity");
        assert_eq!(workspace.spreadsheet.cell("E2").unwrap().value, CellValue::Number(132_617.5));
        assert!(matches!(workspace.spreadsheet.cell("E7").unwrap().value, CellValue::Number(_)));
    }

    #[test]
    fn navigation_is_bounded_and_scrolls_the_viewport() {
        let mut workspace = workspace();
        workspace.visible_rows.set(3);
        for _ in 0..5 { assert!(workspace.handle_key(key(KeyCode::Down))); }
        assert_eq!(workspace.cursor.to_string(), "A6");
        assert_eq!(workspace.first_row, 4);
        workspace.handle_key(key(KeyCode::Left));
        assert_eq!(workspace.cursor.to_string(), "A6");
    }

    #[test]
    fn edit_mode_captures_letters_and_commits_formula() {
        let mut workspace = workspace();
        assert!(workspace.handle_key(key(KeyCode::Char('='))));
        for character in "SUM(B2:B5)".chars() {
            assert!(workspace.handle_key(key(KeyCode::Char(character))));
        }
        assert!(workspace.handle_key(key(KeyCode::Enter)));
        assert!(workspace.edit.is_none());
        assert!(matches!(workspace.spreadsheet.cell("A1").unwrap().value, CellValue::Number(_)));
    }

    #[test]
    fn escape_cancels_without_changing_the_cell() {
        let mut workspace = workspace();
        let original = workspace.spreadsheet.cell("A1").unwrap().raw;
        workspace.handle_key(key(KeyCode::F(2)));
        workspace.handle_key(key(KeyCode::Char('x')));
        assert!(workspace.handle_key(key(KeyCode::Esc)));
        assert_eq!(workspace.spreadsheet.cell("A1").unwrap().raw, original);
    }

    #[test]
    fn command_argument_selects_a_cell() {
        let mut workspace = workspace();
        workspace.handle_command(&CommandInvocation {
            function: "SHEET".to_owned(),
            args: vec!["Z100".to_owned()],
        });
        assert_eq!(workspace.cursor.to_string(), "Z100");
    }

    #[test]
    fn keyboard_undo_and_redo_restore_committed_edits() {
        let mut workspace = workspace();
        let original = workspace.spreadsheet.cell("A1").unwrap().raw;
        workspace.handle_key(key(KeyCode::F(2)));
        for _ in 0..original.chars().count() {
            workspace.handle_key(key(KeyCode::Backspace));
        }
        workspace.handle_key(key(KeyCode::Char('x')));
        workspace.handle_key(key(KeyCode::Enter));
        assert_eq!(workspace.spreadsheet.cell("A1").unwrap().raw, "x");

        assert!(workspace.handle_key(modified_key(
            KeyCode::Char('z'),
            KeyModifiers::CONTROL,
        )));
        assert_eq!(workspace.spreadsheet.cell("A1").unwrap().raw, original);
        assert!(workspace.handle_key(modified_key(
            KeyCode::Char('y'),
            KeyModifiers::CONTROL,
        )));
        assert_eq!(workspace.spreadsheet.cell("A1").unwrap().raw, "x");
    }

    #[test]
    fn keyboard_creates_and_cycles_workbook_tabs() {
        let mut workspace = workspace();
        assert!(workspace.handle_key(modified_key(
            KeyCode::F(11),
            KeyModifiers::SHIFT,
        )));
        assert_eq!(workspace.spreadsheet.workbook().sheet_count(), 2);
        assert_eq!(workspace.spreadsheet.workbook().active_sheet().name(), "Sheet2");

        assert!(workspace.handle_key(modified_key(
            KeyCode::PageUp,
            KeyModifiers::CONTROL,
        )));
        assert_eq!(workspace.spreadsheet.workbook().active_sheet().name(), "Sheet1");
        assert!(workspace.handle_key(modified_key(
            KeyCode::PageDown,
            KeyModifiers::CONTROL,
        )));
        assert_eq!(workspace.spreadsheet.workbook().active_sheet().name(), "Sheet2");
    }

    #[test]
    fn sheet_commands_manage_named_tabs_and_remain_undoable() {
        let mut workspace = workspace();
        workspace.handle_command(&CommandInvocation {
            function: "SHEET".to_owned(),
            args: vec!["ADD".to_owned(), "DCF".to_owned(), "Model".to_owned()],
        });
        assert_eq!(workspace.spreadsheet.workbook().active_sheet().name(), "DCF Model");
        workspace.handle_command(&CommandInvocation {
            function: "SHEET".to_owned(),
            args: vec!["RENAME".to_owned(), "Base".to_owned(), "Case".to_owned()],
        });
        assert_eq!(workspace.spreadsheet.workbook().active_sheet().name(), "Base Case");

        workspace.handle_command(&CommandInvocation {
            function: "SHEET".to_owned(),
            args: vec!["DELETE".to_owned()],
        });
        assert_eq!(workspace.spreadsheet.workbook().sheet_count(), 1);
        workspace.handle_key(modified_key(KeyCode::Char('z'), KeyModifiers::CONTROL));
        assert_eq!(workspace.spreadsheet.workbook().sheet_count(), 2);
        assert_eq!(workspace.spreadsheet.workbook().active_sheet().name(), "Base Case");
    }
}
