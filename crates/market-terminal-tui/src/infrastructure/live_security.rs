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
        Filing, FinancialPeriod, InsiderTransaction, SecurityError, SecurityIdentity, SecurityPage,
        SecurityQuery, SecurityResearch, SecuritySnapshot,
    },
    spreadsheet::{
        MarketDataPoint, MarketDataProvenance, MarketDataQuality, MarketDataRequest,
        MarketDataState, SpreadsheetMarketData,
    },
};

const DEFAULT_TICKERS_URL: &str = "https://www.sec.gov/files/company_tickers.json";
const DEFAULT_DATA_BASE_URL: &str = "https://data.sec.gov/";
const DEFAULT_ARCHIVES_BASE_URL: &str = "https://www.sec.gov/Archives/edgar/data/";
const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
const MAX_FORM4_BYTES: usize = 1024 * 1024;
const MAX_FORM4_FILINGS: usize = 6;
const MAX_INSIDER_TRANSACTIONS: usize = 40;
const MAX_FACTS_CACHE_ENTRIES: usize = 32;
const MAX_FACTS_CACHE_BYTES: usize = 64 * 1024 * 1024;
const CACHE_TTL: Duration = Duration::from_secs(15 * 60);

type TickerCache = Arc<Mutex<Option<(Instant, HashMap<String, SecCompany>)>>>;
type PageCache = Arc<Mutex<HashMap<String, (Instant, SecurityPage)>>>;
type FactsCache = Arc<Mutex<HashMap<String, CachedFacts>>>;
type AnnualFacts = BTreeMap<String, (String, f64)>;

