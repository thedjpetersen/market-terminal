use std::{
    collections::{BTreeMap, HashMap},
    env,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use chrono::NaiveDate;
use reqwest::{blocking::Client, Url};
use serde::Deserialize;
use serde_json::Value;

use crate::features::{
    charting::{ChartHistoryQuery, ChartInstrument, ChartPeriod, HistoryRequest},
    market_data::{CanonicalInstrumentId, MarketDataQuery, QuoteSnapshot},
    security::{
        Filing, FinancialPeriod, SecurityError, SecurityIdentity, SecurityPage, SecurityQuery,
        SecurityResearch, SecuritySnapshot,
    },
};

const DEFAULT_TICKERS_URL: &str = "https://www.sec.gov/files/company_tickers.json";
const DEFAULT_DATA_BASE_URL: &str = "https://data.sec.gov/";
const DEFAULT_ARCHIVES_BASE_URL: &str = "https://www.sec.gov/Archives/edgar/data/";
const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
const CACHE_TTL: Duration = Duration::from_secs(15 * 60);

type TickerCache = Arc<Mutex<Option<(Instant, HashMap<String, SecCompany>)>>>;
type PageCache = Arc<Mutex<HashMap<String, (Instant, SecurityPage)>>>;
type AnnualFacts = BTreeMap<String, (String, f64)>;

#[derive(Clone)]
pub struct LiveSecurityConfig {
    tickers_url: Url,
    data_base_url: Url,
    archives_base_url: Url,
    user_agent: String,
    timeout: Duration,
}

impl LiveSecurityConfig {
    pub fn from_env() -> Self {
        let tickers_url =
            https_url_from_env("MARKET_TERMINAL_SEC_TICKERS_URL", DEFAULT_TICKERS_URL);
        let data_base_url =
            https_base_url_from_env("MARKET_TERMINAL_SEC_DATA_BASE_URL", DEFAULT_DATA_BASE_URL);
        let archives_base_url = https_base_url_from_env(
            "MARKET_TERMINAL_SEC_ARCHIVES_BASE_URL",
            DEFAULT_ARCHIVES_BASE_URL,
        );
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
        Self {
            tickers_url,
            data_base_url,
            archives_base_url,
            user_agent,
            timeout: Duration::from_secs(timeout),
        }
    }
}

#[derive(Clone)]
pub struct LiveSecurityQuery {
    config: LiveSecurityConfig,
    client: Client,
    market_data: Arc<dyn MarketDataQuery>,
    chart_history: Arc<dyn ChartHistoryQuery>,
    ticker_cache: TickerCache,
    page_cache: PageCache,
}

impl LiveSecurityQuery {
    pub fn from_env(
        market_data: Arc<dyn MarketDataQuery>,
        chart_history: Arc<dyn ChartHistoryQuery>,
    ) -> Self {
        Self::new(LiveSecurityConfig::from_env(), market_data, chart_history)
    }

    pub fn new(
        config: LiveSecurityConfig,
        market_data: Arc<dyn MarketDataQuery>,
        chart_history: Arc<dyn ChartHistoryQuery>,
    ) -> Self {
        let client = Client::builder()
            .timeout(config.timeout)
            .user_agent(&config.user_agent)
            .build()
            .expect("SEC security HTTP client should build");
        Self {
            config,
            client,
            market_data,
            chart_history,
            ticker_cache: Arc::new(Mutex::new(None)),
            page_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn load_uncached(&self, symbol: &str) -> Result<SecurityPage, SecurityError> {
        let ticker = normalize_ticker(symbol)?;
        let company = self.resolve_company(&ticker)?;
        let cik = format!("{:010}", company.cik);
        let submissions_url = self
            .config
            .data_base_url
            .join(&format!("submissions/CIK{cik}.json"))
            .map_err(|_| SecurityError::Unavailable("invalid SEC submissions URL".to_owned()))?;
        let facts_url = self
            .config
            .data_base_url
            .join(&format!("api/xbrl/companyfacts/CIK{cik}.json"))
            .map_err(|_| SecurityError::Unavailable("invalid SEC company-facts URL".to_owned()))?;
        let submissions = self.request_json(submissions_url)?;
        let facts = self.request_json(facts_url)?;
        let name = submissions
            .get("name")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(&company.title)
            .to_owned();
        let filings =
            filings_from_submissions(&submissions, company.cik, &self.config.archives_base_url);
        let financials = financials_from_company_facts(&facts);
        let snapshot = self.market_snapshot(&ticker, &name, company.cik, &submissions, &filings);
        let page = SecurityPage {
            snapshot,
            research: SecurityResearch {
                identity: SecurityIdentity::from_sec_cik(company.cik, &ticker),
                financials,
                estimates: Vec::new(),
                owners: Vec::new(),
                filings,
                peers: Vec::new(),
                source: "SEC EDGAR · COMPANYFACTS + SUBMISSIONS".to_owned(),
            },
        };
        Ok(page)
    }

    fn market_snapshot(
        &self,
        ticker: &str,
        name: &str,
        cik: u64,
        submissions: &Value,
        filings: &[Filing],
    ) -> SecuritySnapshot {
        let market_id =
            CanonicalInstrumentId::new(format!("us:listed:{}", ticker.to_ascii_lowercase()));
        let quote = self
            .market_data
            .quote_snapshots(&[market_id])
            .ok()
            .and_then(|mut snapshots| snapshots.pop());
        let history_request = HistoryRequest::new(
            ChartInstrument::new(format!("sec:cik:{cik:010}"), ticker),
            ChartPeriod::OneMonth,
        );
        let history = self.chart_history.load_history(&history_request).ok();

        let (last, absolute_change, percent_change, market_source, quote_summary) =
            quote_fields(quote.as_ref());
        let price_series = history
            .as_ref()
            .map(|series| chart_points(&series.bars))
            .unwrap_or_default();
        let history_status = history
            .as_ref()
            .map_or("HISTORY UNAVAILABLE", |series| series.source.as_str());
        let exchange = submissions
            .get("exchanges")
            .and_then(Value::as_array)
            .and_then(|values| values.first())
            .and_then(Value::as_str)
            .unwrap_or("UNAVAILABLE");
        let industry = submissions
            .get("sicDescription")
            .and_then(Value::as_str)
            .unwrap_or("UNAVAILABLE");
        let fiscal_year_end = submissions
            .get("fiscalYearEnd")
            .and_then(Value::as_str)
            .unwrap_or("UNAVAILABLE");
        let latest_filing = filings
            .first()
            .map_or("UNAVAILABLE", |filing| filing.filed.as_str());

        SecuritySnapshot {
            symbol: format!("{ticker} US EQUITY"),
            name: name.to_owned(),
            last,
            absolute_change,
            percent_change,
            session_summary: format!("{quote_summary} · {history_status}"),
            price_series,
            statistics: vec![
                ("CIK".to_owned(), format!("{cik:010}")),
                ("EXCHANGE".to_owned(), exchange.to_owned()),
                ("INDUSTRY".to_owned(), bound_text(industry, 28)),
                ("FISCAL YEAR END".to_owned(), fiscal_year_end.to_owned()),
                ("LATEST FILING".to_owned(), latest_filing.to_owned()),
                ("FILINGS LOADED".to_owned(), filings.len().to_string()),
            ],
            source: market_source,
        }
    }

    fn resolve_company(&self, ticker: &str) -> Result<SecCompany, SecurityError> {
        {
            let cache = self
                .ticker_cache
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if let Some((stored_at, companies)) = cache.as_ref() {
                if stored_at.elapsed() <= CACHE_TTL {
                    return companies.get(ticker).cloned().ok_or_else(|| {
                        SecurityError::Unavailable(format!(
                            "{ticker} is not in the SEC company-ticker master"
                        ))
                    });
                }
            }
        }
        let payload = self.request_json(self.config.tickers_url.clone())?;
        let tickers: HashMap<String, SecTickerPayload> = serde_json::from_value(payload)
            .map_err(|_| SecurityError::Unavailable("invalid SEC ticker master".to_owned()))?;
        let companies = tickers
            .into_values()
            .filter_map(|value| {
                normalize_ticker(&value.ticker).ok().map(|ticker| {
                    (
                        ticker,
                        SecCompany {
                            cik: value.cik,
                            title: value.title,
                        },
                    )
                })
            })
            .collect::<HashMap<_, _>>();
        let result = companies.get(ticker).cloned().ok_or_else(|| {
            SecurityError::Unavailable(format!("{ticker} is not in the SEC company-ticker master"))
        });
        *self
            .ticker_cache
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some((Instant::now(), companies));
        result
    }

    fn request_json(&self, url: Url) -> Result<Value, SecurityError> {
        let response = self
            .client
            .get(url)
            .send()
            .map_err(|_| SecurityError::Unavailable("SEC transport failure".to_owned()))?;
        if matches!(response.status().as_u16(), 401 | 403) {
            return Err(SecurityError::PermissionDenied(format!(
                "SEC HTTP {}; configure MARKET_TERMINAL_SEC_USER_AGENT",
                response.status().as_u16()
            )));
        }
        if !response.status().is_success() {
            return Err(SecurityError::Unavailable(format!(
                "SEC HTTP {}",
                response.status().as_u16()
            )));
        }
        let bytes = response
            .bytes()
            .map_err(|_| SecurityError::Unavailable("SEC response body failure".to_owned()))?;
        if bytes.len() > MAX_RESPONSE_BYTES {
            return Err(SecurityError::Unavailable(
                "SEC response exceeded 8 MiB limit".to_owned(),
            ));
        }
        serde_json::from_slice(&bytes)
            .map_err(|_| SecurityError::Unavailable("invalid SEC JSON response".to_owned()))
    }
}

impl SecurityQuery for LiveSecurityQuery {
    fn load_security(&self, symbol: &str) -> Result<SecurityPage, SecurityError> {
        let ticker = normalize_ticker(symbol)?;
        {
            let cache = self
                .page_cache
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if let Some((stored_at, page)) = cache.get(&ticker) {
                if stored_at.elapsed() <= CACHE_TTL {
                    return Ok(page.clone());
                }
            }
        }
        let page = self.load_uncached(&ticker)?;
        self.page_cache
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(ticker, (Instant::now(), page.clone()));
        Ok(page)
    }

    fn request_refresh(&self, symbol: &str) {
        if let Ok(ticker) = normalize_ticker(symbol) {
            self.page_cache
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(&ticker);
        }
    }
}

#[derive(Debug, Clone)]
struct SecCompany {
    cik: u64,
    title: String,
}

#[derive(Debug, Deserialize)]
struct SecTickerPayload {
    #[serde(rename = "cik_str")]
    cik: u64,
    ticker: String,
    title: String,
}

fn normalize_ticker(symbol: &str) -> Result<String, SecurityError> {
    let ticker = symbol
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_uppercase();
    if ticker.is_empty()
        || ticker.len() > 32
        || !ticker
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '-'))
    {
        return Err(SecurityError::Unavailable(
            "invalid security ticker".to_owned(),
        ));
    }
    Ok(ticker)
}

fn quote_fields(quote: Option<&QuoteSnapshot>) -> (String, String, String, String, String) {
    let Some(quote) = quote else {
        return (
            "UNAVAILABLE".to_owned(),
            "—".to_owned(),
            "—".to_owned(),
            "MARKET DATA UNAVAILABLE".to_owned(),
            "QUOTE UNAVAILABLE".to_owned(),
        );
    };
    let last = quote
        .last
        .map(|value| format!("{:.2}", value.value()))
        .unwrap_or_else(|| "UNAVAILABLE".to_owned());
    let (absolute_change, percent_change) = quote.change.map_or_else(
        || ("—".to_owned(), "—".to_owned()),
        |change| {
            (
                format!("{:+.2}", change.absolute.value()),
                format!("{:+.2}%", change.percent.value()),
            )
        },
    );
    let source = format!(
        "{} · {}",
        quote.provenance.provider.as_str().to_ascii_uppercase(),
        quote.quality.label()
    );
    let summary = format!("AS OF {} · {}", quote.as_of.as_str(), quote.quality.label());
    (last, absolute_change, percent_change, source, summary)
}

fn chart_points(bars: &[crate::features::charting::PriceBar]) -> Vec<(f64, f64)> {
    let denominator = bars.len().saturating_sub(1).max(1) as f64;
    bars.iter()
        .enumerate()
        .map(|(index, bar)| (index as f64 * 100.0 / denominator, bar.close))
        .collect()
}

fn financials_from_company_facts(payload: &Value) -> Vec<FinancialPeriod> {
    let revenue = first_annual_facts(
        payload,
        &[
            "Revenues",
            "RevenueFromContractWithCustomerExcludingAssessedTax",
            "SalesRevenueNet",
        ],
        "USD",
    );
    let operating_income = first_annual_facts(payload, &["OperatingIncomeLoss"], "USD");
    let net_income = first_annual_facts(payload, &["NetIncomeLoss", "ProfitLoss"], "USD");
    let diluted_eps = first_annual_facts(payload, &["EarningsPerShareDiluted"], "USD/shares");
    revenue
        .iter()
        .rev()
        .take(3)
        .map(|(end, (_, value))| FinancialPeriod {
            period: end
                .get(..4)
                .map_or_else(|| end.clone(), |year| format!("FY{}A", &year[2..])),
            revenue_billions: format_billions(*value),
            operating_income_billions: fact_value(&operating_income, end, true),
            net_income_billions: fact_value(&net_income, end, true),
            diluted_eps: fact_value(&diluted_eps, end, false),
        })
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

fn first_annual_facts(payload: &Value, tags: &[&str], unit: &str) -> AnnualFacts {
    for tag in tags {
        let facts = annual_facts(payload, tag, unit);
        if !facts.is_empty() {
            return facts;
        }
    }
    BTreeMap::new()
}

fn annual_facts(payload: &Value, tag: &str, unit: &str) -> AnnualFacts {
    let Some(values) = payload
        .get("facts")
        .and_then(|value| value.get("us-gaap"))
        .and_then(|value| value.get(tag))
        .and_then(|value| value.get("units"))
        .and_then(|value| value.get(unit))
        .and_then(Value::as_array)
    else {
        return BTreeMap::new();
    };
    let mut facts = BTreeMap::<String, (String, f64)>::new();
    for value in values {
        if value.get("form").and_then(Value::as_str) != Some("10-K")
            || value.get("fp").and_then(Value::as_str) != Some("FY")
        {
            continue;
        }
        let Some(end) = value.get("end").and_then(Value::as_str) else {
            continue;
        };
        let Some(number) = value.get("val").and_then(Value::as_f64) else {
            continue;
        };
        let filed = value
            .get("filed")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let annual_duration = value
            .get("start")
            .and_then(Value::as_str)
            .and_then(|start| NaiveDate::parse_from_str(start, "%Y-%m-%d").ok())
            .zip(NaiveDate::parse_from_str(end, "%Y-%m-%d").ok())
            .is_some_and(|(start, end)| (end - start).num_days() >= 250);
        if !annual_duration || !number.is_finite() {
            continue;
        }
        let should_replace = facts
            .get(end)
            .is_none_or(|(current_filed, _)| filed >= current_filed.as_str());
        if should_replace {
            facts.insert(end.to_owned(), (filed.to_owned(), number));
        }
    }
    facts
}

fn fact_value(facts: &AnnualFacts, end: &str, billions: bool) -> String {
    facts.get(end).map_or_else(
        || "—".to_owned(),
        |(_, value)| {
            if billions {
                format_billions(*value)
            } else {
                format!("{value:.2}")
            }
        },
    )
}

fn format_billions(value: f64) -> String {
    format!("{:.1}", value / 1_000_000_000.0)
}

fn filings_from_submissions(payload: &Value, cik: u64, archives_base_url: &Url) -> Vec<Filing> {
    let Some(recent) = payload.get("filings").and_then(|value| value.get("recent")) else {
        return Vec::new();
    };
    let forms = recent.get("form").and_then(Value::as_array);
    let Some(forms) = forms else {
        return Vec::new();
    };
    (0..forms.len())
        .filter_map(|index| {
            let form = recent_string(recent, "form", index)?;
            if !matches!(form, "10-K" | "10-Q" | "8-K" | "20-F" | "6-K") {
                return None;
            }
            let accession = recent_string(recent, "accessionNumber", index)?.to_owned();
            let primary_document = recent_string(recent, "primaryDocument", index).unwrap_or("");
            Some(Filing {
                filed: recent_string(recent, "filingDate", index)
                    .unwrap_or("—")
                    .to_owned(),
                form: form.to_owned(),
                period: recent_string(recent, "reportDate", index)
                    .filter(|value| !value.is_empty())
                    .unwrap_or("—")
                    .to_owned(),
                description: filing_description(form).to_owned(),
                document_url: filing_url(archives_base_url, cik, &accession, primary_document),
                accession,
            })
        })
        .take(12)
        .collect()
}

fn recent_string<'a>(recent: &'a Value, field: &str, index: usize) -> Option<&'a str> {
    recent
        .get(field)
        .and_then(Value::as_array)
        .and_then(|values| values.get(index))
        .and_then(Value::as_str)
}

