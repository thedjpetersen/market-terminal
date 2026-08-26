use std::{
    cell::Cell as StateCell,
    collections::HashMap,
    sync::{
        mpsc::{sync_channel, Receiver, SyncSender, TrySendError},
        Arc,
    },
};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent};
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
        is_primary_click, scroll_key,
        theme::{AMBER, BG, CYAN, FOOTER_BG, INK, MUTED, NAV_BG, RED, YELLOW},
    },
};

use super::super::{
    domain::{
        parse_formula, AggregateFunction, CellAddress, CellValue, Expr, MAX_COLUMNS, MAX_ROWS,
    },
    MarketDataPoint, MarketDataRequest, MarketDataState, Spreadsheet, SpreadsheetMarketData, ID,
};

const CELL_WIDTH: u16 = 12;
const ROW_HEADER_WIDTH: u16 = 5;

struct MarketDataRefresh {
    generation: u64,
    requests: Vec<MarketDataRequest>,
}

struct MarketDataRefreshResult {
    generation: u64,
    requests: Vec<MarketDataRequest>,
    points: Vec<MarketDataPoint>,
}

#[derive(Debug, Clone, PartialEq)]
enum ExternalCellState {
    Loading,
    Resolved(MarketDataState),
}

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

    fn text(&self) -> String {
        self.characters.iter().collect()
    }

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
    refresh_sender: SyncSender<MarketDataRefresh>,
    refresh_receiver: Receiver<MarketDataRefreshResult>,
    next_refresh_generation: u64,
    applied_refresh_generation: u64,
    external_cells: HashMap<CellAddress, MarketDataRequest>,
    external_states: HashMap<MarketDataRequest, ExternalCellState>,
    cursor: CellAddress,
    first_column: u8,
    first_row: u16,
    visible_columns: StateCell<u8>,
    visible_rows: StateCell<u16>,
    edit: Option<EditSession>,
    clipboard: Option<(String, CellAddress)>,
    status: String,
}

impl SpreadsheetWorkspace {
    pub fn new(market_data: Arc<dyn SpreadsheetMarketData>) -> Self {
        let (refresh_sender, worker_receiver) = sync_channel::<MarketDataRefresh>(1);
        let (worker_sender, refresh_receiver) = sync_channel::<MarketDataRefreshResult>(1);
        std::thread::Builder::new()
            .name("spreadsheet-market-data".to_owned())
            .spawn(move || {
                while let Ok(refresh) = worker_receiver.recv() {
                    let points = market_data.load_batch(&refresh.requests);
                    let result = MarketDataRefreshResult {
                        generation: refresh.generation,
                        requests: refresh.requests,
                        points,
                    };
                    if worker_sender.send(result).is_err() {
                        break;
                    }
                }
            })
            .expect("spreadsheet market-data worker should start");
        let mut workspace = Self {
            spreadsheet: Spreadsheet::new(),
            refresh_sender,
            refresh_receiver,
            next_refresh_generation: 0,
            applied_refresh_generation: 0,
            external_cells: HashMap::new(),
            external_states: HashMap::new(),
            cursor: CellAddress::new(1, 1).expect("A1 is in bounds"),
            first_column: 1,
            first_row: 1,
            visible_columns: StateCell::new(8),
            visible_rows: StateCell::new(18),
            edit: None,
            clipboard: None,
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
            ("A2", "SPY US Equity"),
            ("B2", "=PX_LAST(A2)"),
            ("C2", "=PX_CHANGE(A2, \"1D\")"),
            ("D2", "250"),
            ("E2", "=B2*D2"),
            ("A3", "QQQ US Equity"),
            ("B3", "=PX_LAST(A3)"),
            ("C3", "=PX_CHANGE(A3, \"1D\")"),
            ("D3", "180"),
            ("E3", "=B3*D3"),
            ("A4", "AVGO US Equity"),
            ("B4", "=PX_LAST(A4)"),
            ("C4", "=PX_CHANGE(A4, \"1D\")"),
            ("D4", "120"),
            ("E4", "=B4*D4"),
            ("A5", "NVDA US Equity"),
            ("B5", "=PX_LAST(A5)"),
            ("C5", "=PX_CHANGE(A5, \"1D\")"),
            ("D5", "300"),
            ("E5", "=B5*D5"),
            ("A7", "PORTFOLIO VALUE"),
            ("E7", "=SUM(E2:E5)"),
            ("A9", "MODEL INPUTS"),
            ("A10", "Revenue"),
            ("B10", "1250"),
            ("A11", "Growth"),
            ("B11", "0.12"),
            ("A12", "Forward revenue"),
            ("B12", "=Assumptions!B10*(1+Assumptions!B11)"),
        ] {
            self.spreadsheet
                .set_cell(address, raw)
                .expect("demo seed addresses are valid");
        }
        self.spreadsheet
            .add_sheet("Assumptions")
            .expect("demo sheet name is unique");
        self.spreadsheet
            .select_sheet("Assumptions")
            .expect("demo sheet exists");
        self.spreadsheet
            .set_cells([
                ("A9", "MODEL ASSUMPTIONS"),
                ("A10", "Revenue"),
                ("B10", "1250"),
                ("A11", "Growth"),
                ("B11", "0.12"),
            ])
            .expect("assumption seed addresses are valid");
        self.spreadsheet
            .select_sheet("Sheet1")
            .expect("default demo sheet exists");
        self.refresh_market_data();
        self.spreadsheet.clear_history();
    }

