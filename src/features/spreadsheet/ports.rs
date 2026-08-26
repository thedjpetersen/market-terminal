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
        Self {
            security: security.into(),
            field: field.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MarketDataPoint {
    pub request: MarketDataRequest,
    pub state: MarketDataState,
}

impl MarketDataPoint {
    pub fn ready(request: MarketDataRequest, value: f64, provenance: MarketDataProvenance) -> Self {
        Self {
            request,
            state: MarketDataState::Ready { value, provenance },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarketDataQuality {
    Realtime,
    Delayed,
    Demo,
}

impl MarketDataQuality {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Realtime => "REALTIME",
            Self::Delayed => "DELAYED",
            Self::Demo => "DEMO",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarketDataProvenance {
    pub provider: String,
    pub observed_at: String,
    pub received_at: String,
    pub quality: MarketDataQuality,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MarketDataState {
    Ready {
        value: f64,
        provenance: MarketDataProvenance,
    },
    Stale {
        value: f64,
        provenance: MarketDataProvenance,
    },
    PermissionDenied {
        provider: String,
    },
    Unavailable {
        reason: String,
    },
}

/// Spreadsheet-owned batch port for external market data.
///
/// A real Bloomberg, Refinitiv, or internal feed adapter can implement this
/// trait without changing worksheet, formula, or presentation code.
pub trait SpreadsheetMarketData: Send + Sync {
    fn load_batch(&self, requests: &[MarketDataRequest]) -> Vec<MarketDataPoint>;
}
