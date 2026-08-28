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
    layout::{Constraint, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell as TableCell, Paragraph, Row, Table},
    Frame,
};

use crate::{
    app::{AppIntent, CommandInvocation, Workspace, WorkspaceAction, WorkspaceDescriptor},
    ui::{
        components::terminal_block,
        contains, is_primary_click, scroll_key,
        theme::{AMBER, BG, CYAN, FOOTER_BG, INK, MUTED, NAV_BG, RED, YELLOW},
    },
};

use super::{
    super::{
        domain::{
            parse_formula, AggregateFunction, CellAddress, CellValue, Expr, MAX_COLUMNS, MAX_ROWS,
        },
        MarketDataPoint, MarketDataRequest, MarketDataState, Spreadsheet, SpreadsheetFileStore,
        SpreadsheetMarketData, SpreadsheetWorkbookStore, StoredWorkbook, ID,
    },
    controls::{
        formula_action_area, pack_control_areas, pack_tab_areas, spreadsheet_areas, GridGeometry,
        SpreadsheetControl, CELL_WIDTH, ROW_HEADER_WIDTH,
    },
};

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
    file_store: Option<Arc<dyn SpreadsheetFileStore>>,
    workbook_store: Option<Arc<dyn SpreadsheetWorkbookStore>>,
    workbook_id: String,
    workbook_revision: u64,
    refresh_sender: SyncSender<MarketDataRefresh>,
    refresh_receiver: Receiver<MarketDataRefreshResult>,
    next_refresh_generation: u64,
    applied_refresh_generation: u64,
    external_cells: HashMap<CellAddress, Vec<MarketDataRequest>>,
    external_states: HashMap<MarketDataRequest, ExternalCellState>,
    cursor: CellAddress,
    first_column: u8,
    first_row: u16,
    visible_columns: StateCell<u8>,
    visible_rows: StateCell<u16>,
    edit: Option<EditSession>,
    clipboard: Option<(String, CellAddress)>,
    pending_intents: Vec<AppIntent>,
    status: String,
}

impl SpreadsheetWorkspace {
    pub fn new(market_data: Arc<dyn SpreadsheetMarketData>) -> Self {
        Self::build(market_data, None, None, true)
    }

    pub fn empty(
        market_data: Arc<dyn SpreadsheetMarketData>,
        file_store: Arc<dyn SpreadsheetFileStore>,
    ) -> Self {
        Self::build(market_data, Some(file_store), None, false)
    }

    pub fn persistent(
        market_data: Arc<dyn SpreadsheetMarketData>,
        file_store: Arc<dyn SpreadsheetFileStore>,
        workbook_store: Arc<dyn SpreadsheetWorkbookStore>,
    ) -> Self {
        let mut workspace = Self::build(market_data, Some(file_store), Some(workbook_store), false);
        workspace.load_workbook("default", true);
        workspace
    }

