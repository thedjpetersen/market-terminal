//! Unauthenticated Yahoo Finance chart adapter.
//!
//! The response mapping, daily previous-close fallback, and null-bar handling
//! adapt `makeev/alphai-tui` at commit
//! `9143d2e1176d0a67a9f26960427cf370187fc2e6`.
//! Copyright (c) 2026 Mikhail Makeev, used under the MIT License. See
//! `THIRD_PARTY_NOTICES.md` at the repository root.

use std::{
    collections::HashMap,
    env,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use reqwest::{blocking::Client, StatusCode, Url};
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

const DEFAULT_BASE_URL: &str = "https://query1.finance.yahoo.com/v8/finance/chart/";
const DEFAULT_TIMEOUT_SECS: u64 = 12;
const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
const CACHE_TTL: Duration = Duration::from_secs(60);
const PROVIDER_ID: &str = "yahoo-finance-chart";

type ChartCache = Arc<Mutex<HashMap<String, (Instant, ChartResult)>>>;

#[derive(Clone)]
pub struct YahooConfig {
    base_url: Url,
    timeout: Duration,
}

impl YahooConfig {
    pub fn from_env() -> Self {
        let mut base_url = env::var("MARKET_TERMINAL_YAHOO_BASE_URL")
            .ok()
            .and_then(|value| Url::parse(&value).ok())
            .filter(|url| url.scheme() == "https")
            .unwrap_or_else(|| Url::parse(DEFAULT_BASE_URL).expect("default URL is valid"));
        if !base_url.path().ends_with('/') {
            let path = format!("{}/", base_url.path());
            base_url.set_path(&path);
        }
        let timeout = env::var("MARKET_TERMINAL_YAHOO_TIMEOUT_SECS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .map(|seconds| seconds.clamp(3, 60))
            .unwrap_or(DEFAULT_TIMEOUT_SECS);
        Self {
            base_url,
            timeout: Duration::from_secs(timeout),
        }
    }

    #[cfg(test)]
    fn fixture() -> Self {
        Self {
            base_url: Url::parse(DEFAULT_BASE_URL).expect("default URL is valid"),
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
        }
    }
}

#[derive(Clone)]
pub struct YahooMarketData {
    config: YahooConfig,
    client: Client,
    cache: ChartCache,
}

impl YahooMarketData {
    pub fn new(config: YahooConfig) -> Self {
        let client = Client::builder()
            .timeout(config.timeout)
            .user_agent(concat!("market-terminal/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("Yahoo Finance HTTP client should build");
        Self {
            config,
            client,
            cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn from_env() -> Self {
        Self::new(YahooConfig::from_env())
    }

    fn range_chart(
        &self,
        symbol: &str,
        range: &str,
        interval: &str,
    ) -> Result<(ChartResult, CacheStatus), MarketDataError> {
        self.chart(
            symbol,
            &[
                ("range", range.to_owned()),
                ("interval", interval.to_owned()),
            ],
        )
    }

    fn period_chart(
        &self,
        symbol: &str,
        period1: i64,
        period2: i64,
        interval: &str,
    ) -> Result<(ChartResult, CacheStatus), MarketDataError> {
        self.chart(
            symbol,
            &[
                ("period1", period1.to_string()),
                ("period2", period2.to_string()),
                ("interval", interval.to_owned()),
            ],
        )
    }

    fn chart(
        &self,
        symbol: &str,
        query: &[(&str, String)],
    ) -> Result<(ChartResult, CacheStatus), MarketDataError> {
        let symbol = normalize_symbol(symbol)?;
        let cache_key = format!(
            "{}?{}",
            symbol,
            query
                .iter()
                .map(|(name, value)| format!("{name}={value}"))
                .collect::<Vec<_>>()
                .join("&")
        );
        {
            let cache = self.cache.lock().unwrap_or_else(|error| error.into_inner());
            if let Some((stored_at, result)) = cache.get(&cache_key) {
                if stored_at.elapsed() <= CACHE_TTL {
                    return Ok((result.clone(), CacheStatus::Fresh));
                }
            }
        }
        let url = self.config.base_url.join(&symbol).map_err(|_| {
            MarketDataError::InvalidRequest("invalid Yahoo Finance symbol URL".to_owned())
        })?;
        let response = self.client.get(url).query(query).send().map_err(|_| {
            MarketDataError::TemporarilyUnavailable("Yahoo Finance chart request failed".to_owned())
        })?;
        let status = response.status();
        let bytes = response.bytes().map_err(|_| {
            MarketDataError::TemporarilyUnavailable("Yahoo Finance response body failed".to_owned())
        })?;
        if bytes.len() > MAX_RESPONSE_BYTES {
            return Err(provider_error("response exceeded 8 MiB limit", false));
        }
        if status == StatusCode::TOO_MANY_REQUESTS {
            return Err(MarketDataError::RateLimited {
                retry_after_ms: 60_000,
            });
        }
        if !status.is_success() {
            return Err(provider_error(
                &format!("HTTP {}", status.as_u16()),
                status.is_server_error(),
            ));
        }
        let response: ChartResponse = serde_json::from_slice(&bytes)
            .map_err(|_| provider_error("response was not valid JSON", false))?;
        if let Some(error) = response.chart.error {
            let message = error
                .description
                .or(error.code)
                .unwrap_or_else(|| "chart request failed".to_owned());
            return Err(provider_error(&message, false));
        }
        let result = response
            .chart
            .result
            .and_then(|mut values| (!values.is_empty()).then(|| values.remove(0)))
            .ok_or_else(|| provider_error("empty chart result", false))?;
        self.cache
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(cache_key, (Instant::now(), result.clone()));
        Ok((result, CacheStatus::Live))
    }
}

impl MarketDataQuery for YahooMarketData {
    fn quote_snapshots(
        &self,
        instruments: &[CanonicalInstrumentId],
    ) -> Result<Vec<QuoteSnapshot>, MarketDataError> {
        instruments
            .iter()
            .map(|instrument_id| {
                let symbol = symbol_from_canonical(instrument_id);
                let (result, cache_status) = self.range_chart(&symbol, "5d", "1d")?;
                quote_snapshot(instrument_id.clone(), &symbol, &result, cache_status)
            })
            .collect()
    }

    fn price_history(&self, request: &HistoryRequest) -> Result<Vec<PriceBar>, MarketDataError> {
        let symbol = symbol_from_canonical(&request.instrument_id);
        let period1 = timestamp_from_input(&request.start, false)?;
        let period2 = timestamp_from_input(&request.end, true)?;
        if period1 >= period2 {
            return Err(MarketDataError::InvalidRequest(
                "history start must precede end".to_owned(),
            ));
        }
        let (result, _) =
            self.period_chart(&symbol, period1, period2, market_interval(request.interval))?;
        Ok(build_bars(&result)
            .into_iter()
            .map(|bar| PriceBar {
                instrument_id: request.instrument_id.clone(),
                interval: request.interval,
                opened_at: timestamp(bar.timestamp),
                open: Price::new(bar.open),
                high: Price::new(bar.high),
                low: Price::new(bar.low),
                close: Price::new(bar.close),
                volume: Quantity::new(bar.volume.unwrap_or_default()),
                quality: DataQuality::Delayed { minutes: 15 },
            })
            .collect())
    }
}

impl ChartHistoryQuery for YahooMarketData {
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
        let (range, interval) = chart_parameters(request.period);
        let (result, _) = self
            .range_chart(symbol, range, interval)
            .map_err(chart_history_error)?;
        let mut bars = build_bars(&result);
        let count = request.period.sample_count();
        if bars.len() > count {
            bars.drain(..bars.len() - count);
        }
        if bars.is_empty() {
            return Err(ChartHistoryError::Unavailable(format!(
                "Yahoo Finance returned no observations for {symbol}"
            )));
        }
        Ok(HistorySeries {
            instrument: request.instrument.clone(),
            bars: bars
                .into_iter()
                .map(|bar| ChartPriceBar {
                    timestamp: bar.timestamp,
                    open: bar.open,
                    high: bar.high,
                    low: bar.low,
                    close: bar.close,
                    volume: bar.volume.unwrap_or_default(),
                })
                .collect(),
            quality: HistoryQuality::Delayed,
            source: "YAHOO FINANCE CHART · DELAYED · UNOFFICIAL ENDPOINT".to_owned(),
        })
    }
}

impl SpreadsheetMarketData for YahooMarketData {
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
                CanonicalInstrumentId::new(format!("us:listed:{}", symbol.to_ascii_lowercase()))
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
                                        provider: "YAHOO FINANCE CHART · UNOFFICIAL".to_owned(),
                                        observed_at: snapshot.as_of.as_str().to_owned(),
                                        received_at: received_at.clone(),
                                        quality: MarketDataQuality::Delayed,
                                    },
                                },
                            )
                        }
                        None => MarketDataState::Unavailable {
                            reason: format!("Yahoo Finance response omitted {symbol}"),
                        },
                    },
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

fn quote_snapshot(
    instrument_id: CanonicalInstrumentId,
    requested_symbol: &str,
    result: &ChartResult,
    cache_status: CacheStatus,
) -> Result<QuoteSnapshot, MarketDataError> {
    let bars = build_bars(result);
    let last_close = bars.last().map(|bar| bar.close);
    let price = result
        .meta
        .regular_market_price
        .filter(|value| value.is_finite())
        .or(last_close)
        .ok_or_else(|| provider_error("chart result contained no usable price", false))?;
    let previous_close = result
        .meta
        .previous_close
        .filter(|value| value.is_finite())
        .or_else(|| (bars.len() >= 2).then(|| bars[bars.len().saturating_sub(2)].close))
        .or_else(|| {
            result
                .meta
                .chart_previous_close
                .filter(|value| value.is_finite())
        });
    let change = previous_close
        .filter(|previous| previous.abs() >= f64::EPSILON)
        .map(|previous| PriceChange {
            absolute: Price::new(price - previous),
            percent: Percent::new(((price / previous) - 1.0) * 100.0),
        });
    let source_time = result
        .meta
        .regular_market_time
        .or_else(|| bars.last().map(|bar| bar.timestamp))
        .unwrap_or_else(|| Utc::now().timestamp());
    let source_timestamp = timestamp(source_time);
    let received_at = UtcTimestamp::new(Utc::now().to_rfc3339());
    Ok(QuoteSnapshot {
        instrument_id,
        symbol: result
            .meta
            .symbol
            .clone()
            .filter(|symbol| !symbol.trim().is_empty())
            .unwrap_or_else(|| requested_symbol.to_owned()),
        currency: result
            .meta
            .currency
            .clone()
            .filter(|currency| !currency.trim().is_empty())
            .unwrap_or_else(|| "USD".to_owned()),
        last: Some(Price::new(price)),
        change,
        bid: None,
        ask: None,
        day_low: finite_price(result.meta.regular_market_day_low),
        day_high: finite_price(result.meta.regular_market_day_high),
        volume: result.meta.regular_market_volume.map(Quantity::new),
        as_of: source_timestamp.clone(),
        quality: DataQuality::Delayed { minutes: 15 },
        provenance: DataProvenance {
            provider: ProviderId::new(PROVIDER_ID),
            source_timestamp,
            received_at,
            sequence: None,
            cache_status,
        },
    })
}

fn build_bars(result: &ChartResult) -> Vec<ProviderBar> {
    let Some(timestamps) = result.timestamp.as_ref() else {
        return Vec::new();
    };
    let Some(quote) = result.indicators.quote.first() else {
        return Vec::new();
    };
    let number = |values: &Option<Vec<Option<f64>>>, index: usize| {
        values
            .as_ref()
            .and_then(|values| values.get(index))
            .copied()
            .flatten()
            .filter(|value| value.is_finite())
    };
    let volume = |index: usize| {
        quote
            .volume
            .as_ref()
            .and_then(|values| values.get(index))
            .copied()
            .flatten()
    };
    timestamps
        .iter()
        .enumerate()
        .filter_map(|(index, timestamp)| {
            let close = number(&quote.close, index)?;
            let open = number(&quote.open, index).unwrap_or(close);
            let high = number(&quote.high, index).unwrap_or(close);
            let low = number(&quote.low, index).unwrap_or(close);
            (high >= open.max(close) && low <= open.min(close) && low <= high).then_some(
                ProviderBar {
                    timestamp: *timestamp,
                    open,
                    high,
                    low,
                    close,
                    volume: volume(index),
                },
            )
        })
        .collect()
}

fn timestamp_from_input(value: &UtcTimestamp, inclusive_end: bool) -> Result<i64, MarketDataError> {
    if let Ok(value) = DateTime::parse_from_rfc3339(value.as_str()) {
        return Ok(value.timestamp());
    }
    let date_text = value.as_str().get(..10).unwrap_or(value.as_str());
    let date = NaiveDate::parse_from_str(date_text, "%Y-%m-%d").map_err(|_| {
        MarketDataError::InvalidRequest("history timestamp was not ISO-8601".to_owned())
    })?;
    let date = if inclusive_end {
        date.succ_opt().ok_or_else(|| {
            MarketDataError::InvalidRequest("history end date overflowed".to_owned())
        })?
    } else {
        date
    };
    Ok(date
        .and_hms_opt(0, 0, 0)
        .expect("midnight is valid")
        .and_utc()
        .timestamp())
}

fn timestamp(value: i64) -> UtcTimestamp {
    UtcTimestamp::new(
        Utc.timestamp_opt(value, 0)
            .single()
            .unwrap_or_else(Utc::now)
            .to_rfc3339(),
    )
}

fn finite_price(value: Option<f64>) -> Option<Price> {
    value.filter(|value| value.is_finite()).map(Price::new)
}

fn chart_parameters(period: ChartPeriod) -> (&'static str, &'static str) {
    match period {
        ChartPeriod::OneDay => ("1d", "5m"),
        ChartPeriod::OneMonth => ("1mo", "1d"),
        ChartPeriod::SixMonths => ("6mo", "1d"),
        ChartPeriod::YearToDate => ("ytd", "1d"),
        ChartPeriod::OneYear => ("1y", "1d"),
        ChartPeriod::FiveYears => ("5y", "1wk"),
    }
}

fn market_interval(interval: BarInterval) -> &'static str {
    match interval {
        BarInterval::OneMinute => "1m",
        BarInterval::FiveMinutes => "5m",
        BarInterval::OneHour => "60m",
        BarInterval::OneDay => "1d",
        BarInterval::OneWeek => "1wk",
    }
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
            "invalid Yahoo Finance symbol".to_owned(),
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
struct ChartResponse {
    chart: Chart,
}

#[derive(Debug, Clone, Deserialize)]
struct Chart {
    result: Option<Vec<ChartResult>>,
    error: Option<ApiError>,
}

#[derive(Debug, Clone, Deserialize)]
struct ApiError {
    code: Option<String>,
    description: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ChartResult {
    meta: Meta,
    timestamp: Option<Vec<i64>>,
    indicators: Indicators,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Meta {
    symbol: Option<String>,
    currency: Option<String>,
    regular_market_price: Option<f64>,
    previous_close: Option<f64>,
    chart_previous_close: Option<f64>,
    regular_market_time: Option<i64>,
    regular_market_day_low: Option<f64>,
    regular_market_day_high: Option<f64>,
    regular_market_volume: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
struct Indicators {
    #[serde(default)]
    quote: Vec<QuoteBlock>,
}

#[derive(Debug, Clone, Deserialize)]
struct QuoteBlock {
    open: Option<Vec<Option<f64>>>,
    high: Option<Vec<Option<f64>>>,
    low: Option<Vec<Option<f64>>>,
    close: Option<Vec<Option<f64>>>,
    volume: Option<Vec<Option<u64>>>,
}

#[derive(Debug, Clone)]
struct ProviderBar {
    timestamp: i64,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"{
      "chart": {"result": [{
        "meta": {
          "currency": "USD", "symbol": "AAPL", "regularMarketPrice": 212.5,
          "previousClose": 210.0, "regularMarketTime": 1783526400,
          "regularMarketDayLow": 209.5, "regularMarketDayHigh": 213.0,
          "regularMarketVolume": 1000000
        },
        "timestamp": [1783440000, 1783526400],
        "indicators": {"quote": [{
          "open": [208.0, 211.0], "high": [211.0, 213.0],
          "low": [207.0, 209.5], "close": [210.0, 212.5],
          "volume": [900000, 1000000]
        }]}
      }], "error": null}
    }"#;

    #[test]
    fn chart_payload_maps_quote_bars_and_delayed_provenance() {
        let response: ChartResponse = serde_json::from_str(FIXTURE).unwrap();
        let result = &response.chart.result.unwrap()[0];
        let snapshot = quote_snapshot(
            CanonicalInstrumentId::new("us:listed:aapl"),
            "AAPL",
            result,
            CacheStatus::Live,
        )
        .unwrap();

        assert_eq!(snapshot.last.map(Price::value), Some(212.5));
        assert_eq!(
            snapshot
                .day_range()
                .map(|(low, high)| (low.value(), high.value())),
            Some((209.5, 213.0))
        );
        assert_eq!(snapshot.quality, DataQuality::Delayed { minutes: 15 });
        assert_eq!(snapshot.provenance.provider.as_str(), PROVIDER_ID);
        assert_eq!(build_bars(result).len(), 2);
    }

    #[test]
    fn null_close_drops_a_bar_and_missing_ohl_falls_back_to_close() {
        let response: ChartResponse = serde_json::from_str(
            r#"{"chart":{"result":[{"meta":{},"timestamp":[1,2],"indicators":{"quote":[{"open":[null,null],"high":[null,null],"low":[null,null],"close":[null,2.0],"volume":[null,null]}]}}],"error":null}}"#,
        )
        .unwrap();
        let result = &response.chart.result.unwrap()[0];

        let bars = build_bars(result);

        assert_eq!(bars.len(), 1);
        assert_eq!((bars[0].open, bars[0].high, bars[0].low), (2.0, 2.0, 2.0));
    }

    #[test]
    fn rejects_symbols_that_could_mutate_provider_urls() {
        assert!(normalize_symbol("AAPL").is_ok());
        assert!(normalize_symbol("BRK-B").is_ok());
        assert!(normalize_symbol("AAPL?range=max").is_err());
    }

    #[test]
    #[ignore = "live Yahoo Finance chart contract test"]
    fn live_quote_and_history_flow_through_typed_ports() {
        let adapter = YahooMarketData::new(YahooConfig::fixture());
        let snapshot = adapter
            .quote_snapshots(&[CanonicalInstrumentId::new("us:listed:aapl")])
            .expect("live Yahoo quote")
            .remove(0);
        assert_eq!(snapshot.symbol, "AAPL");
        assert!(snapshot.last.is_some_and(|price| price.value() > 0.0));
        assert_eq!(snapshot.quality, DataQuality::Delayed { minutes: 15 });

        let history = adapter
            .load_history(&ChartHistoryRequest::new(
                crate::features::charting::ChartInstrument::from_terminal_subject("AAPL"),
                ChartPeriod::OneMonth,
            ))
            .expect("live Yahoo history");
        assert!(!history.bars.is_empty());
        assert!(history
            .bars
            .windows(2)
            .all(|bars| bars[0].timestamp < bars[1].timestamp));
    }
}
