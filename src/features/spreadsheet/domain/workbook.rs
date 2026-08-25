use std::fmt;

use super::{Worksheet, WorksheetError};

#[derive(Debug, Clone)]
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
}

impl fmt::Display for WorkbookError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSheet(error) => error.fmt(formatter),
            Self::DuplicateSheetName(name) => write!(formatter, "worksheet '{name}' already exists"),
            Self::SheetNotFound(name) => write!(formatter, "worksheet '{name}' was not found"),
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
}
