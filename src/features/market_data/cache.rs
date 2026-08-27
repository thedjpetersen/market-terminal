use std::collections::HashMap;

use super::{CacheStatus, CanonicalInstrumentId, DataQuality, MarketDataError, QuoteSnapshot};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuoteCachePolicy {
    pub fresh_for_ms: u64,
    pub last_known_good_for_ms: u64,
}

impl QuoteCachePolicy {
    pub fn new(fresh_for_ms: u64, last_known_good_for_ms: u64) -> Result<Self, MarketDataError> {
        if last_known_good_for_ms < fresh_for_ms {
            return Err(MarketDataError::InvalidRequest(
                "last-known-good window must include the freshness window".to_owned(),
            ));
        }
        Ok(Self {
            fresh_for_ms,
            last_known_good_for_ms,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum QuoteCacheLookup {
    Fresh(QuoteSnapshot),
    LastKnownGood(QuoteSnapshot),
    Miss,
}

#[derive(Debug, Clone)]
struct CacheEntry {
    snapshot: QuoteSnapshot,
    stored_at_ms: u64,
}

/// Clock-free cache core. Adapters provide monotonic milliseconds, which makes
/// expiry and failure fallback completely deterministic in tests.
#[derive(Debug)]
pub struct QuoteCache {
    policy: QuoteCachePolicy,
    entries: HashMap<CanonicalInstrumentId, CacheEntry>,
}

impl QuoteCache {
    pub fn new(policy: QuoteCachePolicy) -> Self {
        Self {
            policy,
            entries: HashMap::new(),
        }
    }

    /// Unusable entitlement/error placeholders never replace a usable observation.
    pub fn record(&mut self, snapshot: QuoteSnapshot, stored_at_ms: u64) -> bool {
        if !snapshot.quality.is_usable() {
            return false;
        }
        self.entries.insert(
            snapshot.instrument_id.clone(),
            CacheEntry {
                snapshot,
                stored_at_ms,
            },
        );
        true
    }

    pub fn lookup(
        &mut self,
        instrument_id: &CanonicalInstrumentId,
        now_ms: u64,
    ) -> QuoteCacheLookup {
        let Some(entry) = self.entries.get(instrument_id) else {
            return QuoteCacheLookup::Miss;
        };
        let age_ms = now_ms.saturating_sub(entry.stored_at_ms);
        if age_ms <= self.policy.fresh_for_ms {
            let mut snapshot = entry.snapshot.clone();
            snapshot.provenance.cache_status = CacheStatus::Fresh;
            return QuoteCacheLookup::Fresh(snapshot);
        }
        if age_ms <= self.policy.last_known_good_for_ms {
            let mut snapshot = entry.snapshot.clone();
            snapshot.quality = DataQuality::Stale {
                age_seconds: age_ms / 1_000,
            };
            snapshot.provenance.cache_status = CacheStatus::LastKnownGood;
            return QuoteCacheLookup::LastKnownGood(snapshot);
        }
        self.entries.remove(instrument_id);
        QuoteCacheLookup::Miss
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::market_data::{
        DataProvenance, Percent, Price, PriceChange, ProviderId, UtcTimestamp,
    };

    fn quote(quality: DataQuality) -> QuoteSnapshot {
        let timestamp = UtcTimestamp::new("2026-08-25T20:00:00Z");
        QuoteSnapshot {
            instrument_id: CanonicalInstrumentId::new("us:xnas:aapl"),
            symbol: "AAPL".to_owned(),
            currency: "USD".to_owned(),
            last: Some(Price::new(205.30)),
            change: Some(PriceChange {
                absolute: Price::new(1.0),
                percent: Percent::new(0.5),
            }),
            bid: None,
            ask: None,
            day_low: None,
            day_high: None,
            volume: None,
            as_of: timestamp.clone(),
            quality,
            provenance: DataProvenance::live(
                ProviderId::new("test"),
                timestamp.clone(),
                timestamp,
                Some(1),
            ),
        }
    }

    #[test]
    fn distinguishes_fresh_last_known_good_and_expired() {
        let policy = QuoteCachePolicy::new(1_000, 10_000).expect("policy");
        let mut cache = QuoteCache::new(policy);
        cache.record(quote(DataQuality::RealTime), 1_000);

        assert!(matches!(
            cache.lookup(&CanonicalInstrumentId::new("us:xnas:aapl"), 1_500),
            QuoteCacheLookup::Fresh(_)
        ));
        let QuoteCacheLookup::LastKnownGood(stale) =
            cache.lookup(&CanonicalInstrumentId::new("us:xnas:aapl"), 5_100)
        else {
            panic!("expected last-known-good quote");
        };
        assert_eq!(stale.quality, DataQuality::Stale { age_seconds: 4 });
        assert_eq!(stale.provenance.cache_status, CacheStatus::LastKnownGood);
        assert!(matches!(
            cache.lookup(&CanonicalInstrumentId::new("us:xnas:aapl"), 20_000),
            QuoteCacheLookup::Miss
        ));
        assert!(cache.is_empty());
    }

    #[test]
    fn unavailable_placeholder_cannot_poison_last_known_good() {
        let mut cache = QuoteCache::new(QuoteCachePolicy::new(0, 10_000).expect("policy"));
        assert!(cache.record(quote(DataQuality::RealTime), 1));
        assert!(!cache.record(quote(DataQuality::PermissionDenied), 2));
        let QuoteCacheLookup::LastKnownGood(snapshot) =
            cache.lookup(&CanonicalInstrumentId::new("us:xnas:aapl"), 3)
        else {
            panic!("expected retained quote");
        };
        assert_eq!(snapshot.last, Some(Price::new(205.30)));
    }
}