fn filing_description(form: &str) -> &'static str {
    match form {
        "10-K" | "20-F" => "ANNUAL REPORT",
        "10-Q" => "QUARTERLY REPORT",
        "8-K" | "6-K" => "CURRENT REPORT",
        _ => "REGULATORY FILING",
    }
}

fn filing_url(base_url: &Url, cik: u64, accession: &str, primary_document: &str) -> Option<String> {
    if primary_document.is_empty()
        || !primary_document.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')
        })
        || !accession
            .chars()
            .all(|character| character.is_ascii_digit() || character == '-')
    {
        return None;
    }
    let accession = accession.replace('-', "");
    base_url
        .join(&format!("{cik}/{accession}/{primary_document}"))
        .ok()
        .map(|url| url.to_string())
}

fn bound_text(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

fn https_url_from_env(name: &str, default: &str) -> Url {
    env::var(name)
        .ok()
        .and_then(|value| Url::parse(&value).ok())
        .filter(|url| url.scheme() == "https")
        .unwrap_or_else(|| Url::parse(default).expect("default SEC URL is valid"))
}

fn https_base_url_from_env(name: &str, default: &str) -> Url {
    let mut url = https_url_from_env(name, default);
    if !url.path().ends_with('/') {
        let path = format!("{}/", url.path());
        url.set_path(&path);
    }
    url
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::AlphaVantageMarketData;
    use serde_json::json;

    #[test]
    fn rejects_tickers_that_can_mutate_provider_urls() {
        assert_eq!(normalize_ticker("ibm US").unwrap(), "IBM");
        assert!(normalize_ticker("IBM?x=1").is_err());
    }

    #[test]
    fn company_facts_choose_latest_filed_annual_value() {
        let payload = json!({
            "facts": {"us-gaap": {"Revenues": {"units": {"USD": [
                {"start":"2024-01-01","end":"2024-12-31","val":10_000_000_000.0,"form":"10-K","fp":"FY","filed":"2025-02-01"},
                {"start":"2024-01-01","end":"2024-12-31","val":11_000_000_000.0,"form":"10-K","fp":"FY","filed":"2026-02-01"}
            ]}}}}
        });

        let financials = financials_from_company_facts(&payload);
        assert_eq!(financials.len(), 1);
        assert_eq!(financials[0].period, "FY24A");
        assert_eq!(financials[0].revenue_billions, "11.0");
    }

    #[test]
    #[ignore = "live SEC + Alpha Vantage security contract test"]
    fn live_ibm_security_page_has_real_market_facts_and_filings() {
        let alpha = Arc::new(AlphaVantageMarketData::from_env());
        let query = LiveSecurityQuery::from_env(alpha.clone(), alpha);
        let page = query
            .load_security("IBM US EQUITY")
            .expect("live IBM security page");

        assert_eq!(
            page.research.identity.instrument_id.as_str(),
            "sec:cik:0000051143"
        );
        assert!(page
            .snapshot
            .name
            .contains("INTERNATIONAL BUSINESS MACHINES"));
        assert_ne!(page.snapshot.last, "UNAVAILABLE");
        assert!(!page.snapshot.price_series.is_empty());
        assert!(page.research.financials.len() >= 3);
        assert!(page
            .research
            .filings
            .iter()
            .any(|filing| filing.form == "10-K"));
        assert!(page
            .research
            .filings
            .iter()
            .filter_map(|filing| filing.document_url.as_deref())
            .all(|url| url.starts_with("https://www.sec.gov/Archives/edgar/data/")));
        assert!(page.research.estimates.is_empty());
        assert!(page.research.owners.is_empty());
        assert!(page.research.peers.is_empty());
    }
}