#[derive(Clone)]
struct CachedFacts {
    stored_at: Instant,
    payload: Value,
    bytes: usize,
}

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
    facts_cache: FactsCache,
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
            facts_cache: Arc::new(Mutex::new(HashMap::new())),
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
        let submissions = self.request_json(submissions_url)?;
        let facts = self.load_company_facts_for_company(&ticker, company.cik)?;
        let name = submissions
            .get("name")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(&company.title)
            .to_owned();
        let filings =
            filings_from_submissions(&submissions, company.cik, &self.config.archives_base_url);
        let (insider_transactions, insider_status) =
            self.load_insider_transactions(&submissions, company.cik);
        let financials = financials_from_company_facts(&facts);
        let snapshot = self.market_snapshot(&ticker, &name, company.cik, &submissions, &filings);
        let page = SecurityPage {
            snapshot,
            research: SecurityResearch {
                identity: SecurityIdentity::from_sec_cik(company.cik, &ticker),
                financials,
                estimates: Vec::new(),
                owners: Vec::new(),
                insider_transactions,
                insider_status,
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

    fn load_company_facts(&self, ticker: &str) -> Result<Value, SecurityError> {
        let ticker = normalize_ticker(ticker)?;
        let company = self.resolve_company(&ticker)?;
        self.load_company_facts_for_company(&ticker, company.cik)
    }

    fn load_company_facts_for_company(
        &self,
        ticker: &str,
        cik: u64,
    ) -> Result<Value, SecurityError> {
        {
            let cache = self
                .facts_cache
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if let Some(entry) = cache.get(ticker) {
                if entry.stored_at.elapsed() <= CACHE_TTL {
                    return Ok(entry.payload.clone());
                }
            }
        }
        let cik = format!("{cik:010}");
        let facts_url = self
            .config
            .data_base_url
            .join(&format!("api/xbrl/companyfacts/CIK{cik}.json"))
            .map_err(|_| SecurityError::Unavailable("invalid SEC company-facts URL".to_owned()))?;
        let (payload, payload_bytes) = self.request_json_with_size(facts_url)?;
        let mut cache = self
            .facts_cache
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        cache.retain(|_, entry| entry.stored_at.elapsed() <= CACHE_TTL);
        cache.remove(ticker);
        while !cache.is_empty()
            && (cache.len() >= MAX_FACTS_CACHE_ENTRIES
                || cache.values().map(|entry| entry.bytes).sum::<usize>() + payload_bytes
                    > MAX_FACTS_CACHE_BYTES)
        {
            if let Some(oldest) = cache
                .iter()
                .min_by_key(|(_, entry)| entry.stored_at)
                .map(|(ticker, _)| ticker.clone())
            {
                cache.remove(&oldest);
            }
        }
        cache.insert(
            ticker.to_owned(),
            CachedFacts {
                stored_at: Instant::now(),
                payload: payload.clone(),
                bytes: payload_bytes,
            },
        );
        Ok(payload)
    }

    fn request_json(&self, url: Url) -> Result<Value, SecurityError> {
        self.request_json_with_size(url).map(|(payload, _)| payload)
    }

    fn request_json_with_size(&self, url: Url) -> Result<(Value, usize), SecurityError> {
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
        let size = bytes.len();
        serde_json::from_slice(&bytes)
            .map(|payload| (payload, size))
            .map_err(|_| SecurityError::Unavailable("invalid SEC JSON response".to_owned()))
    }

    fn request_form4_xml(&self, url: Url) -> Result<String, SecurityError> {
        let response =
            self.client.get(url).send().map_err(|_| {
                SecurityError::Unavailable("SEC Form 4 transport failure".to_owned())
            })?;
        if matches!(response.status().as_u16(), 401 | 403) {
            return Err(SecurityError::PermissionDenied(format!(
                "SEC HTTP {}; configure MARKET_TERMINAL_SEC_USER_AGENT",
                response.status().as_u16()
            )));
        }
        if !response.status().is_success() {
            return Err(SecurityError::Unavailable(format!(
                "SEC Form 4 HTTP {}",
                response.status().as_u16()
            )));
        }
        let bytes = response
            .bytes()
            .map_err(|_| SecurityError::Unavailable("SEC Form 4 body failure".to_owned()))?;
        if bytes.len() > MAX_FORM4_BYTES {
            return Err(SecurityError::Unavailable(
                "SEC Form 4 exceeded 1 MiB limit".to_owned(),
            ));
        }
        String::from_utf8(bytes.to_vec())
            .map_err(|_| SecurityError::Unavailable("SEC Form 4 was not UTF-8".to_owned()))
    }

    fn load_insider_transactions(
        &self,
        submissions: &Value,
        cik: u64,
    ) -> (Vec<InsiderTransaction>, String) {
        let filings =
            form4_filings_from_submissions(submissions, cik, &self.config.archives_base_url);
        if filings.is_empty() {
            return (Vec::new(), "NO RECENT FORM 4 FILINGS".to_owned());
        }
        let requested = filings.len();
        let mut parsed = 0;
        let mut transactions = Vec::new();
        for filing in filings {
            let Ok(xml) = self.request_form4_xml(filing.xml_url.clone()) else {
                continue;
            };
            let Ok(mut filing_transactions) = parse_ownership_document(&xml, &filing) else {
                continue;
            };
            parsed += 1;
            transactions.append(&mut filing_transactions);
            if transactions.len() >= MAX_INSIDER_TRANSACTIONS {
                transactions.truncate(MAX_INSIDER_TRANSACTIONS);
                break;
            }
        }
        let status = format!(
            "SEC FORM 4 · {parsed}/{requested} FILINGS · {} TRANSACTIONS",
            transactions.len()
        );
        (transactions, status)
    }
}

impl SpreadsheetMarketData for LiveSecurityQuery {
    fn load_batch(&self, requests: &[MarketDataRequest]) -> Vec<MarketDataPoint> {
        let parsed = requests
            .iter()
            .map(|request| parse_fundamental_field(&request.field))
            .collect::<Vec<_>>();
        let mut facts = HashMap::<String, Result<Value, SecurityError>>::new();
        for (request, field) in requests.iter().zip(&parsed) {
            if !matches!(field, FundamentalRequest::Supported { .. }) {
                continue;
            }
            let Ok(ticker) = normalize_ticker(&request.security) else {
                continue;
            };
            facts
                .entry(ticker.clone())
                .or_insert_with(|| self.load_company_facts(&ticker));
        }
        let received_at = chrono::Utc::now().to_rfc3339();
        requests
            .iter()
            .zip(parsed)
            .map(|(request, field)| {
                let state = match field {
                    FundamentalRequest::Supported { field, fiscal_year } => {
                        match normalize_ticker(&request.security) {
                            Ok(ticker) => fundamental_spreadsheet_state(
                                facts.get(&ticker),
                                field,
                                fiscal_year,
                                &received_at,
                            ),
                            Err(error) => MarketDataState::Unavailable {
                                reason: error.to_string(),
                            },
                        }
                    }
                    FundamentalRequest::Unsupported(reason) => {
                        MarketDataState::Unavailable { reason }
                    }
                };
                MarketDataPoint {
                    request: request.clone(),
                    state,
                }
            })
            .collect()
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
            self.facts_cache
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(&ticker);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FundamentalField {
    Revenue,
    OperatingIncome,
    NetIncome,
    DilutedEps,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FundamentalRequest {
    Supported {
        field: FundamentalField,
        fiscal_year: i32,
    },
    Unsupported(String),
}

#[derive(Debug, Clone, PartialEq)]
struct ReportedFact {
    observed_at: String,
    value: f64,
}

fn parse_fundamental_field(field: &str) -> FundamentalRequest {
    let parts = field.split('|').collect::<Vec<_>>();
    if parts.first() != Some(&"FUNDAMENTAL") {
        return FundamentalRequest::Unsupported(format!("unsupported field {field}"));
    }
    if parts.len() != 3 {
        return FundamentalRequest::Unsupported(
            "FUNDAMENTAL requires field and fiscal period".to_owned(),
        );
    }
    let field = match parts[1] {
        "REVENUE" => FundamentalField::Revenue,
        "OPERATING_INCOME" => FundamentalField::OperatingIncome,
        "NET_INCOME" => FundamentalField::NetIncome,
        "DILUTED_EPS" => FundamentalField::DilutedEps,
        unsupported => {
            return FundamentalRequest::Unsupported(format!(
                "unsupported FUNDAMENTAL field {unsupported}"
            ));
        }
    };
    let period = parts[2].strip_suffix('A').unwrap_or(parts[2]);
    let Some(year) = period.strip_prefix("FY") else {
        return FundamentalRequest::Unsupported(
            "FUNDAMENTAL period must be FY followed by a four-digit year".to_owned(),
        );
    };
    let Ok(fiscal_year) = year.parse::<i32>() else {
        return FundamentalRequest::Unsupported(
            "FUNDAMENTAL period must be FY followed by a four-digit year".to_owned(),
        );
    };
    if year.len() != 4 || !(1900..=2200).contains(&fiscal_year) {
        return FundamentalRequest::Unsupported(
            "FUNDAMENTAL period must be FY followed by a four-digit year".to_owned(),
        );
    }
    FundamentalRequest::Supported { field, fiscal_year }
}

fn fundamental_spreadsheet_state(
    result: Option<&Result<Value, SecurityError>>,
    field: FundamentalField,
    fiscal_year: i32,
    received_at: &str,
) -> MarketDataState {
    match result {
        Some(Ok(payload)) => resolve_reported_fact(payload, field, fiscal_year).map_or_else(
            || MarketDataState::Unavailable {
                reason: format!("SEC Company Facts has no supported FY{fiscal_year} observation"),
            },
            |fact| MarketDataState::Ready {
                value: fact.value,
                provenance: MarketDataProvenance {
                    provider: "SEC EDGAR · COMPANYFACTS".to_owned(),
                    observed_at: fact.observed_at,
                    received_at: received_at.to_owned(),
                    quality: MarketDataQuality::Delayed,
                },
            },
        ),
        Some(Err(SecurityError::PermissionDenied(_))) => MarketDataState::PermissionDenied {
            provider: "SEC EDGAR".to_owned(),
        },
        Some(Err(error)) => MarketDataState::Unavailable {
            reason: error.to_string(),
        },
        None => MarketDataState::Unavailable {
            reason: "SEC Company Facts request was not issued".to_owned(),
        },
    }
}

fn resolve_reported_fact(
    payload: &Value,
    field: FundamentalField,
    fiscal_year: i32,
) -> Option<ReportedFact> {
    let (tags, unit): (&[&str], &str) = match field {
        FundamentalField::Revenue => (
            &[
                "Revenues",
                "RevenueFromContractWithCustomerExcludingAssessedTax",
                "SalesRevenueNet",
            ],
            "USD",
        ),
        FundamentalField::OperatingIncome => (&["OperatingIncomeLoss"], "USD"),
        FundamentalField::NetIncome => (&["NetIncomeLoss", "ProfitLoss"], "USD"),
        FundamentalField::DilutedEps => (&["EarningsPerShareDiluted"], "USD/shares"),
    };
    tags.iter().find_map(|tag| {
        annual_facts(payload, tag, unit)
            .into_iter()
            .rev()
            .find(|(end, _)| {
                end.get(..4).and_then(|year| year.parse::<i32>().ok()) == Some(fiscal_year)
            })
            .map(|(observed_at, (_, value))| ReportedFact { observed_at, value })
    })
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

#[derive(Debug, Clone)]
struct Form4Filing {
    filed: String,
    accession: String,
    xml_url: Url,
    document_url: Option<String>,
}

fn form4_filings_from_submissions(
    payload: &Value,
    cik: u64,
    archives_base_url: &Url,
) -> Vec<Form4Filing> {
    let Some(recent) = payload.get("filings").and_then(|value| value.get("recent")) else {
        return Vec::new();
    };
    let Some(forms) = recent.get("form").and_then(Value::as_array) else {
        return Vec::new();
    };
    (0..forms.len())
        .filter_map(|index| {
            if !matches!(recent_string(recent, "form", index)?, "4" | "4/A") {
                return None;
            }
            let accession = recent_string(recent, "accessionNumber", index)?.to_owned();
            if !safe_accession(&accession) {
                return None;
            }
            let primary_document = recent_string(recent, "primaryDocument", index)?;
            let basename = primary_document.rsplit('/').next()?;
            if basename.is_empty()
                || !basename.chars().all(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')
                })
            {
                return None;
            }
            let compact_accession = accession.replace('-', "");
            let xml_url = archives_base_url
                .join(&format!("{cik}/{compact_accession}/{basename}"))
                .ok()?;
            let document_url = archives_base_url
                .join(&format!("{cik}/{compact_accession}/{accession}-index.htm"))
                .ok()
                .map(|url| url.to_string());
            Some(Form4Filing {
                filed: recent_string(recent, "filingDate", index)
                    .unwrap_or("—")
                    .to_owned(),
                accession,
                xml_url,
                document_url,
            })
        })
        .take(MAX_FORM4_FILINGS)
        .collect()
}

fn safe_accession(accession: &str) -> bool {
    !accession.is_empty()
        && accession.len() <= 32
        && accession
            .chars()
            .all(|character| character.is_ascii_digit() || character == '-')
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct OwnershipDocument {
    document_type: Option<String>,
    period_of_report: Option<String>,
    reporting_owner: Vec<OwnershipReportingOwner>,
    aff10b5_one: Option<String>,
    non_derivative_table: Option<NonDerivativeTable>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct OwnershipReportingOwner {
    reporting_owner_id: ReportingOwnerId,
    reporting_owner_relationship: ReportingOwnerRelationship,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct ReportingOwnerId {
    rpt_owner_name: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct ReportingOwnerRelationship {
    is_director: Option<String>,
    is_officer: Option<String>,
    is_ten_percent_owner: Option<String>,
    is_other: Option<String>,
    officer_title: Option<String>,
    other_text: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct NonDerivativeTable {
    non_derivative_transaction: Vec<NonDerivativeTransaction>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct NonDerivativeTransaction {
    transaction_date: XmlValue,
    transaction_coding: TransactionCoding,
    transaction_amounts: TransactionAmounts,
    post_transaction_amounts: PostTransactionAmounts,
    ownership_nature: OwnershipNature,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct TransactionCoding {
    transaction_code: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct TransactionAmounts {
    transaction_shares: XmlValue,
    transaction_price_per_share: XmlValue,
    transaction_acquired_disposed_code: XmlValue,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct PostTransactionAmounts {
    shares_owned_following_transaction: XmlValue,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct OwnershipNature {
    direct_or_indirect_ownership: XmlValue,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct XmlValue {
    value: Option<String>,
}

fn parse_ownership_document(
    xml: &str,
    filing: &Form4Filing,
) -> Result<Vec<InsiderTransaction>, SecurityError> {
    let document: OwnershipDocument = quick_xml::de::from_str(xml)
        .map_err(|_| SecurityError::Unavailable("invalid SEC ownership XML".to_owned()))?;
    if !matches!(document.document_type.as_deref(), Some("4" | "4/A")) {
        return Err(SecurityError::Unavailable(
            "SEC ownership document was not Form 4".to_owned(),
        ));
    }
    let owner = document
        .reporting_owner
        .iter()
        .filter_map(|owner| owner.reporting_owner_id.rpt_owner_name.as_deref())
        .filter(|name| !name.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" / ");
    let owner = if owner.is_empty() {
        "UNAVAILABLE".to_owned()
    } else {
        bound_text(&owner, 32)
    };
    let role = document
        .reporting_owner
        .first()
        .map(reporting_owner_role)
        .unwrap_or_else(|| "UNAVAILABLE".to_owned());
    let plan_10b5_1 = document.aff10b5_one.as_deref().is_some_and(xml_true);
    let fallback_date = document.period_of_report.as_deref().unwrap_or("—");
    let transactions = document
        .non_derivative_table
        .map(|table| table.non_derivative_transaction)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|transaction| {
            let shares = xml_number(&transaction.transaction_amounts.transaction_shares)?;
            if shares < 0.0 {
                return None;
            }
            let price_per_share =
                xml_number(&transaction.transaction_amounts.transaction_price_per_share);
            let value_usd = price_per_share
                .map(|price| shares * price)
                .filter(|value| value.is_finite());
            let acquisition_disposition = transaction
                .transaction_amounts
                .transaction_acquired_disposed_code
                .value
                .as_deref()
                .map(str::trim)
                .map(|value| match value {
                    "A" => "ACQ",
                    "D" => "DISP",
                    other => other,
                })
                .unwrap_or("—")
                .to_owned();
            Some(InsiderTransaction {
                filed: filing.filed.clone(),
                transaction_date: transaction
                    .transaction_date
                    .value
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| fallback_date.to_owned()),
                owner: owner.clone(),
                role: role.clone(),
                transaction_code: transaction
                    .transaction_coding
                    .transaction_code
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| "—".to_owned()),
                acquisition_disposition,
                shares,
                price_per_share,
                value_usd,
                shares_after: xml_number(
                    &transaction
                        .post_transaction_amounts
                        .shares_owned_following_transaction,
                ),
                ownership_nature: transaction
                    .ownership_nature
                    .direct_or_indirect_ownership
                    .value
                    .as_deref()
                    .map(str::trim)
                    .map(|value| match value {
                        "D" => "DIRECT",
                        "I" => "INDIRECT",
                        other => other,
                    })
                    .unwrap_or("—")
                    .to_owned(),
                plan_10b5_1,
                accession: filing.accession.clone(),
                document_url: filing.document_url.clone(),
            })
        })
        .collect();
    Ok(transactions)
}

fn reporting_owner_role(owner: &OwnershipReportingOwner) -> String {
    let relationship = &owner.reporting_owner_relationship;
    if xml_true_or_false(relationship.is_officer.as_deref()) {
        if let Some(title) = relationship
            .officer_title
            .as_deref()
            .filter(|title| !title.trim().is_empty())
        {
            return bound_text(title, 24);
        }
        return "OFFICER".to_owned();
    }
    if xml_true_or_false(relationship.is_director.as_deref()) {
        return "DIRECTOR".to_owned();
    }
    if xml_true_or_false(relationship.is_ten_percent_owner.as_deref()) {
        return "10% OWNER".to_owned();
    }
    if xml_true_or_false(relationship.is_other.as_deref()) {
        return relationship
            .other_text
            .as_deref()
            .filter(|text| !text.trim().is_empty())
            .map(|text| bound_text(text, 24))
            .unwrap_or_else(|| "OTHER".to_owned());
    }
    "REPORTING OWNER".to_owned()
}

fn xml_true_or_false(value: Option<&str>) -> bool {
    value.is_some_and(xml_true)
}

fn xml_true(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "true" | "1" | "yes"
    )
}

fn xml_number(value: &XmlValue) -> Option<f64> {
    value
        .value
        .as_deref()
        .and_then(|value| value.trim().parse::<f64>().ok())
        .filter(|value| value.is_finite())
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
    let revenue = merged_annual_facts(
        payload,
        &[
            "Revenues",
            "RevenueFromContractWithCustomerExcludingAssessedTax",
            "SalesRevenueNet",
        ],
        "USD",
    );
    let operating_income = merged_annual_facts(payload, &["OperatingIncomeLoss"], "USD");
    let net_income = merged_annual_facts(payload, &["NetIncomeLoss", "ProfitLoss"], "USD");
    let diluted_eps = merged_annual_facts(payload, &["EarningsPerShareDiluted"], "USD/shares");
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

fn merged_annual_facts(payload: &Value, tags: &[&str], unit: &str) -> AnnualFacts {
    let mut merged = AnnualFacts::new();
    for tag in tags {
        for (period, observation) in annual_facts(payload, tag, unit) {
            // Tags can retire independently of their history. Prefer the latest
            // filing per period, then configured tag precedence on equal dates.
            if merged
                .get(&period)
                .is_none_or(|(filed, _)| observation.0 > *filed)
            {
                merged.insert(period, observation);
            }
        }
    }
    merged
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
    fn financials_follow_reporting_periods_across_replacement_tags() {
        let annual = |year: i32, value: f64, filed: &str| {
            json!({
                "start": format!("{year}-01-01"), "end": format!("{year}-12-31"),
                "val": value, "form": "10-K", "fp": "FY", "filed": filed,
            })
        };
        let payload = json!({"facts": {"us-gaap": {
            "Revenues": {"units": {"USD": [
                annual(2018, 265_000_000_000.0, "2019-02-01"),
                annual(2024, 390_000_000_000.0, "2025-02-01")
            ]}},
            "RevenueFromContractWithCustomerExcludingAssessedTax": {"units": {"USD": [
                annual(2023, 383_000_000_000.0, "2024-02-01"),
                annual(2024, 391_000_000_000.0, "2026-02-01"),
                annual(2025, 416_000_000_000.0, "2026-02-01")
            ]}}
        }}});
        let financials = financials_from_company_facts(&payload);
        assert_eq!(
            financials
                .iter()
                .map(|period| period.period.as_str())
                .collect::<Vec<_>>(),
            ["FY23A", "FY24A", "FY25A"]
        );
        assert_eq!(financials[1].revenue_billions, "391.0");
        assert_eq!(financials[2].revenue_billions, "416.0");
        assert_eq!(financials[2].operating_income_billions, "—");
    }

    #[test]
    fn spreadsheet_fundamental_retains_raw_latest_filed_value_and_period_end() {
        let payload = json!({
            "facts": {"us-gaap": {"Revenues": {"units": {"USD": [
                {"start":"2024-01-01","end":"2024-12-31","val":10_000_000_000.0,"form":"10-K","fp":"FY","filed":"2025-02-01"},
                {"start":"2024-01-01","end":"2024-12-31","val":11_000_000_000.0,"form":"10-K","fp":"FY","filed":"2026-02-01"}
            ]}}}}
        });

        let fact = resolve_reported_fact(&payload, FundamentalField::Revenue, 2024).unwrap();

        assert_eq!(fact.value, 11_000_000_000.0);
        assert_eq!(fact.observed_at, "2024-12-31");
        assert_eq!(
            parse_fundamental_field("FUNDAMENTAL|REVENUE|FY2024A"),
            FundamentalRequest::Supported {
                field: FundamentalField::Revenue,
                fiscal_year: 2024,
            }
        );
        assert!(matches!(
            parse_fundamental_field("FUNDAMENTAL|EBITDA|FY2024"),
            FundamentalRequest::Unsupported(reason) if reason.contains("EBITDA")
        ));
    }

    #[test]
    fn form4_metadata_uses_raw_xml_and_a_publisher_index_link() {
        let payload = json!({
            "filings": {"recent": {
                "form": ["4", "10-K"],
                "filingDate": ["2026-07-30", "2026-02-01"],
                "accessionNumber": ["0001022408-26-000070", "0000000000-26-000001"],
                "primaryDocument": ["xslF345X06/form4.xml", "report.htm"]
            }}
        });
        let base = Url::parse(DEFAULT_ARCHIVES_BASE_URL).unwrap();

        let filings = form4_filings_from_submissions(&payload, 1_022_408, &base);

        assert_eq!(filings.len(), 1);
        assert_eq!(
            filings[0].xml_url.as_str(),
            "https://www.sec.gov/Archives/edgar/data/1022408/000102240826000070/form4.xml"
        );
        assert!(filings[0]
            .document_url
            .as_deref()
            .is_some_and(|url| url.ends_with("0001022408-26-000070-index.htm")));
    }

    #[test]
    fn ownership_xml_maps_non_derivative_transactions_without_scoring_them() {
        let xml = r#"<?xml version="1.0"?>
<ownershipDocument>
  <documentType>4</documentType>
  <periodOfReport>2026-07-28</periodOfReport>
  <reportingOwner>
    <reportingOwnerId><rptOwnerName>RAIGUEL DARREN S</rptOwnerName></reportingOwnerId>
    <reportingOwnerRelationship>
      <isDirector>false</isDirector><isOfficer>true</isOfficer>
      <officerTitle>CHIEF OPERATING OFFICER</officerTitle>
    </reportingOwnerRelationship>
  </reportingOwner>
  <aff10b5One>true</aff10b5One>
  <nonDerivativeTable>
    <nonDerivativeTransaction>
      <transactionDate><value>2026-07-28</value></transactionDate>
      <transactionCoding><transactionCode>S</transactionCode></transactionCoding>
      <transactionAmounts>
        <transactionShares><value>65</value></transactionShares>
        <transactionPricePerShare><value>93.1050</value></transactionPricePerShare>
        <transactionAcquiredDisposedCode><value>D</value></transactionAcquiredDisposedCode>
      </transactionAmounts>
      <postTransactionAmounts>
        <sharesOwnedFollowingTransaction><value>71171</value></sharesOwnedFollowingTransaction>
      </postTransactionAmounts>
      <ownershipNature><directOrIndirectOwnership><value>I</value></directOrIndirectOwnership></ownershipNature>
    </nonDerivativeTransaction>
  </nonDerivativeTable>
</ownershipDocument>"#;
        let filing = Form4Filing {
            filed: "2026-07-30".to_owned(),
            accession: "0001022408-26-000070".to_owned(),
            xml_url: Url::parse("https://www.sec.gov/form4.xml").unwrap(),
            document_url: Some("https://www.sec.gov/form4-index.htm".to_owned()),
        };

        let transactions = parse_ownership_document(xml, &filing).unwrap();

        assert_eq!(transactions.len(), 1);
        let transaction = &transactions[0];
        assert_eq!(transaction.owner, "RAIGUEL DARREN S");
        assert_eq!(transaction.role, "CHIEF OPERATING OFFICER");
        assert_eq!(transaction.transaction_code, "S");
        assert_eq!(transaction.acquisition_disposition, "DISP");
        assert_eq!(transaction.shares, 65.0);
        assert_eq!(transaction.price_per_share, Some(93.105));
        assert_eq!(transaction.value_usd, Some(6_051.825));
        assert_eq!(transaction.shares_after, Some(71_171.0));
        assert_eq!(transaction.ownership_nature, "INDIRECT");
        assert!(transaction.plan_10b5_1);
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
        assert!(page.research.insider_status.starts_with("SEC FORM 4"));
        assert!(!page.research.insider_transactions.is_empty());
        assert!(page.research.estimates.is_empty());
        assert!(page.research.owners.is_empty());
        assert!(page.research.peers.is_empty());
    }
}