    fn build(
        market_data: Arc<dyn SpreadsheetMarketData>,
        file_store: Option<Arc<dyn SpreadsheetFileStore>>,
        workbook_store: Option<Arc<dyn SpreadsheetWorkbookStore>>,
        seed_gallery: bool,
    ) -> Self {
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
            file_store,
            workbook_store,
            workbook_id: "default".to_owned(),
            workbook_revision: 0,
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
            pending_intents: Vec::new(),
            status: if seed_gallery {
                String::new()
            } else {
                "EMPTY WORKBOOK · TYPE A VALUE OR USE SHEET IMPORT <FILE.CSV>".to_owned()
            },
        };
        if seed_gallery {
            workspace.seed_demo_workbook();
        }
        workspace
    }

    fn save_workbook(&mut self, requested_id: &str, automatic: bool) {
        let Some(store) = self.workbook_store.clone() else {
            if !automatic {
                self.status = "WORKBOOK PERSISTENCE IS DISABLED IN GALLERY MODE".to_owned();
            }
            return;
        };
        let id = if requested_id.trim().is_empty() {
            self.workbook_id.clone()
        } else {
            requested_id.trim().to_owned()
        };
        let payload = match self.spreadsheet.to_document_payload() {
            Ok(payload) => payload,
            Err(error) => {
                self.status = format!("SAVE ERROR · {error}");
                return;
            }
        };
        let revision = self.workbook_revision.saturating_add(1);
        match store.save_workbook(&StoredWorkbook {
            id: id.clone(),
            revision,
            payload,
        }) {
            Ok(()) => {
                self.workbook_id = id.clone();
                self.workbook_revision = revision;
                if !automatic {
                    self.status = format!("SAVED WORKBOOK {id} · REVISION {revision}");
                }
            }
            Err(error) => self.status = format!("SAVE ERROR · {error}"),
        }
    }

    fn load_workbook(&mut self, requested_id: &str, startup: bool) {
        let Some(store) = self.workbook_store.clone() else {
            if !startup {
                self.status = "WORKBOOK PERSISTENCE IS DISABLED IN GALLERY MODE".to_owned();
            }
            return;
        };
        let id = if requested_id.trim().is_empty() {
            "default"
        } else {
            requested_id.trim()
        };
        match store.load_workbook(id) {
            Ok(Some(document)) => match Spreadsheet::from_document_payload(&document.payload) {
                Ok(spreadsheet) => {
                    self.spreadsheet = spreadsheet;
                    self.workbook_id = document.id;
                    self.workbook_revision = document.revision;
                    self.cursor = CellAddress::new(1, 1).expect("A1 is in bounds");
                    self.first_column = 1;
                    self.first_row = 1;
                    self.refresh_market_data();
                    self.status = format!(
                        "LOADED WORKBOOK {} · REVISION {}",
                        self.workbook_id, self.workbook_revision
                    );
                }
                Err(error) => self.status = format!("LOAD ERROR · {error}"),
            },
            Ok(None) if startup => {
                self.status = "EMPTY WORKBOOK · AUTOSAVE TARGET default".to_owned();
            }
            Ok(None) => self.status = format!("LOAD ERROR · WORKBOOK {id} WAS NOT FOUND"),
            Err(error) => self.status = format!("LOAD ERROR · {error}"),
        }
    }

    fn list_workbooks(&mut self) {
        let Some(store) = self.workbook_store.clone() else {
            self.status = "WORKBOOK PERSISTENCE IS DISABLED IN GALLERY MODE".to_owned();
            return;
        };
        self.status = match store.list_workbooks() {
            Ok(ids) if ids.is_empty() => "NO SAVED WORKBOOKS".to_owned(),
            Ok(ids) => format!("WORKBOOKS · {}", ids.join(" · ")),
            Err(error) => format!("LIST ERROR · {error}"),
        };
    }

    fn delete_workbook(&mut self, id: &str) {
        let Some(store) = self.workbook_store.clone() else {
            self.status = "WORKBOOK PERSISTENCE IS DISABLED IN GALLERY MODE".to_owned();
            return;
        };
        if id.trim().is_empty() {
            self.status = "DELETE ERROR · SHEET DROP REQUIRES A WORKBOOK ID".to_owned();
            return;
        }
        self.status = match store.delete_workbook(id.trim()) {
            Ok(true) => format!("DELETED WORKBOOK {}", id.trim()),
            Ok(false) => format!("WORKBOOK {} WAS NOT FOUND", id.trim()),
            Err(error) => format!("DELETE ERROR · {error}"),
        };
    }

    fn autosave(&mut self) {
        let id = self.workbook_id.clone();
        self.save_workbook(&id, true);
    }

    fn selected_instrument(&self) -> Option<String> {
        self.spreadsheet
            .cell(&self.cursor.to_string())
            .ok()
            .map(|cell| cell.raw.trim().to_owned())
            .filter(|raw| !raw.is_empty() && !raw.starts_with('='))
    }

    fn send_selected_instrument(&mut self, target: &str) {
        let subject = self.selected_instrument();
        let Some(subject) = subject else {
            self.status = format!("{target} REQUIRES A TEXT INSTRUMENT IN THE SELECTED CELL");
            return;
        };
        self.pending_intents.push(AppIntent::DispatchCommand {
            command: format!("{target} {subject}"),
            origin: ID,
        });
        self.status = format!("SENT {subject} TO {target}");
    }

    fn active_sheet_identity(&self) -> u64 {
        stable_identity(self.spreadsheet.workbook().active_sheet().name())
    }

    fn grid_geometry(&self, area: Rect) -> GridGeometry {
        let grid = GridGeometry::new(area, self.first_column, self.first_row);
        self.visible_columns.set(grid.columns);
        self.visible_rows.set(grid.rows);
        grid
    }

    fn is_source_cell_populated(&self, vertical: bool) -> bool {
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
        source
            .and_then(|address| self.spreadsheet.workbook().active_sheet().cell(address))
            .is_some_and(|cell| !cell.raw().is_empty())
    }

    fn control_enabled(&self, control: SpreadsheetControl) -> bool {
        match control {
            SpreadsheetControl::Edit => self.edit.is_none(),
            SpreadsheetControl::Clear | SpreadsheetControl::Copy => !self.selected_raw().is_empty(),
            SpreadsheetControl::Paste => self.clipboard.as_ref().is_some_and(|(sheet, _)| {
                sheet.eq_ignore_ascii_case(self.spreadsheet.workbook().active_sheet().name())
            }),
            SpreadsheetControl::FillDown => self.is_source_cell_populated(true),
            SpreadsheetControl::FillRight => self.is_source_cell_populated(false),
            SpreadsheetControl::Undo => self.spreadsheet.can_undo(),
            SpreadsheetControl::Redo => self.spreadsheet.can_redo(),
            SpreadsheetControl::Security | SpreadsheetControl::Chart | SpreadsheetControl::News => {
                self.selected_instrument().is_some()
            }
            SpreadsheetControl::Refresh => !self.external_cells.is_empty(),
        }
    }

    fn control_label(&self, control: SpreadsheetControl) -> String {
        match control {
            SpreadsheetControl::Edit => format!("Edit {}", self.cursor),
            SpreadsheetControl::Clear => format!("Clear {}", self.cursor),
            SpreadsheetControl::Copy => format!("Copy {}", self.cursor),
            SpreadsheetControl::Paste => format!("Paste into {}", self.cursor),
            SpreadsheetControl::FillDown => format!("Fill {} from the cell above", self.cursor),
            SpreadsheetControl::FillRight => {
                format!("Fill {} from the cell to its left", self.cursor)
            }
            SpreadsheetControl::Undo => "Undo the last workbook change".to_owned(),
            SpreadsheetControl::Redo => "Redo the last undone workbook change".to_owned(),
            SpreadsheetControl::Security => format!(
                "Open {} in Security",
                self.selected_instrument()
                    .unwrap_or_else(|| "selected cell".to_owned())
            ),
            SpreadsheetControl::Chart => format!(
                "Chart {}",
                self.selected_instrument()
                    .unwrap_or_else(|| "selected cell".to_owned())
            ),
            SpreadsheetControl::News => format!(
                "Open news for {}",
                self.selected_instrument()
                    .unwrap_or_else(|| "selected cell".to_owned())
            ),
            SpreadsheetControl::Refresh => "Refresh financial functions".to_owned(),
        }
    }

    fn activate_control(&mut self, control: SpreadsheetControl) -> bool {
        if !self.control_enabled(control) {
            return false;
        }
        match control {
            SpreadsheetControl::Edit => self.begin_edit(None),
            SpreadsheetControl::Clear => self.clear_selected(),
            SpreadsheetControl::Copy => self.copy_selected(),
            SpreadsheetControl::Paste => self.paste_selected(),
            SpreadsheetControl::FillDown => self.fill_from_adjacent(true),
            SpreadsheetControl::FillRight => self.fill_from_adjacent(false),
            SpreadsheetControl::Undo => self.undo(),
            SpreadsheetControl::Redo => self.redo(),
            SpreadsheetControl::Security => self.send_selected_instrument("SEC"),
            SpreadsheetControl::Chart => self.send_selected_instrument("CHART"),
            SpreadsheetControl::News => self.send_selected_instrument("NEWS"),
            SpreadsheetControl::Refresh => self.refresh_market_data(),
        }
        true
    }

    fn tab_areas(&self, area: Rect) -> Vec<(usize, Rect)> {
        let workbook = self.spreadsheet.workbook();
        pack_tab_areas(
            area,
            std::iter::once((usize::MAX, 3)).chain(workbook.sheets().iter().enumerate().map(
                |(index, sheet)| {
                    let label = format!(" {}:{}  ", index + 1, sheet.name());
                    (index, label.chars().count() as u16)
                },
            )),
        )
    }

    fn insert_result(&mut self, value: &str) {
        if value.trim().is_empty() {
            self.status = "INSERT REQUIRES A VALUE".to_owned();
            return;
        }
        let address = self.cursor.to_string();
        match self.spreadsheet.set_cell(&address, value.trim()) {
            Ok(_) => {
                self.refresh_market_data();
                self.autosave();
                self.status = format!("INSERTED RESULT INTO {address}");
            }
            Err(error) => self.status = format!("INSERT ERROR · {error}"),
        }
    }

    fn seed_demo_workbook(&mut self) {
        for (address, raw) in [
            ("A1", "SECURITY"),
            ("B1", "LAST PRICE"),
            ("C1", "DAY %"),
            ("D1", "SHARES"),
            ("E1", "MARKET VALUE"),
            ("A2", "IBM US Equity"),
            ("B2", "=PX_LAST(A2)"),
            ("C2", "=PX_CHANGE(A2, \"1D\")"),
            ("D2", "250"),
            ("E2", "=B2*D2"),
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
        let mut requests = formulas
            .values()
            .flat_map(|requests| requests.iter().cloned())
            .collect::<Vec<_>>();
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

    fn financial_formula_requests(&self) -> HashMap<CellAddress, Vec<MarketDataRequest>> {
        self.spreadsheet
            .workbook()
            .active_sheet()
            .populated_cells()
            .filter_map(|(address, cell)| {
                let expression = parse_formula(cell.raw()).ok()?;
                let mut requests = Vec::new();
                self.collect_financial_requests(&expression, &mut requests);
                requests.sort_by(|left, right| {
                    (&left.security, &left.field).cmp(&(&right.security, &right.field))
                });
                requests.dedup();
                (!requests.is_empty()).then_some((address, requests))
            })
            .collect()
    }

    fn collect_financial_requests(&self, expression: &Expr, requests: &mut Vec<MarketDataRequest>) {
        if let Some(request) = self.financial_request(expression) {
            requests.push(request);
            return;
        }
        match expression {
            Expr::Unary { operand, .. } => self.collect_financial_requests(operand, requests),
            Expr::Binary { left, right, .. } => {
                self.collect_financial_requests(left, requests);
                self.collect_financial_requests(right, requests);
            }
            Expr::Function { arguments, .. } => {
                for argument in arguments {
                    self.collect_financial_requests(argument, requests);
                }
            }
            Expr::Number(_) | Expr::Text(_) | Expr::Reference(_) | Expr::Range(_) => {}
        }
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
            AggregateFunction::History if arguments.len() == 4 => {
                let field = self.expression_text(&arguments[1])?.to_ascii_uppercase();
                let start = self.expression_text(&arguments[2])?;
                let end = self.expression_text(&arguments[3])?;
                Some(MarketDataRequest::new(
                    security,
                    format!("HISTORY|{field}|{start}|{end}"),
                ))
            }
            AggregateFunction::Fundamental if arguments.len() == 3 => {
                let field = self.expression_text(&arguments[1])?.to_ascii_uppercase();
                let period = self.expression_text(&arguments[2])?.to_ascii_uppercase();
                Some(MarketDataRequest::new(
                    security,
                    format!("FUNDAMENTAL|{field}|{period}"),
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
        for address in self.external_cells.keys() {
            let Some(cell) = self.spreadsheet.workbook().active_sheet().cell(*address) else {
                continue;
            };
            let Some(expression) = parse_formula(cell.raw()).ok() else {
                continue;
            };
            let Some(expression) = self.substitute_external_values(&expression) else {
                continue;
            };
            let _ = evaluated.set_cell(&address.to_string(), format!("={expression}"));
        }
        evaluated.clear_history();
        evaluated
    }

    fn substitute_external_values(&self, expression: &Expr) -> Option<Expr> {
        if let Some(request) = self.financial_request(expression) {
            let ExternalCellState::Resolved(state) = self.external_states.get(&request)? else {
                return None;
            };
            return match state {
                MarketDataState::Ready { value, .. } | MarketDataState::Stale { value, .. } => {
                    Some(Expr::Number(*value))
                }
                MarketDataState::PermissionDenied { .. } | MarketDataState::Unavailable { .. } => {
                    None
                }
            };
        }
        match expression {
            Expr::Unary { operator, operand } => Some(Expr::Unary {
                operator: *operator,
                operand: Box::new(self.substitute_external_values(operand)?),
            }),
            Expr::Binary {
                left,
                operator,
                right,
            } => Some(Expr::Binary {
                left: Box::new(self.substitute_external_values(left)?),
                operator: *operator,
                right: Box::new(self.substitute_external_values(right)?),
            }),
            Expr::Function {
                function,
                arguments,
            } => Some(Expr::Function {
                function: *function,
                arguments: arguments
                    .iter()
                    .map(|argument| self.substitute_external_values(argument))
                    .collect::<Option<Vec<_>>>()?,
            }),
            Expr::Number(_) | Expr::Text(_) | Expr::Reference(_) | Expr::Range(_) => {
                Some(expression.clone())
            }
        }
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
                self.autosave();
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
            self.autosave();
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
                self.autosave();
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
                self.autosave();
            }
            Err(error) => self.status = format!("ERROR · {error}"),
        }
    }

    fn undo(&mut self) {
        let changed = self.spreadsheet.undo();
        self.status = if changed {
            self.refresh_market_data();
            format!(
                "UNDID CHANGE · {}",
                self.spreadsheet.workbook().active_sheet().name()
            )
        } else {
            "NOTHING TO UNDO".to_owned()
        };
        if changed {
            self.autosave();
        }
    }

    fn redo(&mut self) {
        let changed = self.spreadsheet.redo();
        self.status = if changed {
            self.refresh_market_data();
            format!(
                "REDID CHANGE · {}",
                self.spreadsheet.workbook().active_sheet().name()
            )
        } else {
            "NOTHING TO REDO".to_owned()
        };
        if changed {
            self.autosave();
        }
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
                self.autosave();
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

    fn import_csv_file(&mut self, location: &str) {
        let Some(file_store) = self.file_store.clone() else {
            self.status = "CSV FILE ACCESS IS DISABLED IN GALLERY MODE".to_owned();
            return;
        };
        if location.trim().is_empty() {
            self.status = "ERROR · SHEET IMPORT REQUIRES A CSV PATH".to_owned();
            return;
        }
        let csv = match file_store.read_csv(location) {
            Ok(csv) => csv,
            Err(error) => {
                self.status = format!("IMPORT ERROR · {error}");
                return;
            }
        };
        match self.spreadsheet.import_csv(&csv) {
            Ok(populated) => {
                self.cursor = CellAddress::new(1, 1).expect("A1 is in bounds");
                self.first_column = 1;
                self.first_row = 1;
                self.refresh_market_data();
                self.status = format!(
                    "IMPORTED {populated} POPULATED CELL(S) · {} · UNDO AVAILABLE",
                    location.trim()
                );
                self.autosave();
            }
            Err(error) => self.status = format!("IMPORT ERROR · {error}"),
        }
    }

    fn export_csv_file(&mut self, location: &str, overwrite: bool) {
        let Some(file_store) = self.file_store.clone() else {
            self.status = "CSV FILE ACCESS IS DISABLED IN GALLERY MODE".to_owned();
            return;
        };
        if location.trim().is_empty() {
            self.status = "ERROR · SHEET EXPORT REQUIRES A CSV PATH".to_owned();
            return;
        }
        let csv = self.spreadsheet.export_csv();
        self.status = match file_store.write_csv(location, &csv, overwrite) {
            Ok(()) => format!("EXPORTED {} BYTES · {}", csv.len(), location.trim()),
            Err(error) => format!("EXPORT ERROR · {error}"),
        };
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
        self.autosave();
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
                Style::new().bg(AMBER.into()).fg(BG.into()).bold(),
            ),
            Span::styled(" ƒx  ", CYAN),
            Span::styled(
                cursor,
                if editing {
                    Style::new().fg(CYAN.into())
                } else {
                    Style::new().fg(INK.into())
                },
            ),
        ]);
        frame.render_widget(
            Paragraph::new(line).block(Block::new().borders(Borders::ALL).border_style(border)),
            area,
        );
    }

    fn render_grid(&self, frame: &mut Frame, area: Rect) {
        let grid = self.grid_geometry(area);
        let columns = grid.columns;
        let rows = grid.rows;
        let evaluated = self.evaluated_spreadsheet();
        let visible_values = evaluated
            .visible_region(self.first_column, self.first_row, columns, rows)
            .expect("clamped viewport is in bounds")
            .into_iter()
            .map(|cell| (cell.address, cell.value))
            .collect::<HashMap<_, _>>();

        let mut widths = vec![Constraint::Length(ROW_HEADER_WIDTH)];
        widths.extend((0..columns).map(|_| Constraint::Length(CELL_WIDTH)));

        let mut header_cells = vec![TableCell::from("").style(Style::new().bg(NAV_BG.into()))];
        for column in self.first_column..self.first_column + columns {
            let name = char::from(b'A' + column - 1).to_string();
            let style = if column == self.cursor.column() {
                Style::new().bg(AMBER.into()).fg(BG.into()).bold()
            } else {
                Style::new().fg(AMBER.into()).add_modifier(Modifier::BOLD)
            };
            header_cells.push(TableCell::from(name).style(style));
        }
        let header = Row::new(header_cells).style(Style::new().bg(NAV_BG.into()));

        let table_rows = (self.first_row..self.first_row + rows)
            .map(|row| {
                let row_style = if row == self.cursor.row() {
                    Style::new().fg(AMBER.into()).add_modifier(Modifier::BOLD)
                } else {
                    Style::new().fg(MUTED.into())
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
                        Style::new()
                            .bg(CYAN.into())
                            .fg(BG.into())
                            .add_modifier(Modifier::BOLD)
                    } else {
                        value_style(&value)
                    };
                    cells.push(TableCell::from(truncate(&value, CELL_WIDTH as usize)).style(style));
                }
                Row::new(cells).style(Style::new().bg(BG.into()))
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
        frame.render_widget(
            Paragraph::new("").style(Style::new().bg(NAV_BG.into())),
            area,
        );
        for (index, tab_area) in self.tab_areas(area) {
            if index == usize::MAX {
                frame.render_widget(
                    Paragraph::new(" + ").style(Style::new().fg(AMBER.into()).bold()),
                    tab_area,
                );
                continue;
            }
            let sheet = &workbook.sheets()[index];
            let style = if index == workbook.active_sheet_index() {
                Style::new().bg(CYAN.into()).fg(BG.into()).bold()
            } else {
                Style::new().fg(MUTED.into())
            };
            frame.render_widget(
                Paragraph::new(format!(" {}:{} ", index + 1, sheet.name())).style(style),
                tab_area,
            );
        }
    }

    fn render_controls(&self, frame: &mut Frame, area: Rect) {
        for (control, control_area) in pack_control_areas(area) {
            let style = if self.control_enabled(control) {
                Style::new().fg(AMBER.into())
            } else {
                Style::new().fg(MUTED.into())
            };
            frame.render_widget(Paragraph::new(control.text()).style(style), control_area);
        }
    }

    fn render_status(&self, frame: &mut Frame, area: Rect) {
        let mode = if self.edit.is_some() { "EDIT" } else { "NAV" };
        let status = self
            .selected_external_status()
            .unwrap_or_else(|| self.status.clone());
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    format!(" {mode} "),
                    Style::new().bg(AMBER.into()).fg(BG.into()).bold(),
                ),
                Span::styled(format!(" {status}   "), INK),
            ]))
            .style(Style::new().bg(FOOTER_BG.into())),
            area,
        );
    }

    fn external_display(&self, address: CellAddress) -> Option<String> {
        let requests = self.external_cells.get(&address)?;
        let expression = self
            .spreadsheet
            .workbook()
            .active_sheet()
            .cell(address)
            .and_then(|cell| parse_formula(cell.raw()).ok())?;
        let request = self.financial_request(&expression)?;
        if requests.len() != 1 || requests.first() != Some(&request) {
            return None;
        }
        Some(match self.external_states.get(&request)? {
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
        let requests = self.external_cells.get(&self.cursor)?;
        if requests.len() > 1 {
            let ready = requests
                .iter()
                .filter(|request| {
                    matches!(
                        self.external_states.get(*request),
                        Some(ExternalCellState::Resolved(MarketDataState::Ready { .. }))
                            | Some(ExternalCellState::Resolved(MarketDataState::Stale { .. }))
                    )
                })
                .count();
            return Some(format!(
                "COMPOSITE FINANCIAL FORMULA · {ready}/{} INPUTS READY · {}",
                requests.len(),
                requests
                    .iter()
                    .map(|request| format!("{} {}", request.security, request.field))
                    .collect::<Vec<_>>()
                    .join(" · ")
            ));
        }
        let request = requests.first()?;
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
            "IMPORT" => self.import_csv_file(&name),
            "EXPORT" => self.export_csv_file(&name, false),
            "EXPORT!" => self.export_csv_file(&name, true),
            "SAVE" => self.save_workbook(&name, false),
            "LOAD" | "OPEN" => self.load_workbook(&name, false),
            "LIST" => self.list_workbooks(),
            "DROP" => self.delete_workbook(&name),
            "INSERT" => self.insert_result(&name),
            "FIND" | "MON" | "SEC" | "CHART" | "NEWS" => self.send_selected_instrument(&operation),
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
                    self.autosave();
                }
                Err(error) => self.status = format!("ERROR · {error}"),
            },
            "DELETE" | "REMOVE" => match self.spreadsheet.remove_active_sheet() {
                Ok(()) => {
                    self.status = format!(
                        "REMOVED SHEET · NOW ON {}",
                        self.spreadsheet.workbook().active_sheet().name()
                    );
                    self.autosave();
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
            KeyCode::Enter => self.begin_edit(None),
            KeyCode::Char('=') => self.begin_edit(Some("=")),
            KeyCode::Delete => self.clear_selected(),
            KeyCode::F(9) => self.refresh_market_data(),
            KeyCode::Char('s') => {
                if !self.activate_control(SpreadsheetControl::Security) {
                    return false;
                }
            }
            KeyCode::Char('c') => {
                if !self.activate_control(SpreadsheetControl::Chart) {
                    return false;
                }
            }
            KeyCode::Char('n') => {
                if !self.activate_control(SpreadsheetControl::News) {
                    return false;
                }
            }
            _ => return false,
        }
        true
    }

    fn handle_mouse(&mut self, event: MouseEvent, area: Rect) -> bool {
        let areas = spreadsheet_areas(area);
        if let Some(key) = scroll_key(event, areas.grid) {
            return self.handle_key(key);
        }
        if !is_primary_click(event, area) {
            return false;
        }
        if self.edit.is_some() {
            if contains(formula_action_area(areas.formula), event.column, event.row) {
                return true;
            }
            self.commit_edit();
        }
        for action in self.actions(area) {
            if contains(action.area, event.column, event.row) {
                return !action.enabled || self.activate_action(&action.id);
            }
        }
        true
    }

    fn actions(&self, area: Rect) -> Vec<WorkspaceAction> {
        let areas = spreadsheet_areas(area);
        let grid = self.grid_geometry(areas.grid);
        let sheet_identity = self.active_sheet_identity();
        let mut actions = Vec::new();

        if let Some(cell_area) = grid.cell_area(self.cursor.column(), self.cursor.row()) {
            actions.push(
                WorkspaceAction::new(
                    format!("cell:{sheet_identity:016x}:{}", self.cursor),
                    cell_action_label(self.cursor, &self.selected_raw()),
                    cell_area,
                )
                .preferred(),
            );
        }
        let formula_area = formula_action_area(areas.formula);
        if formula_area.width > 0 && formula_area.height > 0 {
            let mut formula = WorkspaceAction::new(
                format!("formula:{sheet_identity:016x}:{}", self.cursor),
                format!("Edit {} in the formula bar", self.cursor),
                formula_area,
            );
            if self.edit.is_some() {
                formula = formula.disabled();
            }
            actions.push(formula);
        }

        let workbook = self.spreadsheet.workbook();
        for (index, tab_area) in self.tab_areas(areas.tabs) {
            if index == usize::MAX {
                actions.push(WorkspaceAction::new(
                    "sheet:add",
                    "Add a worksheet",
                    tab_area,
                ));
                continue;
            }
            let sheet = &workbook.sheets()[index];
            let identity = stable_identity(sheet.name());
            actions.push(WorkspaceAction::new(
                format!("sheet:{index}:{identity:016x}"),
                format!("Select worksheet {}", sheet.name()),
                tab_area,
            ));
        }

        actions.extend(pack_control_areas(areas.controls).into_iter().map(
            |(control, control_area)| {
                let mut action = WorkspaceAction::new(
                    control.action_id(),
                    self.control_label(control),
                    control_area,
                );
                if !self.control_enabled(control) {
                    action = action.disabled();
                }
                action
            },
        ));

        for row in grid.first_row..grid.first_row.saturating_add(grid.rows) {
            for column in grid.first_column..grid.first_column.saturating_add(grid.columns) {
                let address = CellAddress::new(column, row).expect("visible cell is in bounds");
                if address == self.cursor {
                    continue;
                }
                let raw = workbook
                    .active_sheet()
                    .cell(address)
                    .map_or("", |cell| cell.raw());
                actions.push(WorkspaceAction::new(
                    format!("cell:{sheet_identity:016x}:{address}"),
                    cell_action_label(address, raw),
                    grid.cell_area(column, row).expect("visible cell geometry"),
                ));
            }
            actions.push(WorkspaceAction::new(
                format!("row:{sheet_identity:016x}:{row}"),
                format!("Select row {row} at column {}", self.cursor.column()),
                grid.row_header_area(row)
                    .expect("visible row header geometry"),
            ));
        }
        actions
    }

    fn activate_action(&mut self, id: &str) -> bool {
        if self.edit.is_some() {
            return false;
        }
        if let Some(control) = SpreadsheetControl::from_action_id(id) {
            return self.activate_control(control);
        }
        if id == "sheet:add" {
            self.add_sheet(None);
            return true;
        }
        if let Some(sheet) = id.strip_prefix("sheet:") {
            let Some((index, expected_identity)) = sheet.split_once(':') else {
                return false;
            };
            let (Ok(index), Ok(expected_identity)) = (
                index.parse::<usize>(),
                u64::from_str_radix(expected_identity, 16),
            ) else {
                return false;
            };
            let Some(name) = self
                .spreadsheet
                .workbook()
                .sheets()
                .get(index)
                .map(|sheet| sheet.name().to_owned())
            else {
                return false;
            };
            if stable_identity(&name) != expected_identity {
                return false;
            }
            self.select_sheet(&name);
            return true;
        }
        let (edit_formula, cell) = if let Some(row) = id.strip_prefix("row:") {
            let Some((expected_identity, row)) = row.split_once(':') else {
                return false;
            };
            let (Ok(expected_identity), Ok(row)) = (
                u64::from_str_radix(expected_identity, 16),
                row.parse::<u16>(),
            ) else {
                return false;
            };
            if expected_identity != self.active_sheet_identity()
                || row < self.first_row
                || row >= self.first_row.saturating_add(self.visible_rows.get())
            {
                return false;
            }
            self.cursor = CellAddress::new(self.cursor.column(), row)
                .expect("visible row and current column are in bounds");
            self.status = format!("SELECTED {}", self.cursor);
            return true;
        } else if let Some(cell) = id.strip_prefix("formula:") {
            (true, cell)
        } else if let Some(cell) = id.strip_prefix("cell:") {
            (false, cell)
        } else {
            return false;
        };
        let Some((expected_identity, address)) = cell.split_once(':') else {
            return false;
        };
        let (Ok(expected_identity), Ok(address)) = (
            u64::from_str_radix(expected_identity, 16),
            address.parse::<CellAddress>(),
        ) else {
            return false;
        };
        if expected_identity != self.active_sheet_identity() {
            return false;
        }
        let column_visible = address.column() >= self.first_column
            && address.column() < self.first_column.saturating_add(self.visible_columns.get());
        let row_visible = address.row() >= self.first_row
            && address.row() < self.first_row.saturating_add(self.visible_rows.get());
        if !column_visible || !row_visible {
            return false;
        }
        if edit_formula && (address != self.cursor || self.edit.is_some()) {
            return false;
        }
        self.cursor = address;
        self.status = format!("SELECTED {address}");
        if edit_formula {
            self.begin_edit(None);
        }
        true
    }

    fn poll_intents(&mut self) -> Vec<AppIntent> {
        self.poll_market_data();
        std::mem::take(&mut self.pending_intents)
    }

    fn on_blur(&mut self) {
        if self.edit.is_some() {
            self.cancel_edit();
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let areas = spreadsheet_areas(area);
        self.render_formula_bar(frame, areas.formula);
        self.render_grid(frame, areas.grid);
        self.render_tabs(frame, areas.tabs);
        self.render_controls(frame, areas.controls);
        self.render_status(frame, areas.status);
    }
}

fn stable_identity(value: &str) -> u64 {
    value
        .as_bytes()
        .iter()
        .fold(0xcbf29ce484222325, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        })
}

