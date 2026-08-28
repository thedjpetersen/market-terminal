use std::{
    fmt,
    path::{Path, PathBuf},
};

use super::{
    PortfolioActivityLedger, PortfolioContributionSnapshot, PortfolioPerformanceSnapshot,
    PortfolioRealizedGainSnapshot, PortfolioSnapshot, PortfolioTaxLotSnapshot,
    PortfolioTradeLedger,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortfolioError {
    Unsupported(String),
    Io(String),
    InvalidCsv(String),
}

impl fmt::Display for PortfolioError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(message) | Self::Io(message) | Self::InvalidCsv(message) => {
                formatter.write_str(message)
            }
        }
    }
}

impl std::error::Error for PortfolioError {}

pub trait PortfolioRepository: Send + Sync {
    fn load_portfolio(&self) -> PortfolioSnapshot;

    fn load_activity(&self) -> PortfolioActivityLedger {
        PortfolioActivityLedger::empty("NO ACTIVITY IMPORTED · USE PORT IMPORT ACTIVITY <FILE.CSV>")
    }

    fn import_csv(&self, _path: &Path) -> Result<PortfolioSnapshot, PortfolioError> {
        Err(PortfolioError::Unsupported(
            "THIS PORTFOLIO PROVIDER DOES NOT SUPPORT CSV IMPORT".to_owned(),
        ))
    }

    fn reload(&self) -> Result<PortfolioSnapshot, PortfolioError> {
        Err(PortfolioError::Unsupported(
            "NO IMPORTED PORTFOLIO TO RELOAD".to_owned(),
        ))
    }

    fn import_activity_csv(&self, _path: &Path) -> Result<PortfolioActivityLedger, PortfolioError> {
        Err(PortfolioError::Unsupported(
            "THIS PORTFOLIO PROVIDER DOES NOT SUPPORT ACTIVITY CSV IMPORT".to_owned(),
        ))
    }

    fn reload_activity(&self) -> Result<PortfolioActivityLedger, PortfolioError> {
        Err(PortfolioError::Unsupported(
            "NO IMPORTED ACTIVITY TO RELOAD".to_owned(),
        ))
    }

    fn load_performance(&self) -> PortfolioPerformanceSnapshot {
        PortfolioPerformanceSnapshot::empty(
            "NO PERFORMANCE IMPORTED · USE PORT IMPORT PERFORMANCE <FILE.CSV>",
        )
    }

    fn import_performance_csv(
        &self,
        _path: &Path,
    ) -> Result<PortfolioPerformanceSnapshot, PortfolioError> {
        Err(PortfolioError::Unsupported(
            "THIS PORTFOLIO PROVIDER DOES NOT SUPPORT PERFORMANCE CSV IMPORT".to_owned(),
        ))
    }

    fn reload_performance(&self) -> Result<PortfolioPerformanceSnapshot, PortfolioError> {
        Err(PortfolioError::Unsupported(
            "NO IMPORTED PERFORMANCE · USE PORT IMPORT PERFORMANCE <FILE.CSV>".to_owned(),
        ))
    }

    fn load_tax_lots(&self) -> PortfolioTaxLotSnapshot {
        PortfolioTaxLotSnapshot::empty("NO TAX LOTS IMPORTED · USE PORT IMPORT LOTS <FILE.CSV>")
    }

    fn import_tax_lots_csv(&self, _path: &Path) -> Result<PortfolioTaxLotSnapshot, PortfolioError> {
        Err(PortfolioError::Unsupported(
            "THIS PORTFOLIO PROVIDER DOES NOT SUPPORT TAX-LOT CSV IMPORT".to_owned(),
        ))
    }

    fn reload_tax_lots(&self) -> Result<PortfolioTaxLotSnapshot, PortfolioError> {
        Err(PortfolioError::Unsupported(
            "NO IMPORTED TAX LOTS · USE PORT IMPORT LOTS <FILE.CSV>".to_owned(),
        ))
    }

    fn load_realized_gains(&self) -> PortfolioRealizedGainSnapshot {
        PortfolioRealizedGainSnapshot::empty(
            "NO REALIZED GAINS IMPORTED · USE PORT IMPORT REALIZED <FILE.CSV>",
        )
    }

    fn import_realized_gains_csv(
        &self,
        _path: &Path,
    ) -> Result<PortfolioRealizedGainSnapshot, PortfolioError> {
        Err(PortfolioError::Unsupported(
            "THIS PORTFOLIO PROVIDER DOES NOT SUPPORT CLOSED-LOT CSV IMPORT".to_owned(),
        ))
    }

