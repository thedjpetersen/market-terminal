use std::{
    collections::HashMap,
    env,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use chrono::{NaiveDate, Utc};
use reqwest::{blocking::Client, Url};
use serde_json::Value;

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

const DEFAULT_BASE_URL: &str = "https://www.alphavantage.co/query";
const DEFAULT_TIMEOUT_SECS: u64 = 12;
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const QUOTE_CACHE_TTL: Duration = Duration::from_secs(60);
const PROVIDER_ID: &str = "alpha-vantage";

#[derive(Clone)]
pub struct AlphaVantageConfig {
    base_url: Url,
    api_key: Arc<str>,
    timeout: Duration,
    demo: bool,
}

impl AlphaVantageConfig {
    pub fn from_env() -> Self {
        let api_key = env::var("ALPHA_VANTAGE_API_KEY")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "demo".to_owned());
        let demo = api_key == "demo";
        let base_url = env::var("ALPHA_VANTAGE_BASE_URL")
            .ok()
            .and_then(|value| Url::parse(&value).ok())
            .filter(|url| url.scheme() == "https")
            .unwrap_or_else(|| Url::parse(DEFAULT_BASE_URL).expect("default URL is valid"));
        let timeout = env::var("ALPHA_VANTAGE_TIMEOUT_SECS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .map(|seconds| seconds.clamp(3, 60))
            .unwrap_or(DEFAULT_TIMEOUT_SECS);
        Self {
            base_url,
            api_key: Arc::from(api_key),
            timeout: Duration::from_secs(timeout),
            demo,
        }
    }

    #[cfg(test)]
    fn demo() -> Self {
        Self {
            base_url: Url::parse(DEFAULT_BASE_URL).expect("default URL is valid"),
            api_key: Arc::from("demo"),
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            demo: true,
        }
    }
}

#[derive(Clone)]
pub struct AlphaVantageMarketData {
    config: AlphaVantageConfig,
    client: Client,
    quote_cache: Arc<Mutex<HashMap<String, (Instant, ProviderQuote)>>>,
}

impl AlphaVantageMarketData {
    pub fn new(config: AlphaVantageConfig) -> Self {
        let client = Client::builder()
            .timeout(config.timeout)
            .user_agent(concat!("market-terminal/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("Alpha Vantage HTTP client should build");
        Self {
            config,
            client,
            quote_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn from_env() -> Self {
        Self::new(AlphaVantageConfig::from_env())
    }

    fn global_quote(&self, symbol: &str) -> Result<ProviderQuote, MarketDataError> {
        let symbol = normalize_symbol(symbol)?;
        if self.config.demo && symbol != "IBM" {
            return Err(MarketDataError::PermissionDenied(
                "Alpha Vantage demo access is limited to IBM; set ALPHA_VANTAGE_API_KEY".to_owned(),
            ));
        }
        let mut cache = self
            .quote_cache
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some((stored_at, quote)) = cache.get(&symbol) {
            if stored_at.elapsed() <= QUOTE_CACHE_TTL {
                return Ok(quote.clone());
            }
        }
        let payload = self.request(&[("function", "GLOBAL_QUOTE"), ("symbol", symbol.as_str())])?;
        let quote = payload
            .get("Global Quote")
            .and_then(Value::as_object)
            .ok_or_else(|| provider_payload_error(&payload))?;
        let quote = ProviderQuote {
            symbol: required_string(quote, "01. symbol")?,
            price: required_number(quote, "05. price")?,
            volume: optional_u64(quote, "06. volume"),
            trading_day: required_string(quote, "07. latest trading day")?,
            previous_close: required_number(quote, "08. previous close")?,
            change_percent: required_percent(quote, "10. change percent")?,
        };
        cache.insert(symbol, (Instant::now(), quote.clone()));
        Ok(quote)
    }

    fn daily_history(&self, symbol: &str) -> Result<Vec<ProviderBar>, MarketDataError> {
        let symbol = normalize_symbol(symbol)?;
        if self.config.demo {
            return Err(MarketDataError::PermissionDenied(
                "Alpha Vantage daily history requires ALPHA_VANTAGE_API_KEY".to_owned(),
            ));
        }
        let payload = self.request(&[
            ("function", "TIME_SERIES_DAILY"),
            ("symbol", symbol.as_str()),
            ("outputsize", "compact"),
        ])?;
        let series = payload
            .get("Time Series (Daily)")
            .and_then(Value::as_object)
            .ok_or_else(|| provider_payload_error(&payload))?;
        let mut bars = series
            .iter()
            .map(|(date, row)| {
                let row = row.as_object().ok_or_else(|| MarketDataError::Provider {
                    provider: ProviderId::new(PROVIDER_ID),
                    message: "daily history row was not an object".to_owned(),
                    retriable: false,
                })?;
                let date = NaiveDate::parse_from_str(date, "%Y-%m-%d").map_err(|_| {
                    MarketDataError::Provider {
                        provider: ProviderId::new(PROVIDER_ID),
                        message: "daily history contained an invalid date".to_owned(),
                        retriable: false,
                    }
                })?;
                Ok(ProviderBar {
                    date,
                    open: required_number(row, "1. open")?,
                    high: required_number(row, "2. high")?,
                    low: required_number(row, "3. low")?,
                    close: required_number(row, "4. close")?,
                    volume: optional_u64(row, "5. volume").unwrap_or_default(),
                })
            })
            .collect::<Result<Vec<_>, MarketDataError>>()?;
        bars.sort_by_key(|bar| bar.date);
        Ok(bars)
    }

    fn request(&self, parameters: &[(&str, &str)]) -> Result<Value, MarketDataError> {
        let mut request = self.client.get(self.config.base_url.clone());
        request = request
            .query(parameters)
            .query(&[("apikey", self.config.api_key.as_ref())]);
        let response = request.send().map_err(|_| {
            MarketDataError::TemporarilyUnavailable(
                "Alpha Vantage transport request failed".to_owned(),
            )
        })?;
        if response.status().as_u16() == 429 {
            return Err(MarketDataError::RateLimited {
                retry_after_ms: 60_000,
            });
        }
        if !response.status().is_success() {
            return Err(MarketDataError::Provider {
                provider: ProviderId::new(PROVIDER_ID),
                message: format!("HTTP {}", response.status().as_u16()),
                retriable: response.status().is_server_error(),
            });
        }
        let bytes = response.bytes().map_err(|_| {
            MarketDataError::TemporarilyUnavailable("Alpha Vantage response body failed".to_owned())
        })?;
        if bytes.len() > MAX_RESPONSE_BYTES {
            return Err(MarketDataError::Provider {
                provider: ProviderId::new(PROVIDER_ID),
                message: "response exceeded 2 MiB limit".to_owned(),
                retriable: false,
            });
        }
        serde_json::from_slice(&bytes).map_err(|_| MarketDataError::Provider {
            provider: ProviderId::new(PROVIDER_ID),
            message: "response was not valid JSON".to_owned(),
            retriable: false,
        })
    }
}

impl MarketDataQuery for AlphaVantageMarketData {
    fn quote_snapshots(
        &self,
        instruments: &[CanonicalInstrumentId],
    ) -> Result<Vec<QuoteSnapshot>, MarketDataError> {
        instruments
            .iter()
            .map(|instrument_id| {
                let symbol = symbol_from_canonical(instrument_id);
                self.global_quote(&symbol)
                    .map(|quote| quote_snapshot(instrument_id.clone(), quote))
            })
            .collect()
    }

    fn price_history(&self, request: &HistoryRequest) -> Result<Vec<PriceBar>, MarketDataError> {
        if !matches!(request.interval, BarInterval::OneDay | BarInterval::OneWeek) {
            return Err(MarketDataError::Unsupported(
                "Alpha Vantage adapter currently exposes daily history".to_owned(),
            ));
        }
        let symbol = symbol_from_canonical(&request.instrument_id);
        let start = request
            .start
            .as_str()
            .get(..10)
            .unwrap_or(request.start.as_str());
        let end = request
            .end
            .as_str()
            .get(..10)
            .unwrap_or(request.end.as_str());
        self.daily_history(&symbol)?
            .into_iter()
            .filter(|bar| {
                let date = bar.date.format("%Y-%m-%d").to_string();
                date.as_str() >= start && date.as_str() <= end
            })
            .map(|bar| provider_bar(request.instrument_id.clone(), request.interval, bar))
            .collect::<Result<Vec<_>, _>>()
    }
}

impl ChartHistoryQuery for AlphaVantageMarketData {
    fn load_history(
        &self,
        request: &ChartHistoryRequest,
    ) -> Result<HistorySeries, ChartHistoryError> {
        if request.period == ChartPeriod::OneDay {
            return Err(ChartHistoryError::PermissionDenied(
                "intraday history requires a realtime Alpha Vantage entitlement".to_owned(),
            ));
        }
        let symbol = request
            .instrument
            .symbol
            .split_whitespace()
            .next()
            .unwrap_or_default();
        let mut bars = self
            .daily_history(symbol)
            .map_err(|error| ChartHistoryError::Unavailable(error.to_string()))?;
        let count = request.period.sample_count();
        if bars.len() > count {
            bars.drain(..bars.len() - count);
        }
        let bars = bars
            .into_iter()
            .map(|bar| ChartPriceBar {
                timestamp: bar
                    .date
                    .and_hms_opt(0, 0, 0)
                    .expect("midnight is valid")
                    .and_utc()
                    .timestamp(),
                open: bar.open,
                high: bar.high,
                low: bar.low,
                close: bar.close,
                volume: bar.volume,
            })
            .collect();
        Ok(HistorySeries {
            instrument: request.instrument.clone(),
            bars,
            quality: HistoryQuality::Delayed,
            source: "ALPHA VANTAGE · EOD".to_owned(),
        })
    }
}

impl SpreadsheetMarketData for AlphaVantageMarketData {
    fn load_batch(&self, requests: &[MarketDataRequest]) -> Vec<MarketDataPoint> {
        let mut quotes = HashMap::<String, Result<ProviderQuote, MarketDataError>>::new();
        for request in requests {
            let symbol = request
                .security
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .to_ascii_uppercase();
            if !quotes.contains_key(&symbol) {
                quotes.insert(symbol.clone(), self.global_quote(&symbol));
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
                            "PX_LAST" => Some(quote.price),
                            "CHG_PCT_1D" => Some(quote.change_percent),
                            _ => None,
                        };
                        value.map_or_else(
                            || MarketDataState::Unavailable {
                                reason: format!("unsupported field {}", request.field),
                            },
                            |value| MarketDataState::Ready {
                                value,
                                provenance: MarketDataProvenance {
                                    provider: "ALPHA VANTAGE · GLOBAL_QUOTE".to_owned(),
                                    observed_at: quote.trading_day.clone(),
                                    received_at: received_at.clone(),
                                    quality: MarketDataQuality::Delayed,
                                },
                            },
                        )
                    }
                    Some(Err(MarketDataError::PermissionDenied(_))) => {
                        MarketDataState::PermissionDenied {
                            provider: "ALPHA VANTAGE".to_owned(),
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

#[derive(Debug, Clone)]
struct ProviderQuote {
    symbol: String,
    price: f64,
    volume: Option<u64>,
    trading_day: String,
    previous_close: f64,
    change_percent: f64,
}

#[derive(Debug, Clone)]
struct ProviderBar {
    date: NaiveDate,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: u64,
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

fn quote_snapshot(instrument_id: CanonicalInstrumentId, quote: ProviderQuote) -> QuoteSnapshot {
    let source_time = UtcTimestamp::new(format!("{}T00:00:00Z", quote.trading_day));
    let received_at = UtcTimestamp::new(Utc::now().to_rfc3339());
    QuoteSnapshot {
        instrument_id,
        symbol: quote.symbol,
        currency: "USD".to_owned(),
        last: Some(Price::new(quote.price)),
        change: Some(PriceChange {
            absolute: Price::new(quote.price - quote.previous_close),
            percent: Percent::new(quote.change_percent),
        }),
        bid: None,
        ask: None,
        volume: quote.volume.map(Quantity::new),
        as_of: source_time.clone(),
        quality: DataQuality::Delayed { minutes: 1_440 },
        provenance: DataProvenance {
            provider: ProviderId::new(PROVIDER_ID),
            source_timestamp: source_time,
            received_at,
            sequence: None,
            cache_status: CacheStatus::Live,
        },
    }
}

fn provider_bar(
    instrument_id: CanonicalInstrumentId,
    interval: BarInterval,
    bar: ProviderBar,
) -> Result<PriceBar, MarketDataError> {
    Ok(PriceBar {
        instrument_id,
        interval,
        opened_at: UtcTimestamp::new(format!("{}T00:00:00Z", bar.date)),
        open: Price::new(bar.open),
        high: Price::new(bar.high),
        low: Price::new(bar.low),
        close: Price::new(bar.close),
        volume: Quantity::new(bar.volume),
        quality: DataQuality::Delayed { minutes: 1_440 },
    })
}

fn required_string(
    object: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<String, MarketDataError> {
    object
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| invalid_field(key))
}

fn required_number(
    object: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<f64, MarketDataError> {
    object
        .get(key)
        .and_then(Value::as_str)
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite())
        .ok_or_else(|| invalid_field(key))
}

fn required_percent(
    object: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<f64, MarketDataError> {
    object
        .get(key)
        .and_then(Value::as_str)
        .and_then(|value| value.trim_end_matches('%').parse::<f64>().ok())
        .filter(|value| value.is_finite())
        .ok_or_else(|| invalid_field(key))
}

fn optional_u64(object: &serde_json::Map<String, Value>, key: &str) -> Option<u64> {
    object
        .get(key)
        .and_then(Value::as_str)
        .and_then(|value| value.parse().ok())
}

fn invalid_field(field: &str) -> MarketDataError {
    MarketDataError::Provider {
        provider: ProviderId::new(PROVIDER_ID),
        message: format!("response omitted or invalidated field {field}"),
        retriable: false,
    }
}

fn provider_payload_error(payload: &Value) -> MarketDataError {
    if let Some(note) = payload.get("Note").and_then(Value::as_str) {
        if note.to_ascii_lowercase().contains("frequency") {
            return MarketDataError::RateLimited {
                retry_after_ms: 60_000,
            };
        }
    }
    if let Some(information) = payload.get("Information").and_then(Value::as_str) {
        return MarketDataError::PermissionDenied(bound_message(information));
    }
    if let Some(message) = payload.get("Error Message").and_then(Value::as_str) {
        return MarketDataError::InvalidRequest(bound_message(message));
    }
    MarketDataError::Provider {
        provider: ProviderId::new(PROVIDER_ID),
        message: "response did not contain requested data".to_owned(),
        retriable: false,
    }
}

fn bound_message(message: &str) -> String {
    message.chars().take(240).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_symbols_that_could_mutate_provider_queries() {
        assert!(normalize_symbol("IBM").is_ok());
        assert!(normalize_symbol("BRK-B").is_ok());
        assert!(normalize_symbol("IBM&apikey=secret").is_err());
    }

    #[test]
    #[ignore = "live Alpha Vantage contract test"]
    fn live_demo_quote_flows_through_market_and_spreadsheet_ports() {
        let adapter = AlphaVantageMarketData::new(AlphaVantageConfig::demo());
        let quote = adapter.global_quote("IBM").expect("live IBM demo quote");
        assert_eq!(quote.symbol, "IBM");
        assert!(quote.price > 0.0);
        assert!(quote.previous_close > 0.0);
        assert!(NaiveDate::parse_from_str(&quote.trading_day, "%Y-%m-%d").is_ok());

        let snapshots = adapter
            .quote_snapshots(&[CanonicalInstrumentId::new("us:listed:ibm")])
            .expect("market-data port quote");
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].symbol, "IBM");
        assert!(snapshots[0].last.is_some_and(|price| price.value() > 0.0));
        assert_eq!(
            snapshots[0].quality,
            DataQuality::Delayed { minutes: 1_440 }
        );
        assert_eq!(snapshots[0].provenance.provider.as_str(), PROVIDER_ID);

        let points = adapter.load_batch(&[
            MarketDataRequest::new("IBM US Equity", "PX_LAST"),
            MarketDataRequest::new("IBM US Equity", "CHG_PCT_1D"),
        ]);
        assert_eq!(points.len(), 2);
        assert!(points.iter().all(|point| matches!(
            point.state,
            MarketDataState::Ready {
                provenance: MarketDataProvenance {
                    quality: MarketDataQuality::Delayed,
                    ..
                },
                ..
            }
        )));
    }
}
