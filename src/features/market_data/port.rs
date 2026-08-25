use super::{
    CanonicalInstrumentId, HistoryRequest, MarketDataError, PriceBar, QuoteSnapshot,
    QuoteSubscriptionRequest,
};

/// Query boundary owned by Market Data and implemented by feed/replay adapters.
pub trait MarketDataQuery: Send + Sync {
    fn quote_snapshots(
        &self,
        instruments: &[CanonicalInstrumentId],
    ) -> Result<Vec<QuoteSnapshot>, MarketDataError>;

    fn price_history(&self, request: &HistoryRequest) -> Result<Vec<PriceBar>, MarketDataError>;

    /// Starts a provider-independent latest-value stream. Query-only adapters
    /// remain source compatible and report this capability explicitly.
    fn subscribe_quotes(
        &self,
        _request: QuoteSubscriptionRequest,
    ) -> Result<Box<dyn QuoteSubscription>, MarketDataError> {
        Err(MarketDataError::Unsupported("streaming quotes".to_owned()))
    }
}

pub trait QuoteSubscription: Send {
    fn id(&self) -> super::SubscriptionId;
    fn drain(&mut self) -> Result<Vec<super::QuoteUpdate>, MarketDataError>;
    fn cancel(&mut self);
    fn is_cancelled(&self) -> bool;
    fn metrics(&self) -> super::SubscriptionMetrics;
}
