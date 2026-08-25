use std::fmt;

use super::{Worksheet, WorksheetError};

#[derive(Debug, Clone, PartialEq)]
pub struct Workbook {
    sheets: Vec<Worksheet>,
    active_sheet: usize,
}

impl Workbook {
    pub fn new() -> Self {
        Self { sheets: vec![Worksheet::new("Sheet1").expect("default sheet name is valid")], active_sheet: 0 }
    }

    pub fn sheets(&self) -> &[Worksheet] {
        &self.sheets
    }

    pub fn active_sheet(&self) -> &Worksheet {
        &self.sheets[self.active_sheet]
    }

    pub const fn active_sheet_index(&self) -> usize {
        self.active_sheet
    }

    pub fn sheet_count(&self) -> usize {
        self.sheets.len()
    }

    pub fn active_sheet_mut(&mut self) -> &mut Worksheet {
        &mut self.sheets[self.active_sheet]
    }

    pub fn add_sheet(&mut self, name: impl Into<String>) -> Result<usize, WorkbookError> {
        let name = name.into();
        if self.sheets.iter().any(|sheet| sheet.name().eq_ignore_ascii_case(name.trim())) {
            return Err(WorkbookError::DuplicateSheetName(name));
        }
        let sheet = Worksheet::new(name).map_err(WorkbookError::InvalidSheet)?;
        self.sheets.push(sheet);
        Ok(self.sheets.len() - 1)
    }

    pub fn select_sheet(&mut self, name: &str) -> Result<(), WorkbookError> {
        self.active_sheet = self.sheets.iter().position(|sheet| sheet.name().eq_ignore_ascii_case(name))
            .ok_or_else(|| WorkbookError::SheetNotFound(name.to_owned()))?;
        Ok(())
    }

    pub fn select_sheet_at(&mut self, index: usize) -> Result<(), WorkbookError> {
        if index >= self.sheets.len() {
            return Err(WorkbookError::SheetIndexOutOfBounds(index));
        }
        self.active_sheet = index;
        Ok(())
    }

    pub fn select_next_sheet(&mut self) {
        self.active_sheet = (self.active_sheet + 1) % self.sheets.len();
    }

    pub fn select_previous_sheet(&mut self) {
        self.active_sheet = (self.active_sheet + self.sheets.len() - 1) % self.sheets.len();
    }

    pub fn rename_active_sheet(&mut self, name: impl Into<String>) -> Result<(), WorkbookError> {
        let name = name.into();
        let normalized = name.trim();
        if self
            .sheets
            .iter()
            .enumerate()
            .any(|(index, sheet)| index != self.active_sheet && sheet.name().eq_ignore_ascii_case(normalized))
        {
            return Err(WorkbookError::DuplicateSheetName(name));
        }
        self.sheets[self.active_sheet]
            .rename(name)
            .map_err(WorkbookError::InvalidSheet)
    }

    pub fn remove_active_sheet(&mut self) -> Result<Worksheet, WorkbookError> {
        if self.sheets.len() == 1 {
            return Err(WorkbookError::CannotRemoveLastSheet);
        }
        let removed = self.sheets.remove(self.active_sheet);
        self.active_sheet = self.active_sheet.min(self.sheets.len() - 1);
        Ok(removed)
    }

    pub fn sheet(&self, name: &str) -> Option<&Worksheet> {
        self.sheets.iter().find(|sheet| sheet.name().eq_ignore_ascii_case(name))
    }

    pub fn sheet_mut(&mut self, name: &str) -> Option<&mut Worksheet> {
        self.sheets.iter_mut().find(|sheet| sheet.name().eq_ignore_ascii_case(name))
    }
}

impl Default for Workbook {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkbookError {
    InvalidSheet(WorksheetError),
    DuplicateSheetName(String),
    SheetNotFound(String),
    SheetIndexOutOfBounds(usize),
    CannotRemoveLastSheet,
}

impl fmt::Display for WorkbookError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSheet(error) => error.fmt(formatter),
            Self::DuplicateSheetName(name) => write!(formatter, "worksheet '{name}' already exists"),
            Self::SheetNotFound(name) => write!(formatter, "worksheet '{name}' was not found"),
            Self::SheetIndexOutOfBounds(index) => write!(formatter, "worksheet index {index} is out of bounds"),
            Self::CannotRemoveLastSheet => write!(formatter, "a workbook must contain at least one worksheet"),
        }
    }
}

impl std::error::Error for WorkbookError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manages_multiple_named_sheets() {
        let mut workbook = Workbook::new();
        workbook.add_sheet("Valuation").unwrap();
        workbook.select_sheet("valuation").unwrap();
        assert_eq!(workbook.active_sheet().name(), "Valuation");
        assert!(matches!(workbook.add_sheet("VALUATION"), Err(WorkbookError::DuplicateSheetName(_))));
    }

    #[test]
    fn cycles_renames_and_removes_sheets_without_losing_a_valid_selection() {
        let mut workbook = Workbook::new();
        workbook.add_sheet("Valuation").unwrap();
        workbook.add_sheet("Scenarios").unwrap();

        workbook.select_previous_sheet();
        assert_eq!(workbook.active_sheet().name(), "Scenarios");
        workbook.select_next_sheet();
        assert_eq!(workbook.active_sheet().name(), "Sheet1");
        workbook.rename_active_sheet("Inputs").unwrap();
        assert_eq!(workbook.active_sheet().name(), "Inputs");

        workbook.select_sheet("Scenarios").unwrap();
        assert_eq!(workbook.remove_active_sheet().unwrap().name(), "Scenarios");
        assert_eq!(workbook.active_sheet().name(), "Valuation");
        assert_eq!(workbook.sheet_count(), 2);
    }

    #[test]
    fn protects_sheet_names_and_the_last_remaining_sheet() {
        let mut workbook = Workbook::new();
        assert_eq!(workbook.remove_active_sheet(), Err(WorkbookError::CannotRemoveLastSheet));
        workbook.add_sheet("Model").unwrap();
        assert!(matches!(
            workbook.rename_active_sheet("sheet1"),
            Err(WorkbookError::DuplicateSheetName(_))
        ));
        assert_eq!(
            workbook.select_sheet_at(2),
            Err(WorkbookError::SheetIndexOutOfBounds(2))
        );
    }
}
