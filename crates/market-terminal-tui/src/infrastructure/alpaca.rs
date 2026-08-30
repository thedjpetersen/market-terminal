//! Official Alpaca Market Data API adapter.
//!
//! The tolerant response shapes, price fallback order, descending-bar handling,
//! and explicit IEX feed selection are adapted from `makeev/alphai-tui` at
//! commit `9143d2e1176d0a67a9f26960427cf370187fc2e6`.
//! Copyright (c) 2026 Mikhail Makeev, used under the MIT License. See
//! `THIRD_PARTY_NOTICES.md` at the repository root.

use std::{
    collections::HashMap,
    env,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use chrono::{Datelike, TimeZone, Utc};
use reqwest::{blocking::Client, header, StatusCode, Url};
use serde::de::DeserializeOwned;
use serde::Deserialize;

use crate::features::{
    charting::{
        ChartHistoryQuery, ChartPeriod, HistoryError as ChartHistoryError, HistoryQuality,
        HistoryRequest as ChartHistoryRequest, HistorySeries, PriceBar as ChartPriceBar,
    },
    market_data::{
        BarInterval, CacheStatus, CanonicalInstrumentId, DataProvenance, DataQuality,
        HistoryRequest, MarketDataError, MarketDataQuery, Percent, Price, PriceBar, PriceChange,
        ProviderId, Quantity, QuoteSnapshot, UtcTimestamp,
    },
    spreadsheet::{
        MarketDataPoint, MarketDataProvenance, MarketDataQuality, MarketDataRequest,
        MarketDataState, SpreadsheetMarketData,
    },
};

const DEFAULT_BASE_URL: &str = "https://data.alpaca.markets/";
const DEFAULT_TIMEOUT_SECS: u64 = 12;
const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
const MAX_SNAPSHOT_SYMBOLS: usize = 200;
const MAX_BARS: usize = 10_000;
const SNAPSHOT_CACHE_TTL: Duration = Duration::from_secs(5);

type SnapshotCache = Arc<Mutex<HashMap<String, (Instant, ProviderSnapshot)>>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AlpacaFeed {
    Iex,
    Sip,
}

impl AlpacaFeed {
    fn from_env() -> Self {
        match env::var("ALPACA_FEED")
            .unwrap_or_else(|_| "iex".to_owned())
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "sip" => Self::Sip,
            _ => Self::Iex,
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Iex => "iex",
            Self::Sip => "sip",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Iex => "IEX",
            Self::Sip => "SIP",
        }
    }
}

#[derive(Clone)]
pub struct AlpacaConfig {
    base_url: Url,
    key_id: Arc<str>,
    secret_key: Arc<str>,
    feed: AlpacaFeed,
    timeout: Duration,
}

