use std::{
    env,
    sync::{
        atomic::{AtomicU64, Ordering as AtomicOrdering},
        mpsc::{sync_channel, SyncSender, TrySendError},
        Arc, RwLock,
    },
    time::Duration,
};

use reqwest::{blocking::Client, Url};
use serde::Deserialize;

use crate::features::instrument::{Instrument, InstrumentId, InstrumentKind, InstrumentSearch};

const DEFAULT_URL: &str = "https://www.sec.gov/files/company_tickers.json";
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone)]
pub struct SecInstrumentSearch {
    state: Arc<SecCatalogState>,
    refresh: SyncSender<()>,
}

struct SecCatalogState {
    instruments: RwLock<Vec<Instrument>>,
    status: RwLock<String>,
    revision: AtomicU64,
}

impl SecInstrumentSearch {
    pub fn from_env() -> Self {
        let url = env::var("MARKET_TERMINAL_SEC_TICKERS_URL")
            .ok()
            .and_then(|value| Url::parse(&value).ok())
            .filter(|url| url.scheme() == "https")
            .unwrap_or_else(|| Url::parse(DEFAULT_URL).expect("default SEC URL is valid"));
        let user_agent = env::var("MARKET_TERMINAL_SEC_USER_AGENT")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| {
                format!(
                    "market-terminal/{} mr.mandible@gmail.com",
                    env!("CARGO_PKG_VERSION")
                )
            });
        let timeout = env::var("MARKET_TERMINAL_SEC_TIMEOUT_SECS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .map(|seconds| seconds.clamp(3, 60))
            .unwrap_or(12);
        Self::new(url, user_agent, Duration::from_secs(timeout))
    }

    fn new(url: Url, user_agent: String, timeout: Duration) -> Self {
        let state = Arc::new(SecCatalogState {
            instruments: RwLock::new(Vec::new()),
            status: RwLock::new("LOADING SEC INSTRUMENT MASTER…".to_owned()),
            revision: AtomicU64::new(0),
        });
        let (refresh, receiver) = sync_channel(1);
        let worker_state = state.clone();
        std::thread::Builder::new()
            .name("sec-instrument-master".to_owned())
            .spawn(move || {
                let client = Client::builder()
                    .timeout(timeout)
                    .user_agent(user_agent)
                    .build()
                    .expect("SEC HTTP client should build");
                loop {
                    match fetch_catalog(&client, &url) {
                        Ok(instruments) => {
                            let count = instruments.len();
                            *write_lock(&worker_state.instruments) = instruments;
                            *write_lock(&worker_state.status) =
                                format!("SEC EDGAR · {count} LIVE IDENTITIES");
                            worker_state.revision.fetch_add(1, AtomicOrdering::Release);
                        }
                        Err(error) => {
                            *write_lock(&worker_state.status) =
                                format!("SEC UNAVAILABLE · {error}");
                            worker_state.revision.fetch_add(1, AtomicOrdering::Release);
                        }
                    }
                    if receiver.recv().is_err() {
                        break;
                    }
                    *write_lock(&worker_state.status) =
                        "REFRESHING SEC INSTRUMENT MASTER…".to_owned();
                }
            })
            .expect("SEC instrument worker should start");
        Self { state, refresh }
    }
}

impl InstrumentSearch for SecInstrumentSearch {
    fn search(&self, query: &str, limit: usize) -> Vec<Instrument> {
        let query = query.trim().to_ascii_uppercase();
        let mut matches = read_lock(&self.state.instruments)
            .iter()
            .filter_map(|instrument| {
                let symbol = instrument.symbol.to_ascii_uppercase();
                let name = instrument.name.to_ascii_uppercase();
                let score = if query.is_empty() {
                    4
                } else if symbol == query {
                    0
                } else if symbol.starts_with(&query) {
                    1
                } else if name.starts_with(&query) {
                    2
                } else if symbol.contains(&query) || name.contains(&query) {
                    3
                } else {
                    return None;
                };
                Some((score, instrument.clone()))
            })
            .collect::<Vec<_>>();
        matches.sort_by(|(left_score, left), (right_score, right)| {
            left_score
                .cmp(right_score)
                .then_with(|| left.symbol.len().cmp(&right.symbol.len()))
                .then_with(|| left.symbol.cmp(&right.symbol))
        });
        matches
            .into_iter()
            .take(limit.min(100))
            .map(|(_, instrument)| instrument)
            .collect()
    }