fn cell_action_label(address: CellAddress, raw: &str) -> String {
    if raw.is_empty() {
        format!("Select blank cell {address}")
    } else {
        format!("Select {address} · {}", truncate(raw, 40))
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
        Style::new().fg(RED.into())
    } else if value.starts_with('…') {
        Style::new().fg(YELLOW.into())
    } else if value.starts_with('~') {
        Style::new().fg(AMBER.into())
    } else {
        Style::new().fg(INK.into())
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
    use crate::features::spreadsheet::{
        MarketDataPoint, MarketDataProvenance, MarketDataQuality, SpreadsheetFileError,
        SpreadsheetWorkbookStore, StoredWorkbook,
    };
    use crossterm::event::{MouseButton, MouseEventKind};
    use std::{collections::HashSet, sync::Mutex};

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
                        ("IBM US Equity", "PX_LAST") => 234.19,
                        ("IBM US Equity", "HISTORY|PX_LAST|2026-01-01|2026-08-26") => 234.19,
                        ("IBM US Equity", "FUNDAMENTAL|REVENUE|FY2025") => 67_500_000_000.0,
                        (_, "CHG_PCT_1D") => 1.0,
                        _ => return None,
                    };
                    Some(MarketDataPoint::ready(request.clone(), value, provenance()))
                })
                .collect()
        }
    }

    struct MemoryFileStore {
        input: String,
        writes: Mutex<Vec<(String, String, bool)>>,
    }

    impl SpreadsheetFileStore for MemoryFileStore {
        fn read_csv(&self, location: &str) -> Result<String, SpreadsheetFileError> {
            if location == "input.csv" {
                Ok(self.input.clone())
            } else {
                Err(SpreadsheetFileError::Io("NOT FOUND".to_owned()))
            }
        }

        fn write_csv(
            &self,
            location: &str,
            csv: &str,
            overwrite: bool,
        ) -> Result<(), SpreadsheetFileError> {
            self.writes.lock().expect("writes lock").push((
                location.to_owned(),
                csv.to_owned(),
                overwrite,
            ));
            Ok(())
        }
    }

    #[derive(Default)]
    struct MemoryWorkbookStore {
        workbooks: Mutex<HashMap<String, StoredWorkbook>>,
    }

    impl SpreadsheetWorkbookStore for MemoryWorkbookStore {
        fn load_workbook(&self, id: &str) -> Result<Option<StoredWorkbook>, SpreadsheetFileError> {
            Ok(self
                .workbooks
                .lock()
                .expect("workbooks lock")
                .get(id)
                .cloned())
        }

        fn save_workbook(&self, workbook: &StoredWorkbook) -> Result<(), SpreadsheetFileError> {
            self.workbooks
                .lock()
                .expect("workbooks lock")
                .insert(workbook.id.clone(), workbook.clone());
            Ok(())
        }

        fn list_workbooks(&self) -> Result<Vec<String>, SpreadsheetFileError> {
            let mut ids = self
                .workbooks
                .lock()
                .expect("workbooks lock")
                .keys()
                .cloned()
                .collect::<Vec<_>>();
            ids.sort();
            Ok(ids)
        }

        fn delete_workbook(&self, id: &str) -> Result<bool, SpreadsheetFileError> {
            Ok(self
                .workbooks
                .lock()
                .expect("workbooks lock")
                .remove(id)
                .is_some())
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
            "IBM US Equity"
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
            CellValue::Number(58_547.5)
        );
        assert_eq!(
            evaluated.cell("E7").unwrap().value,
            CellValue::Number(58_547.5)
        );
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
    fn persistent_constructor_starts_with_a_truthful_empty_workbook() {
        let files = Arc::new(MemoryFileStore {
            input: String::new(),
            writes: Mutex::new(Vec::new()),
        });
        let workspace = SpreadsheetWorkspace::empty(Arc::new(StubMarketData), files);

        assert!(workspace.spreadsheet.cell("A1").unwrap().raw.is_empty());
        assert_eq!(workspace.spreadsheet.workbook().sheet_count(), 1);
        assert!(workspace.status.contains("EMPTY WORKBOOK"));
        assert!(workspace.external_cells.is_empty());
    }

    #[test]
    fn persistent_workspace_autosaves_and_restores_a_complete_workbook() {
        let files = Arc::new(MemoryFileStore {
            input: String::new(),
            writes: Mutex::new(Vec::new()),
        });
        let workbooks = Arc::new(MemoryWorkbookStore::default());
        let mut workspace = SpreadsheetWorkspace::persistent(
            Arc::new(StubMarketData),
            files.clone(),
            workbooks.clone(),
        );
        workspace
            .spreadsheet
            .set_cell("A1", "IBM US Equity")
            .unwrap();
        workspace
            .spreadsheet
            .set_cell("B1", "=PX_LAST(A1)")
            .unwrap();
        workspace.spreadsheet.add_sheet("Model").unwrap();
        workspace.spreadsheet.select_sheet("Model").unwrap();
        workspace
            .spreadsheet
            .set_cell("A1", "=POWER(2, 8)")
            .unwrap();
        workspace.autosave();

        let restored =
            SpreadsheetWorkspace::persistent(Arc::new(StubMarketData), files, workbooks.clone());
        assert_eq!(restored.spreadsheet.workbook().sheet_count(), 2);
        assert_eq!(
            restored.spreadsheet.workbook().active_sheet().name(),
            "Model"
        );
        assert_eq!(restored.spreadsheet.cell("A1").unwrap().raw, "=POWER(2, 8)");
        assert_eq!(restored.workbook_revision, 1);
        assert!(restored.status.contains("LOADED WORKBOOK default"));
        assert!(workbooks
            .workbooks
            .lock()
            .expect("workbooks lock")
            .contains_key("default"));
    }

    #[test]
    fn financial_functions_compose_with_arithmetic_and_each_other() {
        let mut workspace = workspace();
        workspace
            .spreadsheet
            .set_cell("A20", "IBM US Equity")
            .unwrap();
        workspace
            .spreadsheet
            .set_cell(
                "B20",
                "=PX_LAST(A20)*2+FUNDAMENTAL(A20, \"REVENUE\", \"FY2025\")/1000000000",
            )
            .unwrap();
        workspace
            .spreadsheet
            .set_cell(
                "C20",
                "=HISTORY(A20, \"PX_LAST\", \"2026-01-01\", \"2026-08-26\")",
            )
            .unwrap();
        workspace.refresh_market_data();
        wait_for_data(&mut workspace);

        let evaluated = workspace.evaluated_spreadsheet();
        assert_eq!(
            evaluated.cell("B20").unwrap().value,
            CellValue::Number(535.88)
        );
        assert_eq!(
            evaluated.cell("C20").unwrap().value,
            CellValue::Number(234.19)
        );
    }

    #[test]
    fn sheet_commands_import_and_export_real_csv_through_the_file_port() {
        let files = Arc::new(MemoryFileStore {
            input: "Security,Formula\nIBM US Equity,=PX_LAST(A2)".to_owned(),
            writes: Mutex::new(Vec::new()),
        });
        let mut workspace = SpreadsheetWorkspace::empty(Arc::new(StubMarketData), files.clone());

        workspace.handle_command(&CommandInvocation {
            function: "SHEET".to_owned(),
            args: vec!["IMPORT".to_owned(), "input.csv".to_owned()],
        });
        assert_eq!(
            workspace.spreadsheet.cell("A2").unwrap().raw,
            "IBM US Equity"
        );
        assert_eq!(
            workspace.spreadsheet.cell("B2").unwrap().raw,
            "=PX_LAST(A2)"
        );
        assert!(workspace.status.contains("IMPORTED 4 POPULATED CELL(S)"));

        workspace.handle_command(&CommandInvocation {
            function: "SHEET".to_owned(),
            args: vec!["EXPORT!".to_owned(), "output.csv".to_owned()],
        });
        let writes = files.writes.lock().expect("writes lock");
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].0, "output.csv");
        assert!(writes[0].1.contains("=PX_LAST(A2)"));
        assert!(writes[0].2);
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
        assert!(status.contains("IBM US Equity · PX_LAST · TEST FEED"));
        assert!(status.contains("OBS 2026-08-26T13:00:00-07:00"));
        assert!(status.contains("REALTIME"));
    }

    #[test]
    fn financial_cells_render_stale_permission_and_unavailable_states() {
        let mut workspace = workspace();
        let ibm = MarketDataRequest::new("IBM US Equity", "PX_LAST");
        workspace.external_states.insert(
            ibm,
            ExternalCellState::Resolved(MarketDataState::Stale {
                value: 529.0,
                provenance: provenance(),
            }),
        );
        assert_eq!(
            workspace.external_display("B2".parse().unwrap()),
            Some("~529".to_owned())
        );
        let ibm = MarketDataRequest::new("IBM US Equity", "PX_LAST");
        workspace.external_states.insert(
            ibm.clone(),
            ExternalCellState::Resolved(MarketDataState::PermissionDenied {
                provider: "TEST FEED".to_owned(),
            }),
        );
        assert_eq!(
            workspace.external_display("B2".parse().unwrap()),
            Some("#DENIED".to_owned())
        );
        workspace.external_states.insert(
            ibm,
            ExternalCellState::Resolved(MarketDataState::Unavailable {
                reason: "no observation".to_owned(),
            }),
        );
        assert_eq!(
            workspace.external_display("B2".parse().unwrap()),
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

        assert!(workspace.handle_key(key(KeyCode::Char('x'))));
        let row_four = workspace
            .actions(area)
            .into_iter()
            .find(|action| action.id.starts_with("row:") && action.id.ends_with(":4"))
            .unwrap();
        assert!(workspace.handle_mouse(click(row_four.area.x, row_four.area.y), area));
        assert!(workspace.edit.is_none());
        assert_eq!(workspace.cursor.to_string(), "C4");
        assert_eq!(workspace.spreadsheet.cell("C3").unwrap().raw, "x");
    }

    #[test]
    fn actions_share_geometry_revalidate_sheet_identity_and_route_research() {
        let mut workspace = workspace();
        let area = Rect::new(0, 0, 120, 30);
        let actions = workspace.actions(area);
        let ids = actions
            .iter()
            .map(|action| action.id.as_str())
            .collect::<HashSet<_>>();
        assert_eq!(ids.len(), actions.len());
        assert!(actions.iter().all(|action| {
            action.area.width > 0
                && action.area.height > 0
                && action.area.x >= area.x
                && action.area.y >= area.y
                && action.area.right() <= area.right()
                && action.area.bottom() <= area.bottom()
        }));
        assert!(actions
            .iter()
            .any(|action| action.id.ends_with(":A1") && action.preferred));

        let c3 = actions
            .iter()
            .find(|action| action.id.starts_with("cell:") && action.id.ends_with(":C3"))
            .unwrap();
        assert!(workspace.handle_mouse(click(c3.area.x, c3.area.y), area));
        assert_eq!(workspace.cursor.to_string(), "C3");

        let a2 = workspace
            .actions(area)
            .into_iter()
            .find(|action| action.id.starts_with("cell:") && action.id.ends_with(":A2"))
            .unwrap();
        assert!(workspace.activate_action(&a2.id));
        assert!(workspace
            .actions(area)
            .iter()
            .any(|action| action.id == "control:security" && action.enabled));
        assert!(workspace.activate_action("control:security"));
        assert_eq!(
            workspace.poll_intents(),
            vec![AppIntent::DispatchCommand {
                command: "SEC IBM US Equity".to_owned(),
                origin: ID,
            }]
        );

        let assumptions = workspace
            .actions(area)
            .into_iter()
            .find(|action| action.label == "Select worksheet Assumptions")
            .unwrap();
        assert!(workspace.activate_action(&assumptions.id));
        assert!(!workspace.activate_action(&a2.id));
        workspace
            .spreadsheet
            .rename_active_sheet("Renamed Assumptions")
            .unwrap();
        assert!(!workspace.activate_action(&assumptions.id));
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
        workspace.handle_key(key(KeyCode::Enter));
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
    fn selected_text_cells_dispatch_research_intents_without_feature_imports() {
        let mut workspace = workspace();
        workspace.cursor = "A2".parse().unwrap();
        workspace.handle_command(&CommandInvocation {
            function: "SHEET".to_owned(),
            args: vec!["SEC".to_owned()],
        });
        assert_eq!(
            workspace.poll_intents(),
            vec![AppIntent::DispatchCommand {
                command: "SEC IBM US Equity".to_owned(),
                origin: ID,
            }]
        );
    }

    #[test]
    fn insert_command_writes_a_research_result_to_the_selected_cell() {
        let files = Arc::new(MemoryFileStore {
            input: String::new(),
            writes: Mutex::new(Vec::new()),
        });
        let mut workspace = SpreadsheetWorkspace::empty(Arc::new(StubMarketData), files);
        workspace.cursor = "C4".parse().unwrap();
        workspace.handle_command(&CommandInvocation {
            function: "SHEET".to_owned(),
            args: vec!["INSERT".to_owned(), "MSFT".to_owned(), "US".to_owned()],
        });
        assert_eq!(workspace.spreadsheet.cell("C4").unwrap().raw, "MSFT US");
        assert!(workspace.status.contains("INSERTED RESULT INTO C4"));
    }

    #[test]
    fn keyboard_undo_and_redo_restore_committed_edits() {
        let mut workspace = workspace();
        let original = workspace.spreadsheet.cell("A1").unwrap().raw;
        workspace.handle_key(key(KeyCode::Enter));
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
