use std::sync::{
    atomic::{AtomicU64, Ordering},
    Mutex,
};

use crate::features::{
    market_data::{
        CacheStatus, CancellationToken, CanonicalInstrumentId, CoalescingQuoteBuffer,
        DataProvenance, DataQuality, HistoryRequest, MarketDataError, MarketDataQuery, Percent,
        Price, PriceBar, PriceChange, ProviderId, Quantity, QuoteSnapshot, QuoteSubscription,
        QuoteSubscriptionRequest, QuoteUpdate, SubscriptionId, SubscriptionMetrics, UtcTimestamp,
    },
    watchlist::{
        MonitorColumn, SortDirection, SortField, SortSpec, WatchlistCatalog,
        WatchlistDefinition, WatchlistItem,
    },
};

/// Deterministic, finite market-data replay for demos and integration tests.
///
/// Every quote batch advances exactly one frame and the final frame is held.
/// Rendering therefore never changes data; only an explicit workspace refresh
/// (or another query) advances the replay.
pub struct DemoMarketDataReplay {
    cursor: Mutex<usize>,
    next_subscription_id: AtomicU64,
}

impl DemoMarketDataReplay {
    pub fn new() -> Self {
        Self { cursor: Mutex::new(0), next_subscription_id: AtomicU64::new(1) }
    }

    #[cfg(test)]
    fn cursor(&self) -> usize { *self.cursor.lock().expect("replay cursor poisoned") }
}

impl Default for DemoMarketDataReplay {
    fn default() -> Self { Self::new() }
}

impl MarketDataQuery for DemoMarketDataReplay {
    fn quote_snapshots(
        &self,
        instruments: &[CanonicalInstrumentId],
    ) -> Result<Vec<QuoteSnapshot>, MarketDataError> {
        let mut cursor = self.cursor.lock().map_err(|_| {
            MarketDataError::TemporarilyUnavailable("demo replay cursor poisoned".to_owned())
        })?;
        let frame_index = (*cursor).min(REPLAY_FRAMES.len() - 1);
        let frame = &REPLAY_FRAMES[frame_index];
        *cursor = (*cursor + 1).min(REPLAY_FRAMES.len() - 1);

        Ok(instruments
            .iter()
            .map(|instrument_id| {
                frame
                    .quotes
                    .iter()
                    .find(|quote| quote.id == instrument_id.as_str())
                    .map(|quote| quote.snapshot(frame.as_of, frame_index as u64))
                    .unwrap_or_else(|| {
                        unavailable_quote(instrument_id.clone(), frame.as_of, frame_index as u64)
                    })
            })
            .collect())
    }

    fn price_history(&self, request: &HistoryRequest) -> Result<Vec<PriceBar>, MarketDataError> {
        if request.start > request.end {
            return Err(MarketDataError::InvalidRequest(
                "history start must not be after end".to_owned(),
            ));
        }

        let base = match request.instrument_id.as_str() {
            "us:xnas:aapl" => 202.10,
            "us:xnas:msft" => 508.20,
            "us:xnas:nvda" => 180.30,
            "us:arcx:spy" => 648.70,
            "index:spx" => 5_258.20,
            _ => return Ok(Vec::new()),
        };
        let times = [
            "2026-08-21T00:00:00Z",
            "2026-08-22T00:00:00Z",
            "2026-08-25T00:00:00Z",
        ];
        Ok(times
            .into_iter()
            .enumerate()
            .filter(|(_, timestamp)| {
                *timestamp >= request.start.as_str() && *timestamp <= request.end.as_str()
            })
            .map(|(index, timestamp)| {
                let open = base + index as f64 * 1.15;
                PriceBar {
                    instrument_id: request.instrument_id.clone(),
                    interval: request.interval,
                    opened_at: UtcTimestamp::new(timestamp),
                    open: Price::new(open),
                    high: Price::new(open + 1.42),
                    low: Price::new(open - 0.88),
                    close: Price::new(open + 0.64),
                    volume: Quantity::new(30_000_000 + index as u64 * 2_500_000),
                    quality: DataQuality::RealTime,
                }
            })
            .collect())
    }

    fn subscribe_quotes(
        &self,
        request: QuoteSubscriptionRequest,
    ) -> Result<Box<dyn QuoteSubscription>, MarketDataError> {
        let id = SubscriptionId::new(self.next_subscription_id.fetch_add(1, Ordering::Relaxed));
        let mut buffer = CoalescingQuoteBuffer::new(request.capacity())?;
        // Seed both finite frames. Per-instrument coalescing makes the newest
        // replay value observable while exercising overload behavior.
        for (frame_index, frame) in REPLAY_FRAMES.iter().enumerate() {
            for instrument_id in request.instruments() {
                let snapshot = frame
                    .quotes
                    .iter()
                    .find(|quote| quote.id == instrument_id.as_str())
                    .map(|quote| quote.snapshot(frame.as_of, frame_index as u64))
                    .unwrap_or_else(|| {
                        unavailable_quote(instrument_id.clone(), frame.as_of, frame_index as u64)
                    });
                buffer.push(QuoteUpdate { snapshot });
            }
        }
        Ok(Box::new(DemoQuoteSubscription {
            id,
            cancellation: CancellationToken::default(),
            buffer,
        }))
    }
}