    fn refresh_market_data(&mut self) {
        let formulas = self.financial_formula_requests();
        let mut requests = formulas.values().cloned().collect::<Vec<_>>();
        requests.sort_by(|left, right| {
            (&left.security, &left.field).cmp(&(&right.security, &right.field))
        });
        requests.dedup();
        if requests.is_empty() {
            self.external_cells.clear();
            self.external_states.clear();
            self.status = "NO FINANCIAL FUNCTIONS TO REFRESH".to_owned();
            return;
        }

        let generation = self.next_refresh_generation.wrapping_add(1);
        let refresh = MarketDataRefresh {
            generation,
            requests: requests.clone(),
        };
        match self.refresh_sender.try_send(refresh) {
            Ok(()) => {
                self.next_refresh_generation = generation;
                self.external_cells = formulas;
                self.external_states = requests
                    .into_iter()
                    .map(|request| (request, ExternalCellState::Loading))
                    .collect();
                self.status = "LOADING FINANCIAL FUNCTIONS…".to_owned();
            }
            Err(TrySendError::Full(_)) => {
                self.status = "REFRESH ALREADY IN PROGRESS".to_owned();
            }
            Err(TrySendError::Disconnected(_)) => {
                self.status = "ERROR · MARKET-DATA WORKER IS UNAVAILABLE".to_owned();
            }
        }
    }

    fn poll_market_data(&mut self) {
        while let Ok(result) = self.refresh_receiver.try_recv() {
            if result.generation < self.applied_refresh_generation {
                continue;
            }
            self.applied_refresh_generation = result.generation;
            let mut states = result
                .points
                .into_iter()
                .map(|point| (point.request, ExternalCellState::Resolved(point.state)))
                .collect::<HashMap<_, _>>();
            for request in result.requests {
                states.entry(request).or_insert_with(|| {
                    ExternalCellState::Resolved(MarketDataState::Unavailable {
                        reason: "provider returned no value".to_owned(),
                    })
                });
            }
            let loaded = states
                .values()
                .filter(|state| {
                    matches!(
                        state,
                        ExternalCellState::Resolved(MarketDataState::Ready { .. })
                    )
                })
                .count();
            let degraded = states.len().saturating_sub(loaded);
            self.external_states = states;
            self.status = if degraded == 0 {
                format!("FINANCIAL FUNCTIONS READY · {loaded} FIELDS")
            } else {
                format!("FINANCIAL FUNCTIONS READY · {loaded} FIELDS · {degraded} DEGRADED")
            };
        }
    }

    fn financial_formula_requests(&self) -> HashMap<CellAddress, MarketDataRequest> {
        self.spreadsheet
            .workbook()
            .active_sheet()
            .populated_cells()
            .filter_map(|(address, cell)| {
                let expression = parse_formula(cell.raw()).ok()?;
                self.financial_request(&expression)
                    .map(|request| (address, request))
            })
            .collect()
    }

