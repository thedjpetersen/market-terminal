use crate::features::{market_data as source, watchlist as target};
use std::sync::Arc;

pub struct WatchlistQuotes(Arc<dyn source::MarketDataQuery>);
impl WatchlistQuotes {
    pub fn new(source: Arc<dyn source::MarketDataQuery>) -> Self {
        Self(source)
    }
}
fn error(value: source::MarketDataError) -> target::MarketDataError {
    match value {
        source::MarketDataError::Cancelled => target::MarketDataError::Cancelled,
        source::MarketDataError::Unsupported(value) => target::MarketDataError::Unsupported(value),
        other => target::MarketDataError::Unavailable(other.to_string()),
    }
}
fn quote(value: source::QuoteSnapshot) -> target::QuoteSnapshot {
    target::QuoteSnapshot {
        instrument_id: value.instrument_id,
        symbol: value.symbol,
        currency: value.currency,
        last: value.last.map(|v| target::Price::new(v.value())),
        bid: value.bid.map(|v| target::Price::new(v.value())),
        ask: value.ask.map(|v| target::Price::new(v.value())),
        day_low: value.day_low.map(|v| target::Price::new(v.value())),
        day_high: value.day_high.map(|v| target::Price::new(v.value())),
        volume: value.volume.map(|v| target::Quantity::new(v.value())),
        change: value.change.map(|v| target::PriceChange {
            absolute: target::Price::new(v.absolute.value()),
            percent: target::Percent::new(v.percent.value()),
        }),
        as_of: target::UtcTimestamp::new(value.as_of.as_str()),
        quality: match value.quality {
            source::DataQuality::RealTime => target::DataQuality::RealTime,
            source::DataQuality::Delayed { minutes } => target::DataQuality::Delayed { minutes },
            source::DataQuality::Stale { age_seconds } => {
                target::DataQuality::Stale { age_seconds }
            }
            source::DataQuality::Derived => target::DataQuality::Derived,
            source::DataQuality::Unavailable => target::DataQuality::Unavailable,
            source::DataQuality::PermissionDenied => target::DataQuality::PermissionDenied,
        },
        provenance: target::DataProvenance {
            provider: target::ProviderId::new(value.provenance.provider.as_str()),
            source_timestamp: target::UtcTimestamp::new(value.provenance.source_timestamp.as_str()),
            received_at: target::UtcTimestamp::new(value.provenance.received_at.as_str()),
            sequence: value.provenance.sequence,
            cache_status: match value.provenance.cache_status {
                source::CacheStatus::Live => target::CacheStatus::Live,
                source::CacheStatus::Fresh => target::CacheStatus::Fresh,
                source::CacheStatus::LastKnownGood => target::CacheStatus::LastKnownGood,
            },
        },
    }
}
impl target::MarketDataQuery for WatchlistQuotes {
    fn quote_snapshots(
        &self,
        instruments: &[target::CanonicalInstrumentId],
    ) -> Result<Vec<target::QuoteSnapshot>, target::MarketDataError> {
        self.0
            .quote_snapshots(instruments)
            .map(|values| values.into_iter().map(quote).collect())
            .map_err(error)
    }
    fn subscribe_quotes(
        &self,
        request: target::QuoteSubscriptionRequest,
    ) -> Result<Box<dyn target::QuoteSubscription>, target::MarketDataError> {
        let request = source::QuoteSubscriptionRequest::new(
            request.instruments().to_vec(),
            request.capacity(),
        )
        .map_err(error)?;
        self.0
            .subscribe_quotes(request)
            .map(|stream| Box::new(Stream(stream)) as Box<dyn target::QuoteSubscription>)
            .map_err(error)
    }
}
struct Stream(Box<dyn source::QuoteSubscription>);
impl target::QuoteSubscription for Stream {
    fn id(&self) -> target::SubscriptionId {
        target::SubscriptionId::new(self.0.id().value())
    }
    fn drain(&mut self) -> Result<Vec<target::QuoteUpdate>, target::MarketDataError> {
        self.0
            .drain()
            .map(|updates| {
                updates
                    .into_iter()
                    .map(|v| target::QuoteUpdate {
                        snapshot: quote(v.snapshot),
                    })
                    .collect()
            })
            .map_err(error)
    }
    fn cancel(&mut self) {
        self.0.cancel();
    }
    fn is_cancelled(&self) -> bool {
        self.0.is_cancelled()
    }
    fn metrics(&self) -> target::SubscriptionMetrics {
        let v = self.0.metrics();
        target::SubscriptionMetrics {
            received: v.received,
            coalesced: v.coalesced,
            dropped: v.dropped,
            pending: v.pending,
        }
    }
}
