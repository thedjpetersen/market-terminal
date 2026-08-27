use std::{
    collections::HashSet,
    sync::{
        mpsc::{sync_channel, Receiver, SyncSender, TrySendError},
        Arc, RwLock,
    },
    thread,
    time::Duration,
};

use crate::{
    features::{
        market_data::{CanonicalInstrumentId, MarketDataQuery, QuoteSnapshot},
        markets::{LiveMarketRow, LiveMarketsSnapshot, MarketsQuery, MarketsSnapshot},
    },
    foundation::InstrumentId,
};

const MAX_MARKET_SYMBOLS: usize = 12;

enum WorkerCommand {
    Refresh,
    Stop,
}

/// Background, provider-neutral market snapshot composition.
///
/// This adapter deliberately covers only listed instruments available through
/// the configured market-data provider. It does not substitute equity proxies
/// for rates, breadth, sectors, currencies, commodities, or calendars.
pub struct LiveMarketsQuery {
    state: Arc<RwLock<LiveMarketsSnapshot>>,
    commands: SyncSender<WorkerCommand>,
}

impl LiveMarketsQuery {
    pub fn new(
        market_data: Arc<dyn MarketDataQuery>,
        symbols: Vec<String>,
        refresh_interval: Duration,
    ) -> Self {
        let symbols = normalize_symbols(symbols);
        let state = Arc::new(RwLock::new(LiveMarketsSnapshot {
            rows: Vec::new(),
            status: format!("EXTERNAL MARKET DATA · LOADING {} SYMBOL(S)", symbols.len()),
        }));
        let (commands, receiver) = sync_channel(1);
        let worker_state = state.clone();
        let spawn_result = thread::Builder::new()
            .name("market-terminal-markets".to_owned())
            .spawn(move || {
                run_worker(
                    market_data,
                    symbols,
                    refresh_interval,
                    worker_state,
                    receiver,
                )
            });
        if let Err(error) = spawn_result {
            state.write().expect("markets state lock").status =
                format!("MARKET SNAPSHOT WORKER UNAVAILABLE · {error}");
        }
        Self { state, commands }
    }
}

impl MarketsQuery for LiveMarketsQuery {
    fn load_markets(&self) -> MarketsSnapshot {
        MarketsSnapshot::Live(self.state.read().expect("markets state lock").clone())
    }

    fn request_refresh(&self) {
        match self.commands.try_send(WorkerCommand::Refresh) {
            Ok(()) | Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {}
        }
    }
}

impl Drop for LiveMarketsQuery {
    fn drop(&mut self) {
        // Never wait for an in-flight bounded provider request during terminal
        // shutdown. A full queue means a refresh is already coalesced.
        let _ = self.commands.try_send(WorkerCommand::Stop);
    }
}