    fn financial_request(&self, expression: &Expr) -> Option<MarketDataRequest> {
        let Expr::Function {
            function,
            arguments,
        } = expression
        else {
            return None;
        };
        let security = self.expression_text(arguments.first()?)?;
        match function {
            AggregateFunction::PriceLast if arguments.len() == 1 => {
                Some(MarketDataRequest::new(security, "PX_LAST"))
            }
            AggregateFunction::PriceChange if arguments.len() == 2 => {
                let period = self.expression_text(&arguments[1])?.to_ascii_uppercase();
                Some(MarketDataRequest::new(
                    security,
                    format!("CHG_PCT_{period}"),
                ))
            }
            _ => None,
        }
    }

    fn expression_text(&self, expression: &Expr) -> Option<String> {
        match expression {
            Expr::Text(text) => Some(text.clone()),
            Expr::Reference(reference) => {
                let sheet = match reference.sheet() {
                    Some(name) => self.spreadsheet.workbook().sheet(name)?,
                    None => self.spreadsheet.workbook().active_sheet(),
                };
                let raw = sheet.cell(reference.cell().address())?.raw().trim();
                (!raw.is_empty() && !raw.starts_with('=')).then(|| raw.to_owned())
            }
            _ => None,
        }
    }

    fn evaluated_spreadsheet(&self) -> Spreadsheet {
        let mut evaluated = self.spreadsheet.clone();
        for (address, request) in &self.external_cells {
            let Some(ExternalCellState::Resolved(state)) = self.external_states.get(request) else {
                continue;
            };
            let value = match state {
                MarketDataState::Ready { value, .. } | MarketDataState::Stale { value, .. } => {
                    *value
                }
                MarketDataState::PermissionDenied { .. } | MarketDataState::Unavailable { .. } => {
                    continue
                }
            };
            let _ = evaluated.set_cell(&address.to_string(), value.to_string());
        }
        evaluated.clear_history();
        evaluated
    }

    fn selected_address(&self) -> String {
        self.cursor.to_string()
    }

    fn selected_raw(&self) -> String {
        self.spreadsheet
            .cell(&self.selected_address())
            .map(|cell| cell.raw)
            .unwrap_or_default()
    }

