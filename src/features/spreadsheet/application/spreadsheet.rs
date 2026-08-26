use std::fmt;

use crate::features::spreadsheet::domain::{
    translate_formula, AddressError, CellAddress, CellValue, FormulaError, Workbook, WorkbookError,
    MAX_COLUMNS, MAX_ROWS,
};

const HISTORY_LIMIT: usize = 100;

#[derive(Debug, Clone)]
pub struct Spreadsheet {
    workbook: Workbook,
    undo_stack: Vec<Workbook>,
    redo_stack: Vec<Workbook>,
}

impl Spreadsheet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_workbook(workbook: Workbook) -> Self {
        Self {
            workbook,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    pub fn workbook(&self) -> &Workbook {
        &self.workbook
    }

    pub fn into_workbook(self) -> Workbook {
        self.workbook
    }

    pub fn add_sheet(&mut self, name: impl Into<String>) -> Result<usize, SpreadsheetError> {
        let name = name.into();
        self.record_change(|workbook| workbook.add_sheet(name).map_err(SpreadsheetError::Workbook))
    }

    pub fn select_sheet(&mut self, name: &str) -> Result<(), SpreadsheetError> {
        self.workbook
            .select_sheet(name)
            .map_err(SpreadsheetError::Workbook)
    }

    pub fn select_next_sheet(&mut self) {
        self.workbook.select_next_sheet();
    }

    pub fn select_previous_sheet(&mut self) {
        self.workbook.select_previous_sheet();
    }

    pub fn rename_active_sheet(&mut self, name: impl Into<String>) -> Result<(), SpreadsheetError> {
        let name = name.into();
        self.record_change(|workbook| {
            workbook
                .rename_active_sheet(name)
                .map_err(SpreadsheetError::Workbook)
        })
    }

    pub fn remove_active_sheet(&mut self) -> Result<(), SpreadsheetError> {
        self.record_change(|workbook| {
            workbook
                .remove_active_sheet()
                .map(|_| ())
                .map_err(SpreadsheetError::Workbook)
        })
    }

    pub fn set_cell(
        &mut self,
        address: &str,
        raw: impl Into<String>,
    ) -> Result<CellValue, SpreadsheetError> {
        let address = parse_address(address)?;
        let raw = raw.into();
        self.record_change(|workbook| {
            workbook.active_sheet_mut().set(address, raw);
            Ok(workbook.active_value(address))
        })
    }

    /// Copies one cell to another, translating relative formula references.
    pub fn copy_cell(&mut self, source: &str, target: &str) -> Result<CellValue, SpreadsheetError> {
        let source = parse_address(source)?;
        let target = parse_address(target)?;
        let raw = self
            .workbook
            .active_sheet()
            .cell(source)
            .map(|cell| cell.raw().to_owned())
            .unwrap_or_default();
        let translated = translate_raw(&raw, source, target)?;
        self.record_change(|workbook| {
            workbook.active_sheet_mut().set(target, translated);
            Ok(workbook.active_value(target))
        })
    }

    /// Fills a set of destinations from one origin as one atomic undo step.
    pub fn fill_cells<I, A>(&mut self, source: &str, targets: I) -> Result<usize, SpreadsheetError>
    where
        I: IntoIterator<Item = A>,
        A: AsRef<str>,
    {
        let source = parse_address(source)?;
        let raw = self
            .workbook
            .active_sheet()
            .cell(source)
            .map(|cell| cell.raw().to_owned())
            .unwrap_or_default();
        let translated = targets
            .into_iter()
            .map(|target| {
                let target = parse_address(target.as_ref())?;
                Ok((target, translate_raw(&raw, source, target)?))
            })
            .collect::<Result<Vec<_>, SpreadsheetError>>()?;
        let count = translated.len();
        self.record_change(|workbook| {
            for (target, raw) in translated {
                workbook.active_sheet_mut().set(target, raw);
            }
            Ok(count)
        })
    }

    /// Applies a group of edits as one undoable operation.
    ///
    /// Addresses are fully validated before the workbook is changed, so a bad
    /// address cannot leave a partially updated sheet.
    pub fn set_cells<I, A, R>(&mut self, cells: I) -> Result<usize, SpreadsheetError>
    where
        I: IntoIterator<Item = (A, R)>,
        A: AsRef<str>,
        R: Into<String>,
    {
        let cells = cells
            .into_iter()
            .map(|(address, raw)| Ok((parse_address(address.as_ref())?, raw.into())))
            .collect::<Result<Vec<_>, SpreadsheetError>>()?;
        let cell_count = cells.len();
        self.record_change(|workbook| {
            let sheet = workbook.active_sheet_mut();
            for (address, raw) in cells {
                sheet.set(address, raw);
            }
            Ok(cell_count)
        })
    }

    pub fn clear_cell(&mut self, address: &str) -> Result<(), SpreadsheetError> {
        let address = parse_address(address)?;
        self.record_change(|workbook| {
            workbook.active_sheet_mut().clear(address);
            Ok(())
        })
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    pub fn undo(&mut self) -> bool {
        let Some(previous) = self.undo_stack.pop() else {
            return false;
        };
        let current = std::mem::replace(&mut self.workbook, previous);
        push_bounded(&mut self.redo_stack, current);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo_stack.pop() else {
            return false;
        };
        let current = std::mem::replace(&mut self.workbook, next);
        push_bounded(&mut self.undo_stack, current);
        true
    }

    /// Drops transient editing history after loading or seeding a workbook.
    pub fn clear_history(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
    }

    /// Replaces the active sheet with raw CSV fields as one undoable edit.
    ///
    /// This is deliberately text-based: filesystem selection and persistence
    /// belong to adapters outside the deterministic spreadsheet core.
    pub fn import_csv(&mut self, csv: &str) -> Result<usize, SpreadsheetError> {
        let rows = parse_csv(csv).map_err(SpreadsheetError::Csv)?;
        if rows.len() > usize::from(MAX_ROWS) {
            return Err(SpreadsheetError::Csv(CsvError::TooManyRows(rows.len())));
        }
        for (row_index, row) in rows.iter().enumerate() {
            if row.len() > usize::from(MAX_COLUMNS) {
                return Err(SpreadsheetError::Csv(CsvError::TooManyColumns {
                    row: row_index + 1,
                    columns: row.len(),
                }));
            }
        }
        let populated = rows
            .iter()
            .flat_map(|row| row.iter())
            .filter(|value| !value.is_empty())
            .count();
        self.record_change(|workbook| {
            let sheet = workbook.active_sheet_mut();
            sheet.clear_all();
            for (row_index, row) in rows.into_iter().enumerate() {
                for (column_index, raw) in row.into_iter().enumerate() {
                    if raw.is_empty() {
                        continue;
                    }
                    let address = CellAddress::new(
                        u8::try_from(column_index + 1).expect("CSV columns were validated"),
                        u16::try_from(row_index + 1).expect("CSV rows were validated"),
                    )
                    .expect("CSV dimensions were validated");
                    sheet.set(address, raw);
                }
            }
            Ok(populated)
        })
    }

    /// Exports raw cell contents, including formulas, in a minimal bounding box.
    pub fn export_csv(&self) -> String {
        let sheet = self.workbook.active_sheet();
        let Some(max_column) = sheet
            .populated_cells()
            .map(|(address, _)| address.column())
            .max()
        else {
            return String::new();
        };
        let max_row = sheet
            .populated_cells()
            .map(|(address, _)| address.row())
            .max()
            .expect("a maximum column implies at least one cell");
        let mut csv = String::new();
        for row in 1..=max_row {
            if row > 1 {
                csv.push('\n');
            }
            for column in 1..=max_column {
                if column > 1 {
                    csv.push(',');
                }
                let address = CellAddress::new(column, row).expect("export bounds come from cells");
                let raw = sheet
                    .cell(address)
                    .map(|cell| cell.raw())
                    .unwrap_or_default();
                write_csv_field(&mut csv, raw);
            }
        }
        csv
    }

    pub fn cell(&self, address: &str) -> Result<CellView, SpreadsheetError> {
        let address = parse_address(address)?;
        let sheet = self.workbook.active_sheet();
        Ok(CellView {
            address,
            raw: sheet
                .cell(address)
                .map(|cell| cell.raw().to_owned())
                .unwrap_or_default(),
            value: self.workbook.active_value(address),
        })
    }

    pub fn visible_region(
        &self,
        start_column: u8,
        start_row: u16,
        columns: u8,
        rows: u16,
    ) -> Result<Vec<CellView>, SpreadsheetError> {
        if columns == 0 || rows == 0 {
            return Err(SpreadsheetError::RegionOutOfBounds);
        }
        let end_column = start_column
            .checked_add(columns.saturating_sub(1))
            .ok_or(SpreadsheetError::RegionOutOfBounds)?;
        let end_row = start_row
            .checked_add(rows.saturating_sub(1))
            .ok_or(SpreadsheetError::RegionOutOfBounds)?;
        let sheet = self.workbook.active_sheet();
        let values = self.workbook.active_values();
        let mut cells = Vec::with_capacity(columns as usize * rows as usize);
        for row in start_row..=end_row {
            for column in start_column..=end_column {
                let address = CellAddress::new(column, row).map_err(SpreadsheetError::Address)?;
                cells.push(CellView {
                    address,
                    raw: sheet
                        .cell(address)
                        .map(|cell| cell.raw().to_owned())
                        .unwrap_or_default(),
                    value: values.get(&address).cloned().unwrap_or(CellValue::Blank),
                });
            }
        }
        Ok(cells)
    }

    fn record_change<T>(
        &mut self,
        operation: impl FnOnce(&mut Workbook) -> Result<T, SpreadsheetError>,
    ) -> Result<T, SpreadsheetError> {
        let before = self.workbook.clone();
        match operation(&mut self.workbook) {
            Ok(result) => {
                if self.workbook != before {
                    push_bounded(&mut self.undo_stack, before);
                    self.redo_stack.clear();
                }
                Ok(result)
            }
            Err(error) => {
                self.workbook = before;
                Err(error)
            }
        }
    }
}

impl Default for Spreadsheet {
    fn default() -> Self {
        Self::from_workbook(Workbook::new())
    }
}

fn push_bounded(history: &mut Vec<Workbook>, workbook: Workbook) {
    if history.len() == HISTORY_LIMIT {
        history.remove(0);
    }
    history.push(workbook);
}

fn parse_address(input: &str) -> Result<CellAddress, SpreadsheetError> {
    input.parse().map_err(SpreadsheetError::Address)
}

fn translate_raw(
    raw: &str,
    source: CellAddress,
    target: CellAddress,
) -> Result<String, SpreadsheetError> {
    if !raw.trim_start().starts_with('=') {
        return Ok(raw.to_owned());
    }
    let column_delta = i16::from(target.column()) - i16::from(source.column());
    let row_delta = i32::from(target.row()) - i32::from(source.row());
    translate_formula(raw, column_delta, row_delta).map_err(SpreadsheetError::Formula)
}

#[derive(Debug, Clone, PartialEq)]
pub struct CellView {
    pub address: CellAddress,
    pub raw: String,
    pub value: CellValue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpreadsheetError {
    Address(AddressError),
    Workbook(WorkbookError),
    Formula(FormulaError),
    Csv(CsvError),
    RegionOutOfBounds,
}

impl fmt::Display for SpreadsheetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Address(error) => error.fmt(formatter),
            Self::Workbook(error) => error.fmt(formatter),
            Self::Formula(error) => error.fmt(formatter),
            Self::Csv(error) => error.fmt(formatter),
            Self::RegionOutOfBounds => write!(formatter, "visible region is outside A1:Z100"),
        }
    }
}