fn run_worker(
    market_data: Arc<dyn MarketDataQuery>,
    symbols: Vec<String>,
    refresh_interval: Duration,
    state: Arc<RwLock<LiveMarketsSnapshot>>,
    commands: Receiver<WorkerCommand>,
) {
    loop {
        refresh(&*market_data, &symbols, &state);
        match commands.recv_timeout(refresh_interval) {
            Ok(WorkerCommand::Refresh) | Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Ok(WorkerCommand::Stop) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

fn refresh(
    market_data: &dyn MarketDataQuery,
    symbols: &[String],
    state: &RwLock<LiveMarketsSnapshot>,
) {
    let instruments = symbols
        .iter()
        .map(|symbol| InstrumentId::new(format!("us:listed:{}", symbol.to_ascii_lowercase())))
        .collect::<Vec<CanonicalInstrumentId>>();
    match market_data.quote_snapshots(&instruments) {
        Ok(quotes) => {
            let rows = quotes.iter().map(format_quote).collect::<Vec<_>>();
            let provider = rows
                .first()
                .map(|row| row.provider.as_str())
                .unwrap_or("CURRENT PROVIDER");
            let status = if rows.is_empty() {
                format!("{provider} · NO SNAPSHOTS RETURNED · F9 REFRESH")
            } else {
                format!(
                    "{provider} · {}/{} SNAPSHOT(S) · F9 REFRESH",
                    rows.len(),
                    symbols.len()
                )
            };
            *state.write().expect("markets state lock") = LiveMarketsSnapshot { rows, status };
        }
        Err(error) => {
            let mut state = state.write().expect("markets state lock");
            state.status = if state.rows.is_empty() {
                format!("MARKET SNAPSHOTS UNAVAILABLE · {error} · F9 RETRY")
            } else {
                format!("LAST KNOWN SNAPSHOTS · REFRESH FAILED · {error} · F9 RETRY")
            };
        }
    }
}

fn format_quote(quote: &QuoteSnapshot) -> LiveMarketRow {
    let (net_change, percent_change) = quote
        .change
        .map(|change| {
            (
                format_signed(change.absolute.value(), 2),
                format!("{}%", format_signed(change.percent.value(), 2)),
            )
        })
        .unwrap_or_else(|| ("—".to_owned(), "—".to_owned()));
    LiveMarketRow {
        symbol: quote.symbol.clone(),
        last: quote
            .last
            .map(|price| format_price(price.value()))
            .unwrap_or_else(|| "—".to_owned()),
        net_change,
        percent_change,
        quality: quote.quality.label(),
        as_of: quote.as_of.as_str().to_owned(),
        provider: quote.provenance.provider.as_str().to_owned(),
    }
}

fn normalize_symbols(symbols: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let symbols = symbols
        .into_iter()
        .map(|symbol| symbol.trim().to_ascii_uppercase())
        .filter(|symbol| {
            !symbol.is_empty()
                && symbol.len() <= 16
                && symbol.chars().all(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '.' | '-')
                })
        })
        .filter(|symbol| seen.insert(symbol.clone()))
        .take(MAX_MARKET_SYMBOLS)
        .collect::<Vec<_>>();
    if symbols.is_empty() {
        vec!["IBM".to_owned()]
    } else {
        symbols
    }
}

fn format_price(value: f64) -> String {
    format!("{value:.2}")
}

fn format_signed(value: f64, decimals: usize) -> String {
    if value > 0.0 {
        format!("+{value:.decimals$}")
    } else {
        format!("{value:.decimals$}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::market_data::{
        CacheStatus, DataProvenance, DataQuality, HistoryRequest, MarketDataError, Percent, Price,
        PriceBar, PriceChange, ProviderId, UtcTimestamp,
    };

    struct QuoteFixture;

    impl MarketDataQuery for QuoteFixture {
        fn quote_snapshots(
            &self,
            instruments: &[CanonicalInstrumentId],
        ) -> Result<Vec<QuoteSnapshot>, MarketDataError> {
            Ok(instruments
                .iter()
                .map(|instrument| QuoteSnapshot {
                    instrument_id: instrument.clone(),
                    symbol: instrument
                        .as_str()
                        .rsplit(':')
                        .next()
                        .unwrap_or_default()
                        .to_ascii_uppercase(),
                    currency: "USD".to_owned(),
                    last: Some(Price::new(123.45)),
                    change: Some(PriceChange {
                        absolute: Price::new(1.25),
                        percent: Percent::new(1.02),
                    }),
                    bid: None,
                    ask: None,
                    volume: None,
                    as_of: UtcTimestamp::new("2026-08-26T19:00:00Z"),
                    quality: DataQuality::RealTime,
                    provenance: DataProvenance {
                        provider: ProviderId::new("TEST FEED"),
                        source_timestamp: UtcTimestamp::new("2026-08-26T19:00:00Z"),
                        received_at: UtcTimestamp::new("2026-08-26T19:00:01Z"),
                        sequence: None,
                        cache_status: CacheStatus::Live,
                    },
                })
                .collect())
        }

        fn price_history(
            &self,
            _request: &HistoryRequest,
        ) -> Result<Vec<PriceBar>, MarketDataError> {
            Err(MarketDataError::Unsupported("history fixture".to_owned()))
        }
    }

    #[test]
    fn snapshots_are_loaded_in_the_background_and_keep_provider_provenance() {
        let query = LiveMarketsQuery::new(
            Arc::new(QuoteFixture),
            vec!["spy".to_owned(), "spy".to_owned(), "qqq".to_owned()],
            Duration::from_secs(60),
        );

        let mut snapshot = LiveMarketsSnapshot::default();
        for _ in 0..100 {
            let MarketsSnapshot::Live(current) = query.load_markets() else {
                panic!("live adapter must produce live state")
            };
            snapshot = current;
            if snapshot.rows.len() == 2 {
                break;
            }
            thread::sleep(Duration::from_millis(1));
        }

        assert_eq!(snapshot.rows.len(), 2);
        assert_eq!(snapshot.rows[0].symbol, "SPY");
        assert_eq!(snapshot.rows[0].provider, "TEST FEED");
        assert_eq!(snapshot.rows[0].quality, "REALTIME");
        assert!(snapshot.status.contains("2/2 SNAPSHOT(S)"));
    }

    #[test]
    fn symbol_configuration_is_bounded_deduplicated_and_provider_safe() {
        let symbols = normalize_symbols(vec![
            " spy ".to_owned(),
            "SPY".to_owned(),
            "../../secret".to_owned(),
            "brk.b".to_owned(),
        ]);
        assert_eq!(symbols, ["SPY", "BRK.B"]);
    }

    #[test]
    fn the_market_worker_never_blocks_construction() {
        struct SlowQuery;
        impl MarketDataQuery for SlowQuery {
            fn quote_snapshots(
                &self,
                _instruments: &[CanonicalInstrumentId],
            ) -> Result<Vec<QuoteSnapshot>, MarketDataError> {
                thread::sleep(Duration::from_millis(200));
                Ok(Vec::new())
            }

            fn price_history(
                &self,
                _request: &HistoryRequest,
            ) -> Result<Vec<PriceBar>, MarketDataError> {
                Err(MarketDataError::Unsupported("history fixture".to_owned()))
            }
        }

        let started = std::time::Instant::now();
        let _query = LiveMarketsQuery::new(
            Arc::new(SlowQuery),
            vec!["IBM".to_owned()],
            Duration::from_secs(60),
        );
        assert!(started.elapsed() < Duration::from_millis(100));
    }
}
