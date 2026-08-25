use super::{
    CanonicalInstrumentId, HistoryRequest, MarketDataError, PriceBar, QuoteSnapshot,
};

/// Query boundary owned by Market Data and implemented by feed/replay adapters.
pub trait MarketDataQuery: Send + Sync {
    fn quote_snapshots(
        &self,
        instruments: &[CanonicalInstrumentId],
    ) -> Result<Vec<QuoteSnapshot>, MarketDataError>;

    fn price_history(&self, request: &HistoryRequest) -> Result<Vec<PriceBar>, MarketDataError>;
}