    fn revision(&self) -> u64 {
        self.state.revision.load(AtomicOrdering::Acquire)
    }

    fn status(&self) -> String {
        read_lock(&self.state.status).clone()
    }

    fn request_refresh(&self) {
        match self.refresh.try_send(()) {
            Ok(()) | Err(TrySendError::Full(())) => {}
            Err(TrySendError::Disconnected(())) => {
                *write_lock(&self.state.status) = "SEC INSTRUMENT WORKER STOPPED".to_owned();
            }
        }
    }
}

fn fetch_catalog(client: &Client, url: &Url) -> Result<Vec<Instrument>, String> {
    let response = client
        .get(url.clone())
        .send()
        .map_err(|_| "transport failure".to_owned())?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status().as_u16()));
    }
    let bytes = response
        .bytes()
        .map_err(|_| "response body failure".to_owned())?;
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err("response exceeded 2 MiB limit".to_owned());
    }
    let payload: std::collections::HashMap<String, SecTicker> =
        serde_json::from_slice(&bytes).map_err(|_| "invalid JSON response".to_owned())?;
    let mut instruments = payload
        .into_values()
        .filter(|ticker| valid_symbol(&ticker.ticker) && !ticker.title.trim().is_empty())
        .map(|ticker| Instrument {
            id: InstrumentId::new(format!("sec:cik:{:010}", ticker.cik)),
            symbol: ticker.ticker.to_ascii_uppercase(),
            name: ticker.title,
            venue: "US".to_owned(),
            currency: "USD".to_owned(),
            kind: InstrumentKind::Equity,
        })
        .collect::<Vec<_>>();
    instruments.sort_by(|left, right| {
        left.symbol
            .cmp(&right.symbol)
            .then_with(|| left.name.cmp(&right.name))
    });
    if instruments.is_empty() {
        return Err("SEC response contained no instruments".to_owned());
    }
    Ok(instruments)
}

#[derive(Debug, Deserialize)]
struct SecTicker {
    #[serde(rename = "cik_str")]
    cik: u64,
    ticker: String,
    title: String,
}

fn valid_symbol(symbol: &str) -> bool {
    !symbol.is_empty()
        && symbol.len() <= 32
        && symbol
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '-'))
}

fn read_lock<T>(lock: &RwLock<T>) -> std::sync::RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(|error| error.into_inner())
}

fn write_lock<T>(lock: &RwLock<T>) -> std::sync::RwLockWriteGuard<'_, T> {
    lock.write().unwrap_or_else(|error| error.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_symbols_that_are_not_safe_provider_identifiers() {
        assert!(valid_symbol("BRK-B"));
        assert!(!valid_symbol("IBM&query=1"));
    }

    #[test]
    #[ignore = "live SEC contract test"]
    fn live_sec_catalog_contains_canonical_apple_identity() {
        let client = Client::builder()
            .timeout(Duration::from_secs(15))
            .user_agent(format!(
                "market-terminal/{} mr.mandible@gmail.com",
                env!("CARGO_PKG_VERSION")
            ))
            .build()
            .unwrap();
        let instruments = fetch_catalog(&client, &Url::parse(DEFAULT_URL).unwrap()).unwrap();
        let apple = instruments
            .iter()
            .find(|instrument| instrument.symbol == "AAPL")
            .unwrap();
        assert_eq!(apple.id.as_str(), "sec:cik:0000320193");
        assert!(apple.name.to_ascii_uppercase().contains("APPLE"));
    }
}
