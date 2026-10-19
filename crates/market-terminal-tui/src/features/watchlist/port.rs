use super::WatchlistDefinition;

/// Persistence/read-model boundary owned by Watchlists & Monitors.
pub trait WatchlistCatalog: Send + Sync {
    /// `None` resolves the user's default watchlist.
    fn load_watchlist(&self, name: Option<&str>) -> Option<WatchlistDefinition>;
}

use super::{CanonicalInstrumentId, MarketDataError, QuoteSnapshot, QuoteSubscriptionRequest};

/// Watchlist-owned quote boundary, translated by infrastructure.
pub trait MarketDataQuery: Send + Sync {
    fn quote_snapshots(
        &self,
        instruments: &[CanonicalInstrumentId],
    ) -> Result<Vec<QuoteSnapshot>, MarketDataError>;

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
