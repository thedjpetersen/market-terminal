/// A market-data field requested by a worksheet.
///
/// Fields remain strings at this boundary so infrastructure adapters can map
/// vendor-specific fields without leaking those concepts into the spreadsheet
/// domain.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MarketDataRequest {
    pub security: String,
    pub field: String,
}

impl MarketDataRequest {
    pub fn new(security: impl Into<String>, field: impl Into<String>) -> Self {
        Self { security: security.into(), field: field.into() }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MarketDataPoint {
    pub request: MarketDataRequest,
    pub value: f64,
}

/// Spreadsheet-owned batch port for external market data.
///
/// A real Bloomberg, Refinitiv, or internal feed adapter can implement this
/// trait without changing worksheet, formula, or presentation code.
pub trait SpreadsheetMarketData: Send + Sync {
    fn load_batch(&self, requests: &[MarketDataRequest]) -> Vec<MarketDataPoint>;
}
