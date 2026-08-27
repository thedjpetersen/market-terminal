//! Official Finnhub quote adapter with explicitly session-derived chart marks.
//!
//! The bounded session-history behavior adapts `makeev/alphai-tui` at commit
//! `9143d2e1176d0a67a9f26960427cf370187fc2e6`.
//! Copyright (c) 2026 Mikhail Makeev, used under the MIT License. See
//! `THIRD_PARTY_NOTICES.md` at the repository root.

use std::{
    collections::HashMap,
    env,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use chrono::{TimeZone, Utc};
use reqwest::{
    blocking::Client,
    header::{HeaderMap, HeaderValue},
    StatusCode, Url,
};
use serde::Deserialize;

use crate::features::{
    charting::{
        ChartHistoryQuery, HistoryError as ChartHistoryError, HistoryQuality,
        HistoryRequest as ChartHistoryRequest, HistorySeries, PriceBar as ChartPriceBar,
    },
    market_data::{
        CacheStatus, CanonicalInstrumentId, DataProvenance, DataQuality, HistoryRequest,
        MarketDataError, MarketDataQuery, Percent, Price, PriceChange, ProviderId, QuoteSnapshot,
        UtcTimestamp,
    },
    spreadsheet::{
        MarketDataPoint, MarketDataProvenance, MarketDataQuality, MarketDataRequest,
        MarketDataState, SpreadsheetMarketData,
    },
};

const DEFAULT_BASE_URL: &str = "https://finnhub.io/api/v1/";
const DEFAULT_TIMEOUT_SECS: u64 = 12;
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const MAX_HISTORY: usize = 600;
const QUOTE_CACHE_TTL: Duration = Duration::from_secs(15);
const PROVIDER_ID: &str = "finnhub-quote";

type QuoteCache = Arc<Mutex<HashMap<String, (Instant, FinnhubQuote)>>>;
type SessionHistory = Arc<Mutex<HashMap<String, Vec<SessionTick>>>>;

#[derive(Clone)]
pub struct FinnhubConfig {
    base_url: Url,
    api_key: Arc<str>,
    timeout: Duration,
}

impl FinnhubConfig {
    pub fn from_env() -> Self {
        let mut base_url = env::var("FINNHUB_BASE_URL")
            .ok()
            .and_then(|value| Url::parse(&value).ok())
            .filter(|url| url.scheme() == "https")
            .unwrap_or_else(|| Url::parse(DEFAULT_BASE_URL).expect("default URL is valid"));
        if !base_url.path().ends_with('/') {
            let path = format!("{}/", base_url.path());
            base_url.set_path(&path);
        }
        let timeout = env::var("FINNHUB_TIMEOUT_SECS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .map(|seconds| seconds.clamp(3, 60))
            .unwrap_or(DEFAULT_TIMEOUT_SECS);
        Self {
            base_url,
            api_key: Arc::from(env::var("FINNHUB_API_KEY").unwrap_or_default()),
            timeout: Duration::from_secs(timeout),
        }
    }

    #[cfg(test)]
    fn fixture(api_key: &str) -> Self {
        Self {
            base_url: Url::parse(DEFAULT_BASE_URL).expect("default URL is valid"),
            api_key: Arc::from(api_key),
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
        }
    }
}

#[derive(Clone)]
pub struct FinnhubMarketData {
    config: FinnhubConfig,
    client: Client,
    quote_cache: QuoteCache,
    history: SessionHistory,
}

impl FinnhubMarketData {
    pub fn new(config: FinnhubConfig) -> Self {
        let client = Client::builder()
            .timeout(config.timeout)
            .user_agent(concat!("market-terminal/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("Finnhub HTTP client should build");
        Self {
            config,
            client,
            quote_cache: Arc::new(Mutex::new(HashMap::new())),
            history: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn from_env() -> Self {
        Self::new(FinnhubConfig::from_env())
    }

    fn quote(&self, symbol: &str) -> Result<(FinnhubQuote, CacheStatus), MarketDataError> {
        let symbol = normalize_symbol(symbol)?;
        if self.config.api_key.trim().is_empty() {
            return Err(MarketDataError::PermissionDenied(
                "Finnhub requires FINNHUB_API_KEY".to_owned(),
            ));
        }
        {
            let cache = self
                .quote_cache
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if let Some((stored_at, quote)) = cache.get(&symbol) {
                if stored_at.elapsed() <= QUOTE_CACHE_TTL {
                    return Ok((quote.clone(), CacheStatus::Fresh));
                }
            }
        }
        let url = self
            .config
            .base_url
            .join("quote")
            .map_err(|_| MarketDataError::InvalidRequest("invalid Finnhub URL".to_owned()))?;
        let mut headers = HeaderMap::new();
        let token = HeaderValue::from_str(self.config.api_key.as_ref()).map_err(|_| {
            MarketDataError::PermissionDenied("FINNHUB_API_KEY is not a valid header".to_owned())
        })?;
        headers.insert("X-Finnhub-Token", token);
        let response = self
            .client
            .get(url)
            .headers(headers)
            .query(&[("symbol", symbol.as_str())])
            .send()
            .map_err(|_| {
                MarketDataError::TemporarilyUnavailable("Finnhub quote request failed".to_owned())
            })?;
        let status = response.status();
        let bytes = response.bytes().map_err(|_| {
            MarketDataError::TemporarilyUnavailable("Finnhub response body failed".to_owned())
        })?;
        if bytes.len() > MAX_RESPONSE_BYTES {
            return Err(provider_error("response exceeded 1 MiB limit", false));
        }
        if status == StatusCode::TOO_MANY_REQUESTS {
            return Err(MarketDataError::RateLimited {
                retry_after_ms: 60_000,
            });
        }
        if matches!(status.as_u16(), 401 | 403) {
            return Err(MarketDataError::PermissionDenied(
                "Finnhub rejected FINNHUB_API_KEY".to_owned(),
            ));
        }
        if !status.is_success() {
            return Err(provider_error(
                &format!("HTTP {}", status.as_u16()),
                status.is_server_error(),
            ));
        }
        let quote: FinnhubQuote = serde_json::from_slice(&bytes)
            .map_err(|_| provider_error("response was not valid JSON", false))?;
        quote.validate(&symbol)?;
        self.quote_cache
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(symbol.clone(), (Instant::now(), quote.clone()));
        self.record_tick(&symbol, &quote);
        Ok((quote, CacheStatus::Live))
    }

    fn record_tick(&self, symbol: &str, quote: &FinnhubQuote) {
        let mut history = self
            .history
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        push_tick(
            history.entry(symbol.to_owned()).or_default(),
            SessionTick {
                timestamp: quote.timestamp,
                price: quote.current,
            },
        );
    }

    fn session_history(&self, symbol: &str) -> Result<Vec<SessionTick>, MarketDataError> {
        let symbol = normalize_symbol(symbol)?;
        let _ = self.quote(&symbol)?;
        Ok(self
            .history
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(&symbol)
            .cloned()
            .unwrap_or_default())
    }
}

impl MarketDataQuery for FinnhubMarketData {
    fn quote_snapshots(
        &self,
        instruments: &[CanonicalInstrumentId],
    ) -> Result<Vec<QuoteSnapshot>, MarketDataError> {
        instruments
            .iter()
            .map(|instrument_id| {
                let symbol = symbol_from_canonical(instrument_id);
                let (quote, cache_status) = self.quote(&symbol)?;
                Ok(quote_snapshot(
                    instrument_id.clone(),
                    symbol,
                    quote,
                    cache_status,
                ))
            })
            .collect()
    }

    fn price_history(
        &self,
        _request: &HistoryRequest,
    ) -> Result<Vec<crate::features::market_data::PriceBar>, MarketDataError> {
        Err(MarketDataError::Unsupported(
            "Finnhub quote access does not include provider candle history; chart marks are session-only"
                .to_owned(),
        ))
    }
}

impl ChartHistoryQuery for FinnhubMarketData {
    fn load_history(
        &self,
        request: &ChartHistoryRequest,
    ) -> Result<HistorySeries, ChartHistoryError> {
        let symbol = request
            .instrument
            .symbol
            .split_whitespace()
            .next()
            .unwrap_or_default();
        let ticks = self.session_history(symbol).map_err(chart_history_error)?;
        if ticks.is_empty() {
            return Err(ChartHistoryError::Unavailable(format!(
                "Finnhub returned no session quote samples for {symbol}"
            )));
        }
        Ok(HistorySeries {
            instrument: request.instrument.clone(),
            bars: ticks
                .into_iter()
                .map(|tick| ChartPriceBar {
                    timestamp: tick.timestamp,
                    open: tick.price,
                    high: tick.price,
                    low: tick.price,
                    close: tick.price,
                    volume: 0,
                })
                .collect(),
            quality: HistoryQuality::Derived,
            source: "FINNHUB QUOTE SAMPLES · SESSION ONLY · NO PROVIDER CANDLES".to_owned(),
        })
    }
}

impl SpreadsheetMarketData for FinnhubMarketData {
    fn load_batch(&self, requests: &[MarketDataRequest]) -> Vec<MarketDataPoint> {
        let mut quotes = HashMap::<String, Result<FinnhubQuote, MarketDataError>>::new();
        for request in requests {
            let symbol = request
                .security
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .to_ascii_uppercase();
            if !quotes.contains_key(&symbol) {
                quotes.insert(symbol.clone(), self.quote(&symbol).map(|(quote, _)| quote));
            }
        }
        let received_at = Utc::now().to_rfc3339();
        requests
            .iter()
            .map(|request| {
                let symbol = request
                    .security
                    .split_whitespace()
                    .next()
                    .unwrap_or_default()
                    .to_ascii_uppercase();
                let state = match quotes.get(&symbol) {
                    Some(Ok(quote)) => {
                        let value = match request.field.as_str() {
                            "PX_LAST" => Some(quote.current),
                            "CHG_PCT_1D" => Some(quote.percent_change),
                            _ => None,
                        };
                        value.map_or_else(
                            || MarketDataState::Unavailable {
                                reason: format!("unsupported field {}", request.field),
                            },
                            |value| MarketDataState::Ready {
                                value,
                                provenance: MarketDataProvenance {
                                    provider: "FINNHUB · QUOTE".to_owned(),
                                    observed_at: timestamp(quote.timestamp).as_str().to_owned(),
                                    received_at: received_at.clone(),
                                    quality: MarketDataQuality::Realtime,
                                },
                            },
                        )
                    }
                    Some(Err(MarketDataError::PermissionDenied(_))) => {
                        MarketDataState::PermissionDenied {
                            provider: "FINNHUB".to_owned(),
                        }
                    }
                    Some(Err(error)) => MarketDataState::Unavailable {
                        reason: error.to_string(),
                    },
                    None => MarketDataState::Unavailable {
                        reason: "quote request was not issued".to_owned(),
                    },
                };
                MarketDataPoint {
                    request: request.clone(),
                    state,
                }
            })
            .collect()
    }
}

fn quote_snapshot(
    instrument_id: CanonicalInstrumentId,
    symbol: String,
    quote: FinnhubQuote,
    cache_status: CacheStatus,
) -> QuoteSnapshot {
    let source_timestamp = timestamp(quote.timestamp);
    QuoteSnapshot {
        instrument_id,
        symbol,
        currency: "USD".to_owned(),
        last: Some(Price::new(quote.current)),
        change: Some(PriceChange {
            absolute: Price::new(quote.change),
            percent: Percent::new(quote.percent_change),
        }),
        bid: None,
        ask: None,
        day_low: Some(Price::new(quote.low)),
        day_high: Some(Price::new(quote.high)),
        volume: None,
        as_of: source_timestamp.clone(),
        quality: DataQuality::RealTime,
        provenance: DataProvenance {
            provider: ProviderId::new(PROVIDER_ID),
            source_timestamp,
            received_at: UtcTimestamp::new(Utc::now().to_rfc3339()),
            sequence: None,
            cache_status,
        },
    }
}

fn push_tick(series: &mut Vec<SessionTick>, tick: SessionTick) {
    match series.last_mut() {
        Some(last) if last.timestamp == tick.timestamp => *last = tick,
        _ => series.push(tick),
    }
    if series.len() > MAX_HISTORY {
        series.drain(..series.len() - MAX_HISTORY);
    }
}

fn timestamp(value: i64) -> UtcTimestamp {
    UtcTimestamp::new(
        Utc.timestamp_opt(value, 0)
            .single()
            .unwrap_or_else(Utc::now)
            .to_rfc3339(),
    )
}

fn chart_history_error(error: MarketDataError) -> ChartHistoryError {
    match error {
        MarketDataError::PermissionDenied(message) => ChartHistoryError::PermissionDenied(message),
        error => ChartHistoryError::Unavailable(error.to_string()),
    }
}

fn normalize_symbol(symbol: &str) -> Result<String, MarketDataError> {
    let symbol = symbol.trim().to_ascii_uppercase();
    if symbol.is_empty()
        || symbol.len() > 32
        || !symbol
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '-'))
    {
        return Err(MarketDataError::InvalidRequest(
            "invalid Finnhub symbol".to_owned(),
        ));
    }
    Ok(symbol)
}

fn symbol_from_canonical(instrument_id: &CanonicalInstrumentId) -> String {
    instrument_id
        .as_str()
        .rsplit(':')
        .next()
        .unwrap_or(instrument_id.as_str())
        .to_ascii_uppercase()
}

fn provider_error(message: &str, retriable: bool) -> MarketDataError {
    MarketDataError::Provider {
        provider: ProviderId::new(PROVIDER_ID),
        message: message.chars().take(240).collect(),
        retriable,
    }
}

#[derive(Debug, Clone, Deserialize)]
struct FinnhubQuote {
    #[serde(rename = "c")]
    current: f64,
    #[serde(rename = "d")]
    change: f64,
    #[serde(rename = "dp")]
    percent_change: f64,
    #[serde(rename = "h")]
    high: f64,
    #[serde(rename = "l")]
    low: f64,
    #[serde(rename = "o")]
    open: f64,
    #[serde(rename = "pc")]
    previous_close: f64,
    #[serde(rename = "t")]
    timestamp: i64,
}

impl FinnhubQuote {
    fn validate(&self, symbol: &str) -> Result<(), MarketDataError> {
        if self.current == 0.0 && self.previous_close == 0.0 && self.timestamp == 0 {
            return Err(MarketDataError::InvalidRequest(format!(
                "Finnhub returned no quote for {symbol}"
            )));
        }
        if self.timestamp <= 0
            || ![
                self.current,
                self.change,
                self.percent_change,
                self.high,
                self.low,
                self.open,
                self.previous_close,
            ]
            .into_iter()
            .all(f64::is_finite)
            || self.current <= 0.0
            || self.high < self.low
        {
            return Err(provider_error("quote contained invalid fields", false));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct SessionTick {
    timestamp: i64,
    price: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_maps_real_fields_and_provenance() {
        let quote: FinnhubQuote = serde_json::from_str(
            r#"{"c":261.74,"d":2.29,"dp":0.8826,"h":263.31,"l":260.68,"o":261.07,"pc":259.45,"t":1783526400}"#,
        )
        .unwrap();
        quote.validate("AAPL").unwrap();

        let snapshot = quote_snapshot(
            CanonicalInstrumentId::new("us:listed:aapl"),
            "AAPL".to_owned(),
            quote,
            CacheStatus::Live,
        );

        assert_eq!(snapshot.last.map(Price::value), Some(261.74));
        assert_eq!(
            snapshot
                .day_range()
                .map(|(low, high)| (low.value(), high.value())),
            Some((260.68, 263.31))
        );
        assert_eq!(snapshot.quality, DataQuality::RealTime);
        assert_eq!(snapshot.provenance.provider.as_str(), PROVIDER_ID);
    }

    #[test]
    fn session_ticks_update_duplicates_and_remain_bounded() {
        let mut ticks = Vec::new();
        push_tick(
            &mut ticks,
            SessionTick {
                timestamp: 1,
                price: 10.0,
            },
        );
        push_tick(
            &mut ticks,
            SessionTick {
                timestamp: 1,
                price: 11.0,
            },
        );
        for timestamp in 2..=(MAX_HISTORY as i64 + 20) {
            push_tick(
                &mut ticks,
                SessionTick {
                    timestamp,
                    price: 12.0,
                },
            );
        }

        assert_eq!(ticks.len(), MAX_HISTORY);
        assert_eq!(ticks.last().map(|tick| tick.timestamp), Some(620));
        assert_eq!(ticks.first().map(|tick| tick.timestamp), Some(21));
    }

    #[test]
    fn missing_credentials_are_permission_denied_before_network_access() {
        let adapter = FinnhubMarketData::new(FinnhubConfig::fixture(""));

        assert!(matches!(
            adapter.quote("AAPL"),
            Err(MarketDataError::PermissionDenied(_))
        ));
    }

    #[test]
    fn rejects_symbols_that_could_mutate_provider_queries() {
        assert!(normalize_symbol("AAPL").is_ok());
        assert!(normalize_symbol("AAPL&token=secret").is_err());
    }
}