impl std::error::Error for SpreadsheetError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CsvError {
    Malformed,
    TooManyRows(usize),
    TooManyColumns { row: usize, columns: usize },
}

impl fmt::Display for CsvError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed => write!(formatter, "CSV contains an invalid quoted field"),
            Self::TooManyRows(rows) => {
                write!(formatter, "CSV has {rows} rows; the limit is {MAX_ROWS}")
            }
            Self::TooManyColumns { row, columns } => {
                write!(
                    formatter,
                    "CSV row {row} has {columns} columns; the limit is {MAX_COLUMNS}"
                )
            }
        }
    }
}

impl std::error::Error for CsvError {}

fn write_csv_field(output: &mut String, field: &str) {
    if field
        .chars()
        .any(|character| matches!(character, ',' | '"' | '\r' | '\n'))
    {
        output.push('"');
        for character in field.chars() {
            if character == '"' {
                output.push('"');
            }
            output.push(character);
        }
        output.push('"');
    } else {
        output.push_str(field);
    }
}

fn parse_csv(input: &str) -> Result<Vec<Vec<String>>, CsvError> {
    if input.is_empty() {
        return Ok(Vec::new());
    }

    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut characters = input.chars().peekable();
    let mut in_quotes = false;
    let mut closed_quote = false;
    let mut ended_record = false;

    while let Some(character) = characters.next() {
        if in_quotes {
            if character == '"' {
                if characters.peek() == Some(&'"') {
                    characters.next();
                    field.push('"');
                } else {
                    in_quotes = false;
                    closed_quote = true;
                }
            } else {
                field.push(character);
            }
            ended_record = false;
            continue;
        }

        match character {
            '"' if field.is_empty() && !closed_quote => {
                in_quotes = true;
                ended_record = false;
            }
            '"' => return Err(CsvError::Malformed),
            ',' => {
                row.push(std::mem::take(&mut field));
                closed_quote = false;
                ended_record = false;
            }
            '\r' | '\n' => {
                if character == '\r' && characters.peek() == Some(&'\n') {
                    characters.next();
                }
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
                closed_quote = false;
                ended_record = true;
            }
            _ if closed_quote => return Err(CsvError::Malformed),
            _ => {
                field.push(character);
                ended_record = false;
            }
        }
    }

    if in_quotes {
        return Err(CsvError::Malformed);
    }
    if !ended_record {
        row.push(field);
        rows.push(row);
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn application_facade_accepts_user_addresses_and_recalculates() {
        let mut spreadsheet = Spreadsheet::new();
        spreadsheet.set_cell("A1", "100").unwrap();
        spreadsheet.set_cell("B1", "=A1 * 1.2").unwrap();
        assert_eq!(
            spreadsheet.cell("B1").unwrap().value,
            CellValue::Number(120.0)
        );
        spreadsheet.set_cell("A1", "200").unwrap();
        assert_eq!(
            spreadsheet.cell("B1").unwrap().value,
            CellValue::Number(240.0)
        );
    }

    #[test]
    fn visible_region_is_row_major_and_includes_blank_cells() {
        let mut spreadsheet = Spreadsheet::new();
        spreadsheet.set_cell("B2", "7").unwrap();
        let region = spreadsheet.visible_region(1, 1, 2, 2).unwrap();
        assert_eq!(
            region
                .iter()
                .map(|cell| cell.address.to_string())
                .collect::<Vec<_>>(),
            ["A1", "B1", "A2", "B2"]
        );
        assert_eq!(region[3].value, CellValue::Number(7.0));
    }

    #[test]
    fn validates_region_boundaries() {
        let spreadsheet = Spreadsheet::new();
        assert_eq!(
            spreadsheet.visible_region(26, 100, 2, 1),
            Err(SpreadsheetError::Address(AddressError::ColumnOutOfBounds(
                27
            )))
        );
    }

    #[test]
    fn undo_and_redo_restore_formula_dependencies_and_branch_history() {
        let mut spreadsheet = Spreadsheet::new();
        spreadsheet.set_cell("A1", "10").unwrap();
        spreadsheet.set_cell("B1", "=A1*2").unwrap();
        spreadsheet.clear_history();

        spreadsheet.set_cell("A1", "25").unwrap();
        assert_eq!(
            spreadsheet.cell("B1").unwrap().value,
            CellValue::Number(50.0)
        );
        assert!(spreadsheet.can_undo());
        assert!(spreadsheet.undo());
        assert_eq!(spreadsheet.cell("A1").unwrap().raw, "10");
        assert_eq!(
            spreadsheet.cell("B1").unwrap().value,
            CellValue::Number(20.0)
        );
        assert!(spreadsheet.can_redo());
        assert!(spreadsheet.redo());
        assert_eq!(spreadsheet.cell("A1").unwrap().raw, "25");

        assert!(spreadsheet.undo());
        spreadsheet.set_cell("A1", "40").unwrap();
        assert!(!spreadsheet.can_redo());
        assert!(!spreadsheet.redo());
    }

    #[test]
    fn bulk_edits_validate_first_and_undo_as_one_operation() {
        let mut spreadsheet = Spreadsheet::new();
        let error = spreadsheet
            .set_cells([("A1", "saved"), ("AA1", "invalid")])
            .unwrap_err();
        assert!(matches!(error, SpreadsheetError::Address(_)));
        assert_eq!(spreadsheet.cell("A1").unwrap().value, CellValue::Blank);
        assert!(!spreadsheet.can_undo());

        spreadsheet
            .set_cells([("A1", "1"), ("A2", "2"), ("A3", "3")])
            .unwrap();
        assert_eq!(
            spreadsheet.cell("A3").unwrap().value,
            CellValue::Number(3.0)
        );
        assert!(spreadsheet.undo());
        assert_eq!(spreadsheet.cell("A1").unwrap().value, CellValue::Blank);
        assert_eq!(spreadsheet.cell("A3").unwrap().value, CellValue::Blank);
    }

    #[test]
    fn csv_round_trip_preserves_formulas_quotes_commas_and_newlines() {
        let csv = "Security,Formula,Notes\r\n\"Acme, Inc.\",=SUM(D2:D3),\"line 1\nline 2\"\r\n\"Quote \"\"A\"\"\",7,";
        let mut spreadsheet = Spreadsheet::new();
        assert_eq!(spreadsheet.import_csv(csv).unwrap(), 8);
        assert_eq!(spreadsheet.cell("A2").unwrap().raw, "Acme, Inc.");
        assert_eq!(spreadsheet.cell("B2").unwrap().raw, "=SUM(D2:D3)");
        assert_eq!(spreadsheet.cell("C2").unwrap().raw, "line 1\nline 2");
        assert_eq!(spreadsheet.cell("A3").unwrap().raw, "Quote \"A\"");

        let exported = spreadsheet.export_csv();
        let mut round_trip = Spreadsheet::new();
        round_trip.import_csv(&exported).unwrap();
        for address in ["A1", "B1", "C1", "A2", "B2", "C2", "A3", "B3", "C3"] {
            assert_eq!(
                round_trip.cell(address).unwrap().raw,
                spreadsheet.cell(address).unwrap().raw,
                "mismatch at {address}"
            );
        }
    }

    #[test]
    fn csv_import_is_atomic_bounded_and_undoable() {
        let mut spreadsheet = Spreadsheet::new();
        spreadsheet.set_cell("A1", "original").unwrap();
        spreadsheet.clear_history();

        assert_eq!(
            spreadsheet.import_csv("\"unterminated"),
            Err(SpreadsheetError::Csv(CsvError::Malformed))
        );
        assert_eq!(spreadsheet.cell("A1").unwrap().raw, "original");
        assert!(!spreadsheet.can_undo());

        let too_wide = vec!["x"; usize::from(MAX_COLUMNS) + 1].join(",");
        assert!(matches!(
            spreadsheet.import_csv(&too_wide),
            Err(SpreadsheetError::Csv(CsvError::TooManyColumns { .. }))
        ));
        assert_eq!(spreadsheet.cell("A1").unwrap().raw, "original");

        spreadsheet.import_csv("new,values").unwrap();
        assert_eq!(spreadsheet.cell("A1").unwrap().raw, "new");
        assert!(spreadsheet.undo());
        assert_eq!(spreadsheet.cell("A1").unwrap().raw, "original");
    }

    #[test]
    fn sheet_lifecycle_operations_are_undoable() {
        let mut spreadsheet = Spreadsheet::new();
        spreadsheet.add_sheet("Model").unwrap();
        spreadsheet.select_sheet("Model").unwrap();
        spreadsheet.rename_active_sheet("DCF").unwrap();
        assert_eq!(spreadsheet.workbook().active_sheet().name(), "DCF");
        assert!(spreadsheet.undo());
        assert_eq!(spreadsheet.workbook().active_sheet().name(), "Model");

        spreadsheet.remove_active_sheet().unwrap();
        assert_eq!(spreadsheet.workbook().sheet_count(), 1);
        assert!(spreadsheet.undo());
        assert_eq!(spreadsheet.workbook().sheet_count(), 2);
        assert_eq!(spreadsheet.workbook().active_sheet().name(), "Model");
    }

    #[test]
    fn facade_recalculates_qualified_references_after_other_sheet_edits() {
        let mut spreadsheet = Spreadsheet::new();
        spreadsheet.rename_active_sheet("Model").unwrap();
        spreadsheet.add_sheet("Base Case").unwrap();
        spreadsheet.select_sheet("Base Case").unwrap();
        spreadsheet.set_cell("A1", "100").unwrap();
        spreadsheet.select_sheet("Model").unwrap();
        spreadsheet.set_cell("A1", "='Base Case'!A1 * 1.2").unwrap();
        assert_eq!(
            spreadsheet.cell("A1").unwrap().value,
            CellValue::Number(120.0)
        );
        spreadsheet.select_sheet("Base Case").unwrap();
        spreadsheet.set_cell("A1", "200").unwrap();
        spreadsheet.select_sheet("Model").unwrap();
        assert_eq!(
            spreadsheet.cell("A1").unwrap().value,
            CellValue::Number(240.0)
        );
    }

    #[test]
    fn copy_and_fill_translate_formulas_atomically() {
        let mut spreadsheet = Spreadsheet::new();
        spreadsheet
            .set_cell("B2", "=A1 + $A1 + A$1 + $A$1")
            .unwrap();
        spreadsheet.copy_cell("B2", "D5").unwrap();
        assert_eq!(
            spreadsheet.cell("D5").unwrap().raw,
            "=C4 + $A4 + C$1 + $A$1"
        );

        spreadsheet.clear_history();
        spreadsheet.fill_cells("B2", ["B3", "B4"]).unwrap();
        assert_eq!(
            spreadsheet.cell("B3").unwrap().raw,
            "=A2 + $A2 + A$1 + $A$1"
        );
        assert_eq!(
            spreadsheet.cell("B4").unwrap().raw,
            "=A3 + $A3 + A$1 + $A$1"
        );
        assert!(spreadsheet.undo());
        assert_eq!(spreadsheet.cell("B3").unwrap().value, CellValue::Blank);
        assert_eq!(spreadsheet.cell("B4").unwrap().value, CellValue::Blank);
    }

    #[test]
    fn failed_copy_does_not_mutate_the_destination() {
        let mut spreadsheet = Spreadsheet::new();
        spreadsheet.set_cell("B1", "=A1").unwrap();
        spreadsheet.set_cell("A1", "saved").unwrap();
        spreadsheet.clear_history();
        assert!(matches!(
            spreadsheet.copy_cell("B1", "A1"),
            Err(SpreadsheetError::Formula(_))
        ));
        assert_eq!(spreadsheet.cell("A1").unwrap().raw, "saved");
        assert!(!spreadsheet.can_undo());
    }
}
