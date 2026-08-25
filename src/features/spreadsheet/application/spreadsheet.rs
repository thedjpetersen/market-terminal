use std::fmt;

use crate::features::spreadsheet::domain::{
    AddressError, CellAddress, CellValue, Workbook, WorkbookError,
};

#[derive(Debug, Clone, Default)]
pub struct Spreadsheet {
    workbook: Workbook,
}

impl Spreadsheet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_workbook(workbook: Workbook) -> Self {
        Self { workbook }
    }

    pub fn workbook(&self) -> &Workbook {
        &self.workbook
    }

    pub fn into_workbook(self) -> Workbook {
        self.workbook
    }

    pub fn add_sheet(&mut self, name: impl Into<String>) -> Result<usize, SpreadsheetError> {
        self.workbook.add_sheet(name).map_err(SpreadsheetError::Workbook)
    }

    pub fn select_sheet(&mut self, name: &str) -> Result<(), SpreadsheetError> {
        self.workbook.select_sheet(name).map_err(SpreadsheetError::Workbook)
    }

    pub fn set_cell(&mut self, address: &str, raw: impl Into<String>) -> Result<CellValue, SpreadsheetError> {
        let address = parse_address(address)?;
        self.workbook.active_sheet_mut().set(address, raw);
        Ok(self.workbook.active_sheet().value(address))
    }

    pub fn clear_cell(&mut self, address: &str) -> Result<(), SpreadsheetError> {
        let address = parse_address(address)?;
        self.workbook.active_sheet_mut().clear(address);
        Ok(())
    }

    pub fn cell(&self, address: &str) -> Result<CellView, SpreadsheetError> {
        let address = parse_address(address)?;
        let sheet = self.workbook.active_sheet();
        Ok(CellView {
            address,
            raw: sheet.cell(address).map(|cell| cell.raw().to_owned()).unwrap_or_default(),
            value: sheet.value(address),
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
        let end_column = start_column.checked_add(columns.saturating_sub(1))
            .ok_or(SpreadsheetError::RegionOutOfBounds)?;
        let end_row = start_row.checked_add(rows.saturating_sub(1))
            .ok_or(SpreadsheetError::RegionOutOfBounds)?;
        let sheet = self.workbook.active_sheet();
        let values = sheet.values();
        let mut cells = Vec::with_capacity(columns as usize * rows as usize);
        for row in start_row..=end_row {
            for column in start_column..=end_column {
                let address = CellAddress::new(column, row).map_err(SpreadsheetError::Address)?;
                cells.push(CellView {
                    address,
                    raw: sheet.cell(address).map(|cell| cell.raw().to_owned()).unwrap_or_default(),
                    value: values.get(&address).cloned().unwrap_or(CellValue::Blank),
                });
            }
        }
        Ok(cells)
    }
}

fn parse_address(input: &str) -> Result<CellAddress, SpreadsheetError> {
    input.parse().map_err(SpreadsheetError::Address)
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
    RegionOutOfBounds,
}

impl fmt::Display for SpreadsheetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Address(error) => error.fmt(formatter),
            Self::Workbook(error) => error.fmt(formatter),
            Self::RegionOutOfBounds => write!(formatter, "visible region is outside A1:Z100"),
        }
    }
}

impl std::error::Error for SpreadsheetError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn application_facade_accepts_user_addresses_and_recalculates() {
        let mut spreadsheet = Spreadsheet::new();
        spreadsheet.set_cell("A1", "100").unwrap();
        spreadsheet.set_cell("B1", "=A1 * 1.2").unwrap();
        assert_eq!(spreadsheet.cell("B1").unwrap().value, CellValue::Number(120.0));
        spreadsheet.set_cell("A1", "200").unwrap();
        assert_eq!(spreadsheet.cell("B1").unwrap().value, CellValue::Number(240.0));
    }

    #[test]
    fn visible_region_is_row_major_and_includes_blank_cells() {
        let mut spreadsheet = Spreadsheet::new();
        spreadsheet.set_cell("B2", "7").unwrap();
        let region = spreadsheet.visible_region(1, 1, 2, 2).unwrap();
        assert_eq!(region.iter().map(|cell| cell.address.to_string()).collect::<Vec<_>>(), ["A1", "B1", "A2", "B2"]);
        assert_eq!(region[3].value, CellValue::Number(7.0));
    }

    #[test]
    fn validates_region_boundaries() {
        let spreadsheet = Spreadsheet::new();
        assert_eq!(spreadsheet.visible_region(26, 100, 2, 1), Err(SpreadsheetError::Address(AddressError::ColumnOutOfBounds(27))));
    }
}
