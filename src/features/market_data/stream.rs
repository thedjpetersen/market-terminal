use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use super::{CanonicalInstrumentId, MarketDataError, QuoteSnapshot};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuoteSubscriptionRequest {
    instruments: Vec<CanonicalInstrumentId>,
    capacity: usize,
}

impl QuoteSubscriptionRequest {
    pub fn new(
        instruments: Vec<CanonicalInstrumentId>,
        capacity: usize,
    ) -> Result<Self, MarketDataError> {
        if instruments.is_empty() {
            return Err(MarketDataError::InvalidRequest(
                "quote subscription requires at least one instrument".to_owned(),
            ));
        }
        if capacity == 0 {
            return Err(MarketDataError::InvalidRequest(
                "quote subscription capacity must be non-zero".to_owned(),
            ));
        }

        let mut seen = HashSet::new();
        let instruments = instruments
            .into_iter()
            .filter(|instrument| seen.insert(instrument.clone()))
            .collect();
        Ok(Self {
            instruments,
            capacity,
        })
    }

    pub fn instruments(&self) -> &[CanonicalInstrumentId] {
        &self.instruments
    }
    pub const fn capacity(&self) -> usize {
        self.capacity
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SubscriptionId(u64);

impl SubscriptionId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }
    pub const fn value(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct QuoteUpdate {
    pub snapshot: QuoteSnapshot,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SubscriptionMetrics {
    pub received: u64,
    pub coalesced: u64,
    pub dropped: u64,
    pub pending: usize,
}

/// Cooperative cancellation which can be shared with a provider worker.
#[derive(Debug, Clone, Default)]
pub struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// Bounded latest-value queue. A fast producer can never grow memory without bound.
/// Updates for an already pending instrument replace its older value in place.
#[derive(Debug)]
pub struct CoalescingQuoteBuffer {
    capacity: usize,
    order: VecDeque<CanonicalInstrumentId>,
    pending: HashMap<CanonicalInstrumentId, QuoteUpdate>,
    received: u64,
    coalesced: u64,
    dropped: u64,
}

impl CoalescingQuoteBuffer {
    pub fn new(capacity: usize) -> Result<Self, MarketDataError> {
        if capacity == 0 {
            return Err(MarketDataError::InvalidRequest(
                "quote buffer capacity must be non-zero".to_owned(),
            ));
        }
        Ok(Self {
            capacity,
            order: VecDeque::with_capacity(capacity),
            pending: HashMap::with_capacity(capacity),
            received: 0,
            coalesced: 0,
            dropped: 0,
        })
    }

    pub fn push(&mut self, update: QuoteUpdate) {
        self.received = self.received.saturating_add(1);
        let instrument_id = update.snapshot.instrument_id.clone();
        if let Some(pending) = self.pending.get_mut(&instrument_id) {
            self.coalesced = self.coalesced.saturating_add(1);
            *pending = update;
            return;
        }

        if self.pending.len() == self.capacity {
            if let Some(evicted) = self.order.pop_front() {
                self.pending.remove(&evicted);
                self.dropped = self.dropped.saturating_add(1);
            }
        }
        self.order.push_back(instrument_id.clone());
        self.pending.insert(instrument_id, update);
    }

    pub fn drain(&mut self) -> Vec<QuoteUpdate> {
        let mut updates = Vec::with_capacity(self.order.len());
        while let Some(instrument_id) = self.order.pop_front() {
            if let Some(update) = self.pending.remove(&instrument_id) {
                updates.push(update);
            }
        }
        updates
    }

    pub fn clear(&mut self) {
        self.order.clear();
        self.pending.clear();
    }

    pub fn metrics(&self) -> SubscriptionMetrics {
        SubscriptionMetrics {
            received: self.received,
            coalesced: self.coalesced,
            dropped: self.dropped,
            pending: self.pending.len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::market_data::{
        CacheStatus, DataProvenance, DataQuality, ProviderId, UtcTimestamp,
    };

    fn update(id: &str, sequence: u64) -> QuoteUpdate {
        let timestamp = UtcTimestamp::new(format!("2026-08-25T20:00:{sequence:02}Z"));
        QuoteUpdate {
            snapshot: QuoteSnapshot {
                instrument_id: CanonicalInstrumentId::new(id),
                symbol: id.to_owned(),
                currency: "USD".to_owned(),
                last: None,
                change: None,
                bid: None,
                ask: None,
                day_low: None,
                day_high: None,
                volume: None,
                as_of: timestamp.clone(),
                quality: DataQuality::RealTime,
                provenance: DataProvenance {
                    provider: ProviderId::new("test"),
                    source_timestamp: timestamp.clone(),
                    received_at: timestamp,
                    sequence: Some(sequence),
                    cache_status: CacheStatus::Live,
                },
            },
        }
    }

    #[test]
    fn coalesces_by_instrument_and_keeps_latest_value() {
        let mut buffer = CoalescingQuoteBuffer::new(2).expect("buffer");
        buffer.push(update("aapl", 1));
        buffer.push(update("msft", 2));
        buffer.push(update("aapl", 3));

        let drained = buffer.drain();
        assert_eq!(drained.len(), 2);
        assert_eq!(drained[0].snapshot.provenance.sequence, Some(3));
        assert_eq!(buffer.metrics().coalesced, 1);
        assert_eq!(buffer.metrics().pending, 0);
    }

    #[test]
    fn evicts_oldest_distinct_instrument_at_capacity() {
        let mut buffer = CoalescingQuoteBuffer::new(2).expect("buffer");
        buffer.push(update("aapl", 1));
        buffer.push(update("msft", 2));
        buffer.push(update("nvda", 3));

        let drained = buffer.drain();
        assert_eq!(
            drained
                .iter()
                .map(|item| item.snapshot.symbol.as_str())
                .collect::<Vec<_>>(),
            vec!["msft", "nvda"]
        );
        assert_eq!(buffer.metrics().dropped, 1);
    }

    #[test]
    fn subscription_request_deduplicates_without_reordering() {
        let request = QuoteSubscriptionRequest::new(
            vec![
                CanonicalInstrumentId::new("aapl"),
                CanonicalInstrumentId::new("aapl"),
                CanonicalInstrumentId::new("msft"),
            ],
            4,
        )
        .expect("request");
        assert_eq!(request.instruments().len(), 2);
        assert_eq!(request.instruments()[1].as_str(), "msft");
    }
}