    fn begin_edit(&mut self, initial: Option<&str>) {
        let value = initial
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| self.selected_raw());
        self.edit = Some(EditSession::new(&value));
        self.status = "EDIT · ENTER TO COMMIT · ESC TO CANCEL".to_owned();
    }

    fn commit_edit(&mut self) {
        let Some(edit) = self.edit.take() else { return };
        let address = self.selected_address();
        match self.spreadsheet.set_cell(&address, edit.text()) {
            Ok(value) => {
                self.status = format!("{address} = {value}");
                self.refresh_market_data();
            }
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
                if let Some(edit) = &mut self.edit {
                    edit.insert(character);
                }
            }
            KeyCode::Backspace => {
                if let Some(edit) = &mut self.edit {
                    edit.backspace();
                }
            }
            KeyCode::Delete => {
                if let Some(edit) = &mut self.edit {
                    edit.delete();
                }
            }
            KeyCode::Left => {
                if let Some(edit) = &mut self.edit {
                    edit.cursor = edit.cursor.saturating_sub(1);
                }
            }
            KeyCode::Right => {
                if let Some(edit) = &mut self.edit {
                    edit.cursor = (edit.cursor + 1).min(edit.characters.len());
                }
            }
            KeyCode::Home => {
                if let Some(edit) = &mut self.edit {
                    edit.cursor = 0;
                }
            }
            KeyCode::End => {
                if let Some(edit) = &mut self.edit {
                    edit.cursor = edit.characters.len();
                }
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
            self.refresh_market_data();
        }
    }

    fn copy_selected(&mut self) {
        let sheet = self.spreadsheet.workbook().active_sheet().name().to_owned();
        self.clipboard = Some((sheet, self.cursor));
        self.status = format!("COPIED {}", self.cursor);
    }

    fn paste_selected(&mut self) {
        let Some((sheet, source)) = self.clipboard.clone() else {
            self.status = "CLIPBOARD EMPTY".to_owned();
            return;
        };
        if !sheet.eq_ignore_ascii_case(self.spreadsheet.workbook().active_sheet().name()) {
            self.status = "PASTE REQUIRES THE SOURCE SHEET".to_owned();
            return;
        }
        let source = source.to_string();
        let target = self.selected_address();
        match self.spreadsheet.copy_cell(&source, &target) {
            Ok(value) => {
                self.status = format!("PASTED {source} → {target} · {value}");
                self.refresh_market_data();
            }
            Err(error) => self.status = format!("ERROR · {error}"),
        }
    }

    fn fill_from_adjacent(&mut self, vertical: bool) {
        let source = if vertical {
            self.cursor
                .row()
                .checked_sub(1)
                .and_then(|row| CellAddress::new(self.cursor.column(), row).ok())
        } else {
            self.cursor
                .column()
                .checked_sub(1)
                .and_then(|column| CellAddress::new(column, self.cursor.row()).ok())
        };
        let Some(source) = source else {
            self.status = if vertical {
                "NO CELL ABOVE TO FILL"
            } else {
                "NO CELL LEFT TO FILL"
            }
            .to_owned();
            return;
        };
        let target = self.selected_address();
        match self.spreadsheet.copy_cell(&source.to_string(), &target) {
            Ok(value) => {
                self.status = format!("FILLED {source} → {target} · {value}");
                self.refresh_market_data();
            }
            Err(error) => self.status = format!("ERROR · {error}"),
        }
    }

    fn undo(&mut self) {
        self.status = if self.spreadsheet.undo() {
            self.refresh_market_data();
            format!(
                "UNDID CHANGE · {}",
                self.spreadsheet.workbook().active_sheet().name()
            )
        } else {
            "NOTHING TO UNDO".to_owned()
        };
    }

    fn redo(&mut self) {
        self.status = if self.spreadsheet.redo() {
            self.refresh_market_data();
            format!(
                "REDID CHANGE · {}",
                self.spreadsheet.workbook().active_sheet().name()
            )
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
                self.status = format!(
                    "SELECTED SHEET {}",
                    self.spreadsheet.workbook().active_sheet().name()
                );
                self.refresh_market_data();
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
        self.status = format!(
            "SELECTED SHEET {}",
            self.spreadsheet.workbook().active_sheet().name()
        );
        self.refresh_market_data();
    }

    fn render_formula_bar(&self, frame: &mut Frame, area: Rect) {
        let raw = self
            .edit
            .as_ref()
            .map(EditSession::text)
            .unwrap_or_else(|| self.selected_raw());
        let editing = self.edit.is_some();
        let cursor = if let Some(edit) = &self.edit {
            let before = edit.characters.iter().take(edit.cursor).collect::<String>();
            format!(
                "{before}▌{}",
                edit.characters.iter().skip(edit.cursor).collect::<String>()
            )
        } else {
            raw
        };
        let border = if editing { CYAN } else { AMBER };
        let line = Line::from(vec![
            Span::styled(
                format!(" {:<5} ", self.cursor),
                Style::new().bg(AMBER).fg(BG).bold(),
            ),
            Span::styled(" ƒx  ", CYAN),
            Span::styled(
                cursor,
                if editing {
                    Style::new().fg(CYAN)
                } else {
                    Style::new().fg(INK)
                },
            ),
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
        let rows = area
            .height
            .saturating_sub(3)
            .max(1)
            .min(MAX_ROWS - self.first_row + 1);
        self.visible_columns.set(columns);
        self.visible_rows.set(rows);
        let evaluated = self.evaluated_spreadsheet();
        let visible_values = evaluated
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
                    let address =
                        CellAddress::new(column, row).expect("render region is in bounds");
                    let value = self.external_display(address).unwrap_or_else(|| {
                        visible_values
                            .get(&address)
                            .map(format_value)
                            .unwrap_or_default()
                    });
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
            tabs.push(Span::styled(
                format!(" {}:{} ", index + 1, sheet.name()),
                style,
            ));
            tabs.push(Span::raw(" "));
        }
        frame.render_widget(
            Paragraph::new(Line::from(tabs)).style(Style::new().bg(NAV_BG)),
            area,
        );
    }

    fn render_status(&self, frame: &mut Frame, area: Rect) {
        let mode = if self.edit.is_some() { "EDIT" } else { "NAV" };
        let status = self
            .selected_external_status()
            .unwrap_or_else(|| self.status.clone());
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(format!(" {mode} "), Style::new().bg(AMBER).fg(BG).bold()),
                Span::styled(format!(" {status}   "), INK),
                Span::styled(
                    "F9 REFRESH  Y COPY  P PASTE  CTRL-D/R FILL  CTRL-Z/Y UNDO/REDO  F2 EDIT",
                    MUTED,
                ),
            ]))
            .style(Style::new().bg(FOOTER_BG)),
            area,
        );
    }

    fn external_display(&self, address: CellAddress) -> Option<String> {
        let request = self.external_cells.get(&address)?;
        Some(match self.external_states.get(request)? {
            ExternalCellState::Loading => "…LOADING".to_owned(),
            ExternalCellState::Resolved(MarketDataState::Ready { value, .. }) => {
                format_value(&CellValue::Number(*value))
            }
            ExternalCellState::Resolved(MarketDataState::Stale { value, .. }) => {
                format!("~{}", format_value(&CellValue::Number(*value)))
            }
            ExternalCellState::Resolved(MarketDataState::PermissionDenied { .. }) => {
                "#DENIED".to_owned()
            }
            ExternalCellState::Resolved(MarketDataState::Unavailable { .. }) => "#N/A".to_owned(),
        })
    }

    fn selected_external_status(&self) -> Option<String> {
        let request = self.external_cells.get(&self.cursor)?;
        Some(match self.external_states.get(request)? {
            ExternalCellState::Loading => {
                format!("LOADING · {} · {}", request.security, request.field)
            }
            ExternalCellState::Resolved(MarketDataState::Ready { provenance, .. }) => format!(
                "{} · {} · {} · OBS {} · RX {} · {}",
                request.security,
                request.field,
                provenance.provider,
                provenance.observed_at,
                provenance.received_at,
                provenance.quality.label(),
            ),
            ExternalCellState::Resolved(MarketDataState::Stale { provenance, .. }) => format!(
                "STALE · {} · {} · {} · OBS {} · RX {} · {}",
                request.security,
                request.field,
                provenance.provider,
                provenance.observed_at,
                provenance.received_at,
                provenance.quality.label(),
            ),
            ExternalCellState::Resolved(MarketDataState::PermissionDenied { provider }) => {
                format!(
                    "PERMISSION DENIED · {} · {} · {provider}",
                    request.security, request.field
                )
            }
            ExternalCellState::Resolved(MarketDataState::Unavailable { reason }) => {
                format!(
                    "UNAVAILABLE · {} · {} · {reason}",
                    request.security, request.field
                )
            }
        })
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

    fn hotkey(&self) -> Option<char> {
        None
    }

    fn is_favorite(&self) -> bool {
        true
    }

    fn handle_command(&mut self, invocation: &CommandInvocation) -> bool {
        let Some(first) = invocation.args.first() else {
            return true;
        };
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
            "SELECT" if name.is_empty() => {
                self.status = "ERROR · SHEET SELECT REQUIRES A NAME".to_owned()
            }
            "SELECT" => self.select_sheet(&name),
            "RENAME" if name.is_empty() => {
                self.status = "ERROR · SHEET RENAME REQUIRES A NAME".to_owned()
            }
            "RENAME" => match self.spreadsheet.rename_active_sheet(name) {
                Ok(()) => {
                    self.status = format!(
                        "RENAMED SHEET {}",
                        self.spreadsheet.workbook().active_sheet().name()
                    );
                }
                Err(error) => self.status = format!("ERROR · {error}"),
            },
            "DELETE" | "REMOVE" => match self.spreadsheet.remove_active_sheet() {
                Ok(()) => {
                    self.status = format!(
                        "REMOVED SHEET · NOW ON {}",
                        self.spreadsheet.workbook().active_sheet().name()
                    );
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
            KeyCode::Char('d') if control => self.fill_from_adjacent(true),
            KeyCode::Char('r') if control => self.fill_from_adjacent(false),
            KeyCode::PageDown if control => self.select_adjacent_sheet(true),
            KeyCode::PageUp if control => self.select_adjacent_sheet(false),
            KeyCode::F(11) if shift => self.add_sheet(None),
            KeyCode::Char('y') => self.copy_selected(),
            KeyCode::Char('p') => self.paste_selected(),
            KeyCode::Up | KeyCode::Char('k') => self.move_cursor(0, -1),
            KeyCode::Down | KeyCode::Char('j') => self.move_cursor(0, 1),
            KeyCode::Left | KeyCode::Char('h') => self.move_cursor(-1, 0),
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Tab => self.move_cursor(1, 0),
            KeyCode::BackTab => self.move_cursor(-1, 0),
            KeyCode::Enter | KeyCode::F(2) => self.begin_edit(None),
            KeyCode::Char('=') => self.begin_edit(Some("=")),
            KeyCode::Delete => self.clear_selected(),
            KeyCode::F(9) => self.refresh_market_data(),
            _ => return false,
        }
        true
    }

    fn handle_mouse(&mut self, event: MouseEvent, area: Rect) -> bool {
        let regions = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(area);
        if let Some(key) = scroll_key(event, regions[1]) {
            return self.handle_key(key);
        }
        if is_primary_click(event, regions[0]) {
            if self.edit.is_none() {
                self.begin_edit(None);
            }
            return true;
        }
        if is_primary_click(event, regions[2]) {
            if self.edit.is_some() {
                self.commit_edit();
            }
            let mut x = regions[2].x;
            if event.column < x.saturating_add(3) {
                self.add_sheet(None);
                return true;
            }
            x = x.saturating_add(3);
            let target = self
                .spreadsheet
                .workbook()
                .sheets()
                .iter()
                .enumerate()
                .find_map(|(index, sheet)| {
                    let width = format!(" {}:{} ", index + 1, sheet.name()).chars().count() as u16;
                    let hit = event.column >= x && event.column < x.saturating_add(width);
                    x = x.saturating_add(width).saturating_add(1);
                    hit.then(|| sheet.name().to_owned())
                });
            if let Some(name) = target {
                self.select_sheet(&name);
            }
            return true;
        }
        if !is_primary_click(event, regions[1]) {
            return false;
        }
        if self.edit.is_some() {
            self.commit_edit();
        }

        let grid = regions[1];
        let available_width = grid.width.saturating_sub(ROW_HEADER_WIDTH + 3);
        let columns = (available_width / (CELL_WIDTH + 1))
            .max(1)
            .min(u16::from(MAX_COLUMNS - self.first_column + 1)) as u8;
        let rows = grid
            .height
            .saturating_sub(3)
            .max(1)
            .min(MAX_ROWS - self.first_row + 1);
        let data_y = grid.y.saturating_add(2);
        if event.row < data_y || event.row >= data_y.saturating_add(rows) {
            return true;
        }
        let row = self.first_row.saturating_add(event.row - data_y);
        let row_header_x = grid.x.saturating_add(1);
        let data_x = row_header_x.saturating_add(ROW_HEADER_WIDTH + 1);
        let column = if event.column >= row_header_x
            && event.column < row_header_x.saturating_add(ROW_HEADER_WIDTH)
        {
            self.cursor.column()
        } else if event.column >= data_x {
            let relative = event.column - data_x;
            let offset = relative / (CELL_WIDTH + 1);
            if offset >= u16::from(columns) || relative % (CELL_WIDTH + 1) >= CELL_WIDTH {
                return true;
            }
            self.first_column.saturating_add(offset as u8)
        } else {
            return true;
        };
        self.cursor = CellAddress::new(column, row).expect("visible grid click is in bounds");
        self.status = format!("SELECTED {}", self.cursor);
        true
    }

    fn poll_intents(&mut self) -> Vec<crate::app::AppIntent> {
        self.poll_market_data();
        Vec::new()
    }

    fn on_blur(&mut self) {
        if self.edit.is_some() {
            self.cancel_edit();
        }
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
    } else if value.starts_with('…') {
        Style::new().fg(YELLOW)
    } else if value.starts_with('~') {
        Style::new().fg(AMBER)
    } else {
        Style::new().fg(INK)
    }
}

fn truncate(value: &str, width: usize) -> String {
    let max = width.saturating_sub(1);
    let mut characters = value.chars();
    let shortened = characters.by_ref().take(max).collect::<String>();
    if characters.next().is_some() && max > 0 {
        format!(
            "{}…",
            shortened
                .chars()
                .take(max.saturating_sub(1))
                .collect::<String>()
        )
    } else {
        shortened
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::spreadsheet::{MarketDataPoint, MarketDataProvenance, MarketDataQuality};
    use crossterm::event::{MouseButton, MouseEventKind};

    fn provenance() -> MarketDataProvenance {
        MarketDataProvenance {
            provider: "TEST FEED".to_owned(),
            observed_at: "2026-08-26T13:00:00-07:00".to_owned(),
            received_at: "2026-08-26T13:00:01-07:00".to_owned(),
            quality: MarketDataQuality::Realtime,
        }
    }

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
                    Some(MarketDataPoint::ready(request.clone(), value, provenance()))
                })
                .collect()
        }
    }

    fn workspace() -> SpreadsheetWorkspace {
        let mut workspace = SpreadsheetWorkspace::new(Arc::new(StubMarketData));
        wait_for_data(&mut workspace);
        workspace
    }

    fn wait_for_data(workspace: &mut SpreadsheetWorkspace) {
        for _ in 0..100 {
            workspace.poll_market_data();
            if !workspace
                .external_states
                .values()
                .any(|state| matches!(state, ExternalCellState::Loading))
            {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        panic!("spreadsheet market-data worker did not respond");
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn modified_key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
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
    fn seeds_market_data_and_formulas() {
        let workspace = workspace();
        assert_eq!(
            workspace.spreadsheet.cell("A2").unwrap().raw,
            "SPY US Equity"
        );
        assert_eq!(
            workspace.spreadsheet.cell("B2").unwrap().raw,
            "=PX_LAST(A2)"
        );
        assert_eq!(
            workspace.spreadsheet.cell("C2").unwrap().raw,
            "=PX_CHANGE(A2, \"1D\")"
        );
        let evaluated = workspace.evaluated_spreadsheet();
        assert_eq!(
            evaluated.cell("E2").unwrap().value,
            CellValue::Number(132_617.5)
        );
        assert!(matches!(
            evaluated.cell("E7").unwrap().value,
            CellValue::Number(_)
        ));
        let forward_revenue = workspace
            .spreadsheet
            .cell("B12")
            .unwrap()
            .value
            .as_number()
            .unwrap();
        assert!((forward_revenue - 1400.0).abs() < 1e-9);
        assert_eq!(workspace.spreadsheet.workbook().sheet_count(), 2);
    }

    #[test]
    fn financial_cells_begin_loading_and_expose_provenance_when_ready() {
        let loading = SpreadsheetWorkspace::new(Arc::new(StubMarketData));
        assert_eq!(
            loading.external_display("B2".parse().unwrap()),
            Some("…LOADING".to_owned())
        );

        let mut ready = workspace();
        ready.cursor = "B2".parse().unwrap();
        let status = ready.selected_external_status().unwrap();
        assert!(status.contains("SPY US Equity · PX_LAST · TEST FEED"));
        assert!(status.contains("OBS 2026-08-26T13:00:00-07:00"));
        assert!(status.contains("REALTIME"));
    }

    #[test]
    fn financial_cells_render_stale_permission_and_unavailable_states() {
        let mut workspace = workspace();
        let spy = MarketDataRequest::new("SPY US Equity", "PX_LAST");
        let qqq = MarketDataRequest::new("QQQ US Equity", "PX_LAST");
        let avgo = MarketDataRequest::new("AVGO US Equity", "PX_LAST");
        workspace.external_states.insert(
            spy,
            ExternalCellState::Resolved(MarketDataState::Stale {
                value: 529.0,
                provenance: provenance(),
            }),
        );
        workspace.external_states.insert(
            qqq,
            ExternalCellState::Resolved(MarketDataState::PermissionDenied {
                provider: "TEST FEED".to_owned(),
            }),
        );
        workspace.external_states.insert(
            avgo,
            ExternalCellState::Resolved(MarketDataState::Unavailable {
                reason: "no observation".to_owned(),
            }),
        );

        assert_eq!(
            workspace.external_display("B2".parse().unwrap()),
            Some("~529".to_owned())
        );
        assert_eq!(
            workspace.external_display("B3".parse().unwrap()),
            Some("#DENIED".to_owned())
        );
        assert_eq!(
            workspace.external_display("B4".parse().unwrap()),
            Some("#N/A".to_owned())
        );
    }

    #[test]
    fn navigation_is_bounded_and_scrolls_the_viewport() {
        let mut workspace = workspace();
        workspace.visible_rows.set(3);
        for _ in 0..5 {
            assert!(workspace.handle_key(key(KeyCode::Down)));
        }
        assert_eq!(workspace.cursor.to_string(), "A6");
        assert_eq!(workspace.first_row, 4);
        workspace.handle_key(key(KeyCode::Left));
        assert_eq!(workspace.cursor.to_string(), "A6");
    }

    #[test]
    fn clicking_grid_cells_and_formula_bar_updates_focus() {
        let mut workspace = workspace();
        let area = Rect::new(0, 0, 120, 30);

        assert!(workspace.handle_mouse(click(34, 7), area));
        assert_eq!(workspace.cursor.to_string(), "C3");

        assert!(workspace.handle_mouse(click(20, 1), area));
        assert!(workspace.edit.is_some());
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
        wait_for_data(&mut workspace);
        assert!(matches!(
            workspace.evaluated_spreadsheet().cell("A1").unwrap().value,
            CellValue::Number(_)
        ));
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

        assert!(workspace.handle_key(modified_key(KeyCode::Char('z'), KeyModifiers::CONTROL,)));
        assert_eq!(workspace.spreadsheet.cell("A1").unwrap().raw, original);
        assert!(workspace.handle_key(modified_key(KeyCode::Char('y'), KeyModifiers::CONTROL,)));
        assert_eq!(workspace.spreadsheet.cell("A1").unwrap().raw, "x");
    }

    #[test]
    fn keyboard_creates_and_cycles_workbook_tabs() {
        let mut workspace = workspace();
        assert!(workspace.handle_key(modified_key(KeyCode::F(11), KeyModifiers::SHIFT,)));
        assert_eq!(workspace.spreadsheet.workbook().sheet_count(), 3);
        assert_eq!(
            workspace.spreadsheet.workbook().active_sheet().name(),
            "Sheet3"
        );

        assert!(workspace.handle_key(modified_key(KeyCode::PageUp, KeyModifiers::CONTROL,)));
        assert_eq!(
            workspace.spreadsheet.workbook().active_sheet().name(),
            "Assumptions"
        );
        assert!(workspace.handle_key(modified_key(KeyCode::PageDown, KeyModifiers::CONTROL,)));
        assert_eq!(
            workspace.spreadsheet.workbook().active_sheet().name(),
            "Sheet3"
        );
    }

    #[test]
    fn sheet_commands_manage_named_tabs_and_remain_undoable() {
        let mut workspace = workspace();
        workspace.handle_command(&CommandInvocation {
            function: "SHEET".to_owned(),
            args: vec!["ADD".to_owned(), "DCF".to_owned(), "Model".to_owned()],
        });
        assert_eq!(
            workspace.spreadsheet.workbook().active_sheet().name(),
            "DCF Model"
        );
        workspace.handle_command(&CommandInvocation {
            function: "SHEET".to_owned(),
            args: vec!["RENAME".to_owned(), "Base".to_owned(), "Case".to_owned()],
        });
        assert_eq!(
            workspace.spreadsheet.workbook().active_sheet().name(),
            "Base Case"
        );

        workspace.handle_command(&CommandInvocation {
            function: "SHEET".to_owned(),
            args: vec!["DELETE".to_owned()],
        });
        assert_eq!(workspace.spreadsheet.workbook().sheet_count(), 2);
        workspace.handle_key(modified_key(KeyCode::Char('z'), KeyModifiers::CONTROL));
        assert_eq!(workspace.spreadsheet.workbook().sheet_count(), 3);
        assert_eq!(
            workspace.spreadsheet.workbook().active_sheet().name(),
            "Base Case"
        );
    }

    #[test]
    fn keyboard_copy_paste_and_fill_translate_relative_formulas() {
        let mut workspace = workspace();
        workspace.spreadsheet.set_cell("A20", "5").unwrap();
        workspace.spreadsheet.set_cell("B20", "=A20").unwrap();
        workspace.cursor = "B20".parse().unwrap();
        assert!(workspace.handle_key(key(KeyCode::Char('y'))));
        workspace.cursor = "C21".parse().unwrap();
        assert!(workspace.handle_key(key(KeyCode::Char('p'))));
        assert_eq!(workspace.spreadsheet.cell("C21").unwrap().raw, "=B21");

        workspace.spreadsheet.set_cell("C20", "=B20 * 2").unwrap();
        workspace.cursor = "C21".parse().unwrap();
        assert!(workspace.handle_key(modified_key(KeyCode::Char('d'), KeyModifiers::CONTROL)));
        assert_eq!(workspace.spreadsheet.cell("C21").unwrap().raw, "=B21 * 2");
    }
}