struct DemoQuoteSubscription {
    id: SubscriptionId,
    cancellation: CancellationToken,
    buffer: CoalescingQuoteBuffer,
}

impl QuoteSubscription for DemoQuoteSubscription {
    fn id(&self) -> SubscriptionId { self.id }

    fn drain(&mut self) -> Result<Vec<QuoteUpdate>, MarketDataError> {
        if self.cancellation.is_cancelled() {
            return Err(MarketDataError::Cancelled);
        }
        Ok(self.buffer.drain())
    }

    fn cancel(&mut self) {
        self.cancellation.cancel();
        self.buffer.clear();
    }

    fn is_cancelled(&self) -> bool { self.cancellation.is_cancelled() }

    fn metrics(&self) -> SubscriptionMetrics { self.buffer.metrics() }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct DemoWatchlistCatalog;

impl WatchlistCatalog for DemoWatchlistCatalog {
    fn load_watchlist(&self, name: Option<&str>) -> Option<WatchlistDefinition> {
        match name.unwrap_or("CORE").trim().to_ascii_uppercase().as_str() {
            "CORE" | "DEFAULT" | "EQUITIES" => Some(core_watchlist()),
            "MACRO" | "CROSS ASSET" => Some(macro_watchlist()),
            "MOVERS" => Some(movers_watchlist()),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct ReplayFrame {
    as_of: &'static str,
    quotes: &'static [ReplayQuote],
}

#[derive(Debug, Clone, Copy)]
struct ReplayQuote {
    id: &'static str,
    symbol: &'static str,
    currency: &'static str,
    last: Option<f64>,
    change: Option<f64>,
    percent: Option<f64>,
    bid: Option<f64>,
    ask: Option<f64>,
    volume: Option<u64>,
    quality: DataQuality,
}

impl ReplayQuote {
    fn snapshot(self, as_of: &str, sequence: u64) -> QuoteSnapshot {
        let timestamp = UtcTimestamp::new(as_of);
        QuoteSnapshot {
            instrument_id: CanonicalInstrumentId::new(self.id),
            symbol: self.symbol.to_owned(),
            currency: self.currency.to_owned(),
            last: self.last.map(Price::new),
            change: self.change.zip(self.percent).map(|(absolute, percent)| PriceChange {
                absolute: Price::new(absolute),
                percent: Percent::new(percent),
            }),
            bid: self.bid.map(Price::new),
            ask: self.ask.map(Price::new),
            volume: self.volume.map(Quantity::new),
            as_of: timestamp.clone(),
            quality: self.quality,
            provenance: DataProvenance {
                provider: ProviderId::new("demo-replay"),
                source_timestamp: timestamp.clone(),
                received_at: timestamp,
                sequence: Some(sequence),
                cache_status: CacheStatus::Live,
            },
        }
    }
}

const FRAME_0: [ReplayQuote; 10] = [
    replay_quote("us:xnas:aapl", "AAPL", "USD", 205.30, 1.72, 0.84, 205.28, 205.32, 41_820_000, DataQuality::RealTime),
    replay_quote("us:xnas:msft", "MSFT", "USD", 512.44, 3.81, 0.75, 512.40, 512.48, 22_140_000, DataQuality::RealTime),
    replay_quote("us:xnas:nvda", "NVDA", "USD", 184.92, 4.26, 2.36, 184.88, 184.96, 188_450_000, DataQuality::Delayed { minutes: 15 }),
    replay_quote("us:xnas:meta", "META", "USD", 738.10, 8.42, 1.15, 737.90, 738.28, 14_880_000, DataQuality::Stale { age_seconds: 180 }),
    replay_quote("us:arcx:spy", "SPY", "USD", 653.28, 4.13, 0.64, 653.26, 653.30, 69_210_000, DataQuality::RealTime),
    replay_quote("index:spx", "SPX", "USD", 5304.72, 45.18, 0.86, 5304.50, 5304.94, 0, DataQuality::Derived),
    replay_quote("index:ndx", "NDX", "USD", 18658.32, 183.72, 1.00, 18657.80, 18658.84, 0, DataQuality::Delayed { minutes: 15 }),
    replay_quote("fx:eurusd", "EURUSD", "USD", 1.0837, 0.0023, 0.21, 1.0836, 1.0838, 0, DataQuality::RealTime),
    replay_quote("commodity:cl", "CL", "USD", 78.42, 0.88, 1.14, 78.41, 78.43, 312_400, DataQuality::Stale { age_seconds: 42 }),
    ReplayQuote { id: "commodity:xau", symbol: "XAU", currency: "USD", last: None, change: None, percent: None, bid: None, ask: None, volume: None, quality: DataQuality::PermissionDenied },
];

const FRAME_1: [ReplayQuote; 10] = [
    replay_quote("us:xnas:aapl", "AAPL", "USD", 205.36, 1.78, 0.87, 205.34, 205.38, 41_901_000, DataQuality::RealTime),
    replay_quote("us:xnas:msft", "MSFT", "USD", 512.39, 3.76, 0.74, 512.35, 512.43, 22_188_000, DataQuality::RealTime),
    replay_quote("us:xnas:nvda", "NVDA", "USD", 184.92, 4.26, 2.36, 184.88, 184.96, 188_450_000, DataQuality::Delayed { minutes: 15 }),
    replay_quote("us:xnas:meta", "META", "USD", 738.10, 8.42, 1.15, 737.90, 738.28, 14_880_000, DataQuality::Stale { age_seconds: 181 }),
    replay_quote("us:arcx:spy", "SPY", "USD", 653.31, 4.16, 0.64, 653.29, 653.33, 69_295_000, DataQuality::RealTime),
    replay_quote("index:spx", "SPX", "USD", 5305.04, 45.50, 0.87, 5304.82, 5305.26, 0, DataQuality::Derived),
    replay_quote("index:ndx", "NDX", "USD", 18658.32, 183.72, 1.00, 18657.80, 18658.84, 0, DataQuality::Delayed { minutes: 15 }),
    replay_quote("fx:eurusd", "EURUSD", "USD", 1.0838, 0.0024, 0.22, 1.0837, 1.0839, 0, DataQuality::RealTime),
    replay_quote("commodity:cl", "CL", "USD", 78.44, 0.90, 1.16, 78.43, 78.45, 313_100, DataQuality::Stale { age_seconds: 43 }),
    ReplayQuote { id: "commodity:xau", symbol: "XAU", currency: "USD", last: None, change: None, percent: None, bid: None, ask: None, volume: None, quality: DataQuality::PermissionDenied },
];

const REPLAY_FRAMES: [ReplayFrame; 2] = [
    ReplayFrame { as_of: "2026-08-25T20:00:00Z", quotes: &FRAME_0 },
    ReplayFrame { as_of: "2026-08-25T20:00:01Z", quotes: &FRAME_1 },
];

// A row-shaped fixture constructor keeps the two replay frames visually diffable.
#[allow(clippy::too_many_arguments)]
const fn replay_quote(
    id: &'static str,
    symbol: &'static str,
    currency: &'static str,
    last: f64,
    change: f64,
    percent: f64,
    bid: f64,
    ask: f64,
    volume: u64,
    quality: DataQuality,
) -> ReplayQuote {
    ReplayQuote {
        id,
        symbol,
        currency,
        last: Some(last),
        change: Some(change),
        percent: Some(percent),
        bid: Some(bid),
        ask: Some(ask),
        volume: if volume > 0 { Some(volume) } else { None },
        quality,
    }
}

fn unavailable_quote(
    instrument_id: CanonicalInstrumentId,
    as_of: &str,
    sequence: u64,
) -> QuoteSnapshot {
    let timestamp = UtcTimestamp::new(as_of);
    QuoteSnapshot {
        symbol: instrument_id.as_str().to_ascii_uppercase(),
        instrument_id,
        currency: String::new(),
        last: None,
        change: None,
        bid: None,
        ask: None,
        volume: None,
        as_of: timestamp.clone(),
        quality: DataQuality::Unavailable,
        provenance: DataProvenance {
            provider: ProviderId::new("demo-replay"),
            source_timestamp: timestamp.clone(),
            received_at: timestamp,
            sequence: Some(sequence),
            cache_status: CacheStatus::Live,
        },
    }
}

fn core_watchlist() -> WatchlistDefinition {
    WatchlistDefinition::new(
        "core",
        "CORE EQUITIES",
        vec![
            item("us:xnas:aapl", "AAPL", "Apple Inc."),
            item("us:xnas:msft", "MSFT", "Microsoft Corporation"),
            item("us:xnas:nvda", "NVDA", "NVIDIA Corporation"),
            item("us:xnas:meta", "META", "Meta Platforms"),
            item("us:arcx:spy", "SPY", "SPDR S&P 500 ETF"),
        ],
    )
}

fn macro_watchlist() -> WatchlistDefinition {
    WatchlistDefinition::new(
        "macro",
        "CROSS ASSET MACRO",
        vec![
            item("index:spx", "SPX", "S&P 500 Index"),
            item("index:ndx", "NDX", "NASDAQ 100 Index"),
            item("fx:eurusd", "EURUSD", "Euro / U.S. Dollar"),
            item("commodity:cl", "CL", "WTI Crude Oil"),
            item("commodity:xau", "XAU", "Gold Spot"),
        ],
    )
    .with_columns(vec![
        MonitorColumn::Symbol,
        MonitorColumn::Last,
        MonitorColumn::Change,
        MonitorColumn::ChangePercent,
        MonitorColumn::Bid,
        MonitorColumn::Ask,
        MonitorColumn::Quality,
        MonitorColumn::AsOf,
    ])
}

fn movers_watchlist() -> WatchlistDefinition {
    WatchlistDefinition::new(
        "movers",
        "PRICE MOVERS",
        vec![
            item("us:xnas:nvda", "NVDA", "NVIDIA Corporation"),
            item("us:xnas:meta", "META", "Meta Platforms"),
            item("us:xnas:aapl", "AAPL", "Apple Inc."),
            item("us:xnas:msft", "MSFT", "Microsoft Corporation"),
        ],
    )
    .with_sort(SortSpec {
        field: SortField::ChangePercent,
        direction: SortDirection::Descending,
    })
}

fn item(id: &str, symbol: &str, description: &str) -> WatchlistItem {
    WatchlistItem::new(CanonicalInstrumentId::new(id), symbol, description)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::market_data::BarInterval;

    #[test]
    fn replay_advances_once_per_quote_batch_then_holds() {
        let replay = DemoMarketDataReplay::new();
        let ids = [CanonicalInstrumentId::new("us:xnas:aapl")];

        let first = replay.quote_snapshots(&ids).expect("frame zero");
        let second = replay.quote_snapshots(&ids).expect("frame one");
        let held = replay.quote_snapshots(&ids).expect("held frame");

        assert_eq!(first[0].last, Some(Price::new(205.30)));
        assert_eq!(second[0].last, Some(Price::new(205.36)));
        assert_eq!(held, second);
        assert_eq!(replay.cursor(), 1);
    }

    #[test]
    fn quote_batches_preserve_canonical_request_order_and_missing_state() {
        let replay = DemoMarketDataReplay::new();
        let ids = [
            CanonicalInstrumentId::new("fx:eurusd"),
            CanonicalInstrumentId::new("unknown:test"),
            CanonicalInstrumentId::new("us:xnas:aapl"),
        ];

        let quotes = replay.quote_snapshots(&ids).expect("quotes");

        assert_eq!(quotes.iter().map(|quote| quote.instrument_id.as_str()).collect::<Vec<_>>(), vec!["fx:eurusd", "unknown:test", "us:xnas:aapl"]);
        assert_eq!(quotes[1].quality, DataQuality::Unavailable);
    }

    #[test]
    fn history_is_fixed_and_validates_time_range() {
        let replay = DemoMarketDataReplay::new();
        let request = HistoryRequest {
            instrument_id: CanonicalInstrumentId::new("us:xnas:aapl"),
            interval: BarInterval::OneDay,
            start: UtcTimestamp::new("2026-08-21T00:00:00Z"),
            end: UtcTimestamp::new("2026-08-25T00:00:00Z"),
        };
        assert_eq!(replay.price_history(&request).expect("history").len(), 3);
    }

    #[test]
    fn catalog_resolves_names_without_symbol_identity_leaks() {
        let macro_list = DemoWatchlistCatalog.load_watchlist(Some("macro")).expect("macro list");
        assert_eq!(macro_list.id, "macro");
        assert_eq!(macro_list.items[0].instrument_id.as_str(), "index:spx");
    }

    #[test]
    fn stream_is_bounded_coalesced_and_cancellable() {
        let replay = DemoMarketDataReplay::new();
        let request = QuoteSubscriptionRequest::new(
            vec![
                CanonicalInstrumentId::new("us:xnas:aapl"),
                CanonicalInstrumentId::new("us:xnas:msft"),
            ],
            2,
        )
        .expect("request");
        let mut subscription = replay.subscribe_quotes(request).expect("subscription");

        let metrics = subscription.metrics();
        assert_eq!(metrics.received, 4);
        assert_eq!(metrics.coalesced, 2);
        let updates = subscription.drain().expect("updates");
        assert_eq!(updates.len(), 2);
        assert_eq!(updates[0].snapshot.last, Some(Price::new(205.36)));
        assert_eq!(updates[0].snapshot.provenance.sequence, Some(1));

        subscription.cancel();
        assert!(subscription.is_cancelled());
        assert_eq!(subscription.drain(), Err(MarketDataError::Cancelled));
    }
}
