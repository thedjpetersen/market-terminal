pub mod application;
pub mod domain;
mod ports;
mod presentation;

pub use application::{CellView, CsvError, Spreadsheet, SpreadsheetError};
pub use ports::{
    MarketDataPoint, MarketDataProvenance, MarketDataQuality, MarketDataRequest, MarketDataState,
    SpreadsheetFileError, SpreadsheetFileStore, SpreadsheetMarketData, SpreadsheetWorkbookStore,
    StoredWorkbook,
};
pub use presentation::SpreadsheetWorkspace;

use crate::app::WorkspaceId;

pub const ID: WorkspaceId = WorkspaceId::new("spreadsheet");