    fn reload_realized_gains(&self) -> Result<PortfolioRealizedGainSnapshot, PortfolioError> {
        Err(PortfolioError::Unsupported(
            "NO IMPORTED REALIZED GAINS · USE PORT IMPORT REALIZED <FILE.CSV>".to_owned(),
        ))
    }

    fn load_trades(&self) -> PortfolioTradeLedger {
        PortfolioTradeLedger::empty("NO TRADES IMPORTED · USE PORT IMPORT TRADES <FILE.CSV>")
    }

    fn import_trades_csv(&self, _path: &Path) -> Result<PortfolioTradeLedger, PortfolioError> {
        Err(PortfolioError::Unsupported(
            "THIS PORTFOLIO PROVIDER DOES NOT SUPPORT EXECUTION CSV IMPORT".to_owned(),
        ))
    }

    fn reload_trades(&self) -> Result<PortfolioTradeLedger, PortfolioError> {
        Err(PortfolioError::Unsupported(
            "NO IMPORTED TRADES · USE PORT IMPORT TRADES <FILE.CSV>".to_owned(),
        ))
    }

    fn load_contribution(&self) -> PortfolioContributionSnapshot {
        PortfolioContributionSnapshot::empty(
            "NO CONTRIBUTION IMPORTED · USE PORT IMPORT CONTRIBUTION <FILE.CSV>",
        )
    }

    fn import_contribution_csv(
        &self,
        _path: &Path,
    ) -> Result<PortfolioContributionSnapshot, PortfolioError> {
        Err(PortfolioError::Unsupported(
            "THIS PORTFOLIO PROVIDER DOES NOT SUPPORT CONTRIBUTION CSV IMPORT".to_owned(),
        ))
    }

    fn reload_contribution(&self) -> Result<PortfolioContributionSnapshot, PortfolioError> {
        Err(PortfolioError::Unsupported(
            "NO IMPORTED CONTRIBUTION · USE PORT IMPORT CONTRIBUTION <FILE.CSV>".to_owned(),
        ))
    }
}

/// Persists only the user-selected import location, never portfolio contents.
pub trait PortfolioImportStateStore: Send + Sync {
    fn load_import_path(&self) -> Result<Option<PathBuf>, PortfolioError>;
    fn save_import_path(&self, path: &Path) -> Result<(), PortfolioError>;

    fn load_activity_import_path(&self) -> Result<Option<PathBuf>, PortfolioError> {
        Ok(None)
    }

    fn save_activity_import_path(&self, _path: &Path) -> Result<(), PortfolioError> {
        Err(PortfolioError::Unsupported(
            "THIS STATE STORE DOES NOT SUPPORT ACTIVITY IMPORT PATHS".to_owned(),
        ))
    }

    fn load_performance_import_path(&self) -> Result<Option<PathBuf>, PortfolioError> {
        Ok(None)
    }

    fn save_performance_import_path(&self, _path: &Path) -> Result<(), PortfolioError> {
        Err(PortfolioError::Unsupported(
            "THIS STATE STORE DOES NOT SUPPORT PERFORMANCE IMPORT PATHS".to_owned(),
        ))
    }

    fn load_tax_lot_import_path(&self) -> Result<Option<PathBuf>, PortfolioError> {
        Ok(None)
    }

    fn save_tax_lot_import_path(&self, _path: &Path) -> Result<(), PortfolioError> {
        Err(PortfolioError::Unsupported(
            "THIS STATE STORE DOES NOT SUPPORT TAX-LOT IMPORT PATHS".to_owned(),
        ))
    }

    fn load_realized_gain_import_path(&self) -> Result<Option<PathBuf>, PortfolioError> {
        Ok(None)
    }

    fn save_realized_gain_import_path(&self, _path: &Path) -> Result<(), PortfolioError> {
        Err(PortfolioError::Unsupported(
            "THIS STATE STORE DOES NOT SUPPORT CLOSED-LOT IMPORT PATHS".to_owned(),
        ))
    }

    fn load_trade_import_path(&self) -> Result<Option<PathBuf>, PortfolioError> {
        Ok(None)
    }

    fn save_trade_import_path(&self, _path: &Path) -> Result<(), PortfolioError> {
        Err(PortfolioError::Unsupported(
            "THIS STATE STORE DOES NOT SUPPORT EXECUTION IMPORT PATHS".to_owned(),
        ))
    }

    fn load_contribution_import_path(&self) -> Result<Option<PathBuf>, PortfolioError> {
        Ok(None)
    }

    fn save_contribution_import_path(&self, _path: &Path) -> Result<(), PortfolioError> {
        Err(PortfolioError::Unsupported(
            "THIS STATE STORE DOES NOT SUPPORT CONTRIBUTION IMPORT PATHS".to_owned(),
        ))
    }
}