impl AlpacaConfig {
    pub fn from_env() -> Self {
        let base_url = env::var("ALPACA_DATA_URL")
            .ok()
            .and_then(|value| Url::parse(&value).ok())
            .filter(|url| url.scheme() == "https")
            .unwrap_or_else(|| Url::parse(DEFAULT_BASE_URL).expect("default URL is valid"));
        let timeout = env::var("ALPACA_TIMEOUT_SECS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .map(|seconds| seconds.clamp(3, 60))
            .unwrap_or(DEFAULT_TIMEOUT_SECS);
        Self {
            base_url,
            key_id: Arc::from(env::var("APCA_API_KEY_ID").unwrap_or_default()),
            secret_key: Arc::from(env::var("APCA_API_SECRET_KEY").unwrap_or_default()),
            feed: AlpacaFeed::from_env(),
            timeout: Duration::from_secs(timeout),
        }
    }

    #[cfg(test)]
    fn fixture() -> Self {
        Self {
            base_url: Url::parse(DEFAULT_BASE_URL).expect("default URL is valid"),
            key_id: Arc::from("fixture-key"),
            secret_key: Arc::from("fixture-secret"),
            feed: AlpacaFeed::Iex,
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
        }
    }
}

#[derive(Clone)]
pub struct AlpacaMarketData {
    config: AlpacaConfig,
    client: Client,
    snapshot_cache: SnapshotCache,
}

impl AlpacaMarketData {
    pub fn new(config: AlpacaConfig) -> Self {
        let client = Client::builder()
            .timeout(config.timeout)
            .user_agent(concat!("market-terminal/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("Alpaca HTTP client should build");
        Self {
            config,
            client,
            snapshot_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn from_env() -> Self {
        Self::new(AlpacaConfig::from_env())
    }

    fn provider_id(&self) -> ProviderId {
        ProviderId::new(format!("alpaca-{}", self.config.feed.as_str()))
    }

    fn source_label(&self) -> String {
        format!("ALPACA · {} REALTIME", self.config.feed.label())
    }

    fn snapshots(
        &self,
        symbols: &[String],
    ) -> Result<HashMap<String, (ProviderSnapshot, CacheStatus)>, MarketDataError> {
        if symbols.is_empty() {
            return Ok(HashMap::new());
        }
        if symbols.len() > MAX_SNAPSHOT_SYMBOLS {
            return Err(MarketDataError::InvalidRequest(format!(
                "Alpaca snapshot batches support at most {MAX_SNAPSHOT_SYMBOLS} symbols"
            )));
        }

        let mut resolved = HashMap::new();
        let mut missing = Vec::new();
        {
            let cache = self
                .snapshot_cache
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            for symbol in symbols {
                if let Some((stored_at, snapshot)) = cache.get(symbol) {
                    if stored_at.elapsed() <= SNAPSHOT_CACHE_TTL {
                        resolved.insert(symbol.clone(), (snapshot.clone(), CacheStatus::Fresh));
                        continue;
                    }
                }
                missing.push(symbol.clone());
            }
        }

        if !missing.is_empty() {
            let joined = missing.join(",");
            let payload: HashMap<String, ProviderSnapshot> = self.request_json(
                "v2/stocks/snapshots",
                &[
                    ("symbols", joined.as_str()),
                    ("feed", self.config.feed.as_str()),
                ],
            )?;
            let mut cache = self
                .snapshot_cache
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            for symbol in missing {
                let snapshot =
                    payload
                        .get(&symbol)
                        .cloned()
                        .ok_or_else(|| MarketDataError::Provider {
                            provider: self.provider_id(),
                            message: format!("snapshot omitted {symbol}"),
                            retriable: false,
                        })?;
                cache.insert(symbol.clone(), (Instant::now(), snapshot.clone()));
                resolved.insert(symbol, (snapshot, CacheStatus::Live));
            }
        }
        Ok(resolved)
    }

    fn bars(
        &self,
        symbol: &str,
        timeframe: &str,
        start: &str,
        end: &str,
    ) -> Result<Vec<ProviderBar>, MarketDataError> {
        let symbol = normalize_symbol(symbol)?;
        let path = format!("v2/stocks/{symbol}/bars");
        let payload: ProviderBars = self.request_json(
            &path,
            &[
                ("timeframe", timeframe),
                ("start", start),
                ("end", end),
                ("limit", "10000"),
                ("sort", "desc"),
                ("adjustment", "split"),
                ("feed", self.config.feed.as_str()),
            ],
        )?;
        if payload.next_page_token.is_some() {
            return Err(MarketDataError::Unsupported(format!(
                "Alpaca history exceeded the bounded {MAX_BARS}-bar page; narrow the range"
            )));
        }
        let mut bars = payload
            .bars
            .unwrap_or_default()
            .into_iter()
            .filter_map(|mut bar| bar.is_usable().then_some(bar))
            .collect::<Vec<_>>();
        bars.sort_by(|left, right| left.timestamp.cmp(&right.timestamp));
        Ok(bars)
    }

    fn request_json<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<T, MarketDataError> {
        if self.config.key_id.trim().is_empty() || self.config.secret_key.trim().is_empty() {
            return Err(MarketDataError::PermissionDenied(
                "Alpaca requires APCA_API_KEY_ID and APCA_API_SECRET_KEY".to_owned(),
            ));
        }
        let url =
            self.config.base_url.join(path).map_err(|_| {
                MarketDataError::InvalidRequest("invalid Alpaca API path".to_owned())
            })?;
        let response = self
            .client
            .get(url)
            .header("APCA-API-KEY-ID", self.config.key_id.as_ref())
            .header("APCA-API-SECRET-KEY", self.config.secret_key.as_ref())
            .query(query)
            .send()
            .map_err(|_| {
                MarketDataError::TemporarilyUnavailable(
                    "Alpaca transport request failed".to_owned(),
                )
            })?;
        let status = response.status();
        let retry_after_ms = response
            .headers()
            .get(header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(60)
            .saturating_mul(1_000);
        let bytes = response.bytes().map_err(|_| {
            MarketDataError::TemporarilyUnavailable("Alpaca response body failed".to_owned())
        })?;
        if bytes.len() > MAX_RESPONSE_BYTES {
            return Err(MarketDataError::Provider {
                provider: self.provider_id(),
                message: "response exceeded 8 MiB limit".to_owned(),
                retriable: false,
            });
        }
        if !status.is_success() {
            return Err(self.response_error(status, &bytes, retry_after_ms));
        }
        serde_json::from_slice(&bytes).map_err(|_| MarketDataError::Provider {
            provider: self.provider_id(),
            message: "response was not valid JSON".to_owned(),
            retriable: false,
        })
    }

    fn response_error(
        &self,
        status: StatusCode,
        body: &[u8],
        retry_after_ms: u64,
    ) -> MarketDataError {
        if status == StatusCode::TOO_MANY_REQUESTS {
            return MarketDataError::RateLimited { retry_after_ms };
        }
        let message = serde_json::from_slice::<ProviderError>(body)
            .ok()
            .and_then(|error| error.message)
            .unwrap_or_else(|| format!("HTTP {}", status.as_u16()));
        let message = bound_message(&message);
        if matches!(status.as_u16(), 401 | 403 | 422) {
            return MarketDataError::PermissionDenied(if message.contains("subscription") {
                format!("{message}; free accounts should use ALPACA_FEED=iex")
            } else {
                message
            });
        }
        MarketDataError::Provider {
            provider: self.provider_id(),
            message,
            retriable: status.is_server_error(),
        }
    }
}

impl MarketDataQuery for AlpacaMarketData {
    fn quote_snapshots(
        &self,
        instruments: &[CanonicalInstrumentId],
    ) -> Result<Vec<QuoteSnapshot>, MarketDataError> {
        let symbols = instruments
            .iter()
            .map(symbol_from_canonical)
            .map(|symbol| normalize_symbol(&symbol))
            .collect::<Result<Vec<_>, _>>()?;
        let snapshots = self.snapshots(&symbols)?;
        instruments
            .iter()
            .zip(symbols)
            .map(|(instrument_id, symbol)| {
                let (snapshot, cache_status) =
                    snapshots
                        .get(&symbol)
                        .ok_or_else(|| MarketDataError::Provider {
                            provider: self.provider_id(),
                            message: format!("snapshot omitted {symbol}"),
                            retriable: false,
                        })?;
                quote_snapshot(
                    instrument_id.clone(),
                    &symbol,
                    snapshot,
                    self.provider_id(),
                    *cache_status,
                )
            })
            .collect()
    }

    fn price_history(&self, request: &HistoryRequest) -> Result<Vec<PriceBar>, MarketDataError> {
        let symbol = symbol_from_canonical(&request.instrument_id);
        let bars = self.bars(
            &symbol,
            timeframe(request.interval),
            request.start.as_str(),
            request.end.as_str(),
        )?;
        bars.into_iter()
            .map(|bar| market_price_bar(request.instrument_id.clone(), request.interval, bar))
            .collect()
    }
}

impl ChartHistoryQuery for AlpacaMarketData {
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
        let (start, end, timeframe) = chart_range(request.period);
        let mut bars = self
            .bars(symbol, timeframe, &start, &end)
            .map_err(chart_history_error)?;
        let count = request.period.sample_count();
        if bars.len() > count {
            bars.drain(..bars.len() - count);
        }
        if bars.is_empty() {
            return Err(ChartHistoryError::Unavailable(format!(
                "Alpaca returned no observations for {symbol}"
            )));
        }
        let bars = bars
            .into_iter()
            .map(|bar| ChartPriceBar {
                timestamp: chrono::DateTime::parse_from_rfc3339(&bar.timestamp)
                    .expect("usable bar timestamp was validated")
                    .timestamp(),
                open: bar.open.expect("usable bar has open or close"),
                high: bar.high.expect("usable bar has high or close"),
                low: bar.low.expect("usable bar has low or close"),
                close: bar.close.expect("usable bar has close"),
                volume: bar.volume.unwrap_or_default(),
            })
            .collect();
        Ok(HistorySeries {
            instrument: request.instrument.clone(),
            bars,
            quality: HistoryQuality::Live,
            source: self.source_label(),
        })
    }
}

impl SpreadsheetMarketData for AlpacaMarketData {
    fn load_batch(&self, requests: &[MarketDataRequest]) -> Vec<MarketDataPoint> {
        let mut symbols = Vec::new();
        for request in requests {
            let symbol = request
                .security
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .to_ascii_uppercase();
            if !symbols.contains(&symbol) {
                symbols.push(symbol);
            }
        }
        let canonical = symbols
            .iter()
            .map(|symbol| {
                CanonicalInstrumentId::new(format!("us:listed:{}", symbol.to_lowercase()))
            })
            .collect::<Vec<_>>();
        let quotes = self.quote_snapshots(&canonical).map(|snapshots| {
            snapshots
                .into_iter()
                .map(|snapshot| (snapshot.symbol.clone(), snapshot))
                .collect::<HashMap<_, _>>()
        });
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
                let state = match &quotes {
                    Ok(quotes) => match quotes.get(&symbol) {
                        Some(snapshot) => {
                            let value = match request.field.as_str() {
                                "PX_LAST" => snapshot.last.map(Price::value),
                                "CHG_PCT_1D" => {
                                    snapshot.change.map(|change| change.percent.value())
                                }
                                _ => None,
                            };
                            value.map_or_else(
                                || MarketDataState::Unavailable {
                                    reason: format!("unsupported field {}", request.field),
                                },
                                |value| MarketDataState::Ready {
                                    value,
                                    provenance: MarketDataProvenance {
                                        provider: self.source_label(),
                                        observed_at: snapshot.as_of.as_str().to_owned(),
                                        received_at: received_at.clone(),
                                        quality: MarketDataQuality::Realtime,
                                    },
                                },
                            )
                        }
                        None => MarketDataState::Unavailable {
                            reason: format!("Alpaca snapshot omitted {symbol}"),
                        },
                    },
                    Err(MarketDataError::PermissionDenied(_)) => {
                        MarketDataState::PermissionDenied {
                            provider: "ALPACA".to_owned(),
                        }
                    }
                    Err(error) => MarketDataState::Unavailable {
                        reason: error.to_string(),
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

fn chart_range(period: ChartPeriod) -> (String, String, &'static str) {
    let end = Utc::now();
    let start = match period {
        ChartPeriod::OneDay => end - chrono::Duration::days(7),
        ChartPeriod::OneMonth => end - chrono::Duration::days(45),
        ChartPeriod::SixMonths => end - chrono::Duration::days(220),
        ChartPeriod::YearToDate => Utc
            .with_ymd_and_hms(end.year(), 1, 1, 0, 0, 0)
            .single()
            .expect("January first is valid"),
        ChartPeriod::OneYear => end - chrono::Duration::days(400),
        ChartPeriod::FiveYears => end - chrono::Duration::days(365 * 6),
    };
    let timeframe = match period {
        ChartPeriod::OneDay => "5Min",
        ChartPeriod::FiveYears => "1Week",
        _ => "1Day",
    };
    (start.to_rfc3339(), end.to_rfc3339(), timeframe)
}

fn timeframe(interval: BarInterval) -> &'static str {
    match interval {
        BarInterval::OneMinute => "1Min",
        BarInterval::FiveMinutes => "5Min",
        BarInterval::OneHour => "1Hour",
        BarInterval::OneDay => "1Day",
        BarInterval::OneWeek => "1Week",
    }
}

fn quote_snapshot(
    instrument_id: CanonicalInstrumentId,
    symbol: &str,
    snapshot: &ProviderSnapshot,
    provider: ProviderId,
    cache_status: CacheStatus,
) -> Result<QuoteSnapshot, MarketDataError> {
    let last = snapshot
        .latest_trade
        .as_ref()
        .and_then(|trade| trade.price)
        .or_else(|| snapshot.minute_bar.as_ref().and_then(|bar| bar.close))
        .or_else(|| snapshot.daily_bar.as_ref().and_then(|bar| bar.close))
        .filter(|value| value.is_finite())
        .ok_or_else(|| MarketDataError::Provider {
            provider: provider.clone(),
            message: format!("snapshot contained no usable price for {symbol}"),
            retriable: false,
        })?;
    let previous_close = snapshot
        .previous_daily_bar
        .as_ref()
        .and_then(|bar| bar.close)
        .filter(|value| value.is_finite());
    let change = previous_close
        .filter(|value| value.abs() >= f64::EPSILON)
        .map(|previous| PriceChange {
            absolute: Price::new(last - previous),
            percent: Percent::new(((last / previous) - 1.0) * 100.0),
        });
    let received_at = Utc::now().to_rfc3339();
    let source_timestamp = snapshot
        .latest_trade
        .as_ref()
        .and_then(|trade| trade.timestamp.as_deref())
        .or_else(|| {
            snapshot
                .latest_quote
                .as_ref()
                .and_then(|quote| quote.timestamp.as_deref())
        })
        .or_else(|| {
            snapshot
                .minute_bar
                .as_ref()
                .and_then(|bar| (!bar.timestamp.is_empty()).then_some(bar.timestamp.as_str()))
        })
        .or_else(|| {
            snapshot
                .daily_bar
                .as_ref()
                .and_then(|bar| (!bar.timestamp.is_empty()).then_some(bar.timestamp.as_str()))
        })
        .unwrap_or(received_at.as_str());
    let source_timestamp = UtcTimestamp::new(source_timestamp);
    Ok(QuoteSnapshot {
        instrument_id,
        symbol: symbol.to_owned(),
        currency: "USD".to_owned(),
        last: Some(Price::new(last)),
        change,
        bid: snapshot
            .latest_quote
            .as_ref()
            .and_then(|quote| quote.bid)
            .filter(|value| value.is_finite())
            .map(Price::new),
        ask: snapshot
            .latest_quote
            .as_ref()
            .and_then(|quote| quote.ask)
            .filter(|value| value.is_finite())
            .map(Price::new),
        day_low: snapshot
            .daily_bar
            .as_ref()
            .and_then(|bar| bar.low)
            .filter(|value| value.is_finite())
            .map(Price::new),
        day_high: snapshot
            .daily_bar
            .as_ref()
            .and_then(|bar| bar.high)
            .filter(|value| value.is_finite())
            .map(Price::new),
        volume: snapshot
            .daily_bar
            .as_ref()
            .and_then(|bar| bar.volume)
            .map(Quantity::new),
        as_of: source_timestamp.clone(),
        quality: DataQuality::RealTime,
        provenance: DataProvenance {
            provider,
            source_timestamp,
            received_at: UtcTimestamp::new(received_at),
            sequence: None,
            cache_status,
        },
    })
}

fn market_price_bar(
    instrument_id: CanonicalInstrumentId,
    interval: BarInterval,
    bar: ProviderBar,
) -> Result<PriceBar, MarketDataError> {
    Ok(PriceBar {
        instrument_id,
        interval,
        opened_at: UtcTimestamp::new(bar.timestamp),
        open: Price::new(bar.open.expect("usable bar has open or close")),
        high: Price::new(bar.high.expect("usable bar has high or close")),
        low: Price::new(bar.low.expect("usable bar has low or close")),
        close: Price::new(bar.close.expect("usable bar has close")),
        volume: Quantity::new(bar.volume.unwrap_or_default()),
        quality: DataQuality::RealTime,
    })
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
            "invalid provider symbol".to_owned(),
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

fn bound_message(message: &str) -> String {
    message.chars().take(240).collect()
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct ProviderSnapshot {
    latest_trade: Option<ProviderTrade>,
    latest_quote: Option<ProviderQuote>,
    minute_bar: Option<ProviderBar>,
    daily_bar: Option<ProviderBar>,
    #[serde(alias = "prevDailyBar")]
    previous_daily_bar: Option<ProviderBar>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct ProviderTrade {
    #[serde(rename = "t")]
    timestamp: Option<String>,
    #[serde(rename = "p")]
    price: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct ProviderQuote {
    #[serde(rename = "t")]
    timestamp: Option<String>,
    #[serde(rename = "bp")]
    bid: Option<f64>,
    #[serde(rename = "ap")]
    ask: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct ProviderBar {
    #[serde(rename = "t")]
    timestamp: String,
    #[serde(rename = "o")]
    open: Option<f64>,
    #[serde(rename = "h")]
    high: Option<f64>,
    #[serde(rename = "l")]
    low: Option<f64>,
    #[serde(rename = "c")]
    close: Option<f64>,
    #[serde(rename = "v")]
    volume: Option<u64>,
}

impl ProviderBar {
    fn is_usable(&mut self) -> bool {
        if chrono::DateTime::parse_from_rfc3339(&self.timestamp).is_err() {
            return false;
        }
        let Some(close) = self.close.filter(|value| value.is_finite()) else {
            return false;
        };
        self.open = self.open.filter(|value| value.is_finite()).or(Some(close));
        self.high = self.high.filter(|value| value.is_finite()).or(Some(close));
        self.low = self.low.filter(|value| value.is_finite()).or(Some(close));
        true
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ProviderBars {
    bars: Option<Vec<ProviderBar>>,
    next_page_token: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ProviderError {
    message: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const SNAPSHOT: &str = r#"{
      "latestTrade": {"t": "2026-07-10T15:59:58Z", "p": 214.53},
      "latestQuote": {"t": "2026-07-10T15:59:59Z", "bp": 214.5, "ap": 214.55},
      "minuteBar": {"t": "2026-07-10T15:59:00Z", "o": 214.4, "h": 214.6, "l": 214.3, "c": 214.5, "v": 12000},
      "dailyBar": {"t": "2026-07-10T04:00:00Z", "o": 212.0, "h": 215.0, "l": 211.5, "c": 214.5, "v": 5000000},
      "prevDailyBar": {"t": "2026-07-09T04:00:00Z", "o": 210.0, "h": 212.5, "l": 209.0, "c": 210.4, "v": 4800000}
    }"#;

    #[test]
    fn snapshot_maps_trade_quote_change_and_provenance() {
        let snapshot: ProviderSnapshot = serde_json::from_str(SNAPSHOT).unwrap();
        let quote = quote_snapshot(
            CanonicalInstrumentId::new("us:listed:aapl"),
            "AAPL",
            &snapshot,
            ProviderId::new("alpaca-iex"),
            CacheStatus::Live,
        )
        .unwrap();

        assert_eq!(quote.last.map(Price::value), Some(214.53));
        assert_eq!(quote.bid.map(Price::value), Some(214.5));
        assert_eq!(quote.ask.map(Price::value), Some(214.55));
        assert_eq!(
            quote
                .day_range()
                .map(|(low, high)| (low.value(), high.value())),
            Some((211.5, 215.0))
        );
        assert_eq!(quote.volume.map(Quantity::value), Some(5_000_000));
        assert!(quote
            .change
            .is_some_and(|change| change.percent.value() > 1.9));
        assert_eq!(quote.quality, DataQuality::RealTime);
        assert_eq!(quote.provenance.provider.as_str(), "alpaca-iex");
        assert_eq!(quote.as_of.as_str(), "2026-07-10T15:59:58Z");
    }

    #[test]
    fn snapshot_falls_back_to_bar_close_for_thin_symbols() {
        let snapshot: ProviderSnapshot =
            serde_json::from_str(r#"{"dailyBar":{"t":"2026-07-10T04:00:00Z","c":9.8}}"#).unwrap();
        let quote = quote_snapshot(
            CanonicalInstrumentId::new("us:listed:thin"),
            "THIN",
            &snapshot,
            ProviderId::new("alpaca-iex"),
            CacheStatus::Live,
        )
        .unwrap();
        assert_eq!(quote.last.map(Price::value), Some(9.8));
        assert!(quote.change.is_none());
    }

    #[test]
    fn bars_validate_fields_and_sort_oldest_first() {
        let mut bars: ProviderBars = serde_json::from_str(
            r#"{
              "bars":[
                {"t":"2026-07-10T15:55:00Z","o":214.4,"h":214.6,"l":214.3,"c":214.5,"v":900},
                {"t":"2026-07-10T15:50:00Z","c":214.4,"v":800},
                {"t":"not-a-date","c":214.1}
              ],
              "next_page_token":null
            }"#,
        )
        .unwrap();
        let mut bars = bars
            .bars
            .take()
            .unwrap()
            .into_iter()
            .filter_map(|mut bar| bar.is_usable().then_some(bar))
            .collect::<Vec<_>>();
        bars.sort_by(|left, right| left.timestamp.cmp(&right.timestamp));
        assert_eq!(bars.len(), 2);
        assert_eq!(bars[0].timestamp, "2026-07-10T15:50:00Z");
        assert_eq!(bars[0].open, Some(214.4));
    }

    #[test]
    fn symbols_and_timeframes_are_provider_safe() {
        assert!(normalize_symbol("BRK-B").is_ok());
        assert!(normalize_symbol("IBM&feed=sip").is_err());
        assert_eq!(timeframe(BarInterval::FiveMinutes), "5Min");
        assert_eq!(AlpacaConfig::fixture().feed, AlpacaFeed::Iex);
    }

    #[test]
    fn missing_credentials_are_a_typed_permission_failure() {
        let mut config = AlpacaConfig::fixture();
        config.key_id = Arc::from("");
        let adapter = AlpacaMarketData::new(config);
        let error = adapter
            .quote_snapshots(&[CanonicalInstrumentId::new("us:listed:aapl")])
            .unwrap_err();
        assert!(matches!(error, MarketDataError::PermissionDenied(_)));
    }

    #[test]
    fn response_statuses_preserve_entitlement_and_retry_types() {
        let adapter = AlpacaMarketData::new(AlpacaConfig::fixture());
        let entitlement = adapter.response_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            br#"{"message":"subscription does not permit querying recent SIP data"}"#,
            60_000,
        );
        assert!(matches!(entitlement, MarketDataError::PermissionDenied(_)));
        assert!(entitlement.to_string().contains("ALPACA_FEED=iex"));
        assert_eq!(
            adapter.response_error(StatusCode::TOO_MANY_REQUESTS, b"", 12_000),
            MarketDataError::RateLimited {
                retry_after_ms: 12_000
            }
        );
    }

    #[test]
    #[ignore = "requires configured Alpaca market-data credentials"]
    fn live_alpaca_quote_and_history_contract() {
        if env::var("APCA_API_KEY_ID").unwrap_or_default().is_empty()
            || env::var("APCA_API_SECRET_KEY")
                .unwrap_or_default()
                .is_empty()
        {
            eprintln!("skipping: Alpaca credentials are not configured");
            return;
        }
        let adapter = AlpacaMarketData::from_env();
        let quote = adapter
            .quote_snapshots(&[CanonicalInstrumentId::new("us:listed:aapl")])
            .expect("live Alpaca quote");
        assert_eq!(quote.len(), 1);
        assert!(quote[0].last.is_some_and(|price| price.value() > 0.0));
        let history = adapter
            .load_history(&ChartHistoryRequest::new(
                crate::features::charting::ChartInstrument::from_terminal_subject("AAPL"),
                ChartPeriod::OneMonth,
            ))
            .expect("live Alpaca history");
        assert!(!history.bars.is_empty());
        assert!(history
            .bars
            .windows(2)
            .all(|bars| bars[0].timestamp < bars[1].timestamp));
    }
}
