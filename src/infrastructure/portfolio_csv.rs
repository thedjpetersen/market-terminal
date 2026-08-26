use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    sync::RwLock,
};

use chrono::Utc;
use csv::StringRecord;

use crate::features::portfolio::{
    PortfolioError, PortfolioRepository, PortfolioSnapshot, Position,
};

const MAX_PORTFOLIO_BYTES: u64 = 10 * 1024 * 1024;
const MAX_PORTFOLIO_ROWS: usize = 25_000;
const MAX_PORTFOLIO_COLUMNS: usize = 256;

pub struct CsvPortfolioRepository {
    snapshot: RwLock<PortfolioSnapshot>,
    path: RwLock<Option<PathBuf>>,
}

impl CsvPortfolioRepository {
    pub fn from_env() -> Self {
        let repository = Self {
            snapshot: RwLock::new(PortfolioSnapshot::empty(
                "NO PORTFOLIO IMPORTED · USE PORT IMPORT <FILE.CSV>",
            )),
            path: RwLock::new(None),
        };
        if let Some(path) = env::var_os("MARKET_TERMINAL_PORTFOLIO_CSV")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
        {
            let path = expand_home(path);
            if let Err(error) = repository.import_path(&path) {
                repository
                    .snapshot
                    .write()
                    .expect("portfolio snapshot lock")
                    .source = format!("AUTO-IMPORT ERROR · {error}");
            }
        }
        repository
    }

    fn import_path(&self, path: &Path) -> Result<PortfolioSnapshot, PortfolioError> {
        let metadata = fs::metadata(path).map_err(|error| {
            PortfolioError::Io(format!("CANNOT READ {} · {error}", display_name(path)))
        })?;
        if metadata.len() > MAX_PORTFOLIO_BYTES {
            return Err(PortfolioError::InvalidCsv(format!(
                "{} IS TOO LARGE · LIMIT IS 10 MB",
                display_name(path)
            )));
        }
        let bytes = fs::read(path).map_err(|error| {
            PortfolioError::Io(format!("CANNOT READ {} · {error}", display_name(path)))
        })?;
        let snapshot = parse_portfolio_csv(&bytes, display_name(path))?;
        *self.snapshot.write().expect("portfolio snapshot lock") = snapshot.clone();
        *self.path.write().expect("portfolio path lock") = Some(path.to_path_buf());
        Ok(snapshot)
    }
}

impl PortfolioRepository for CsvPortfolioRepository {
    fn load_portfolio(&self) -> PortfolioSnapshot {
        self.snapshot
            .read()
            .expect("portfolio snapshot lock")
            .clone()
    }

    fn import_csv(&self, path: &Path) -> Result<PortfolioSnapshot, PortfolioError> {
        self.import_path(path)
    }

    fn reload(&self) -> Result<PortfolioSnapshot, PortfolioError> {
        let path = self
            .path
            .read()
            .expect("portfolio path lock")
            .clone()
            .ok_or_else(|| {
                PortfolioError::Unsupported(
                    "NO IMPORTED FILE · USE PORT IMPORT <FILE.CSV>".to_owned(),
                )
            })?;
        self.import_path(&path)
    }
}

#[derive(Debug, Default)]
struct AggregatePosition {
    quantity: f64,
    market_value: f64,
    cost_basis: f64,
    has_cost_basis: bool,
    weighted_pnl_percent: f64,
    pnl_weight: f64,
    cash: bool,
}

#[derive(Debug)]
struct Columns {
    symbol: Option<usize>,
    description: Option<usize>,
    quantity: usize,
    market_value: Option<usize>,
    price: Option<usize>,
    average_cost: Option<usize>,
    cost_basis: Option<usize>,
    pnl_percent: Option<usize>,
    currency: Option<usize>,
}

fn parse_portfolio_csv(
    bytes: &[u8],
    source_name: String,
) -> Result<PortfolioSnapshot, PortfolioError> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(bytes);
    let records = reader
        .records()
        .take(MAX_PORTFOLIO_ROWS + 32)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| PortfolioError::InvalidCsv(format!("CSV PARSE ERROR · {error}")))?;
    if records.len() > MAX_PORTFOLIO_ROWS + 31 {
        return Err(PortfolioError::InvalidCsv(format!(
            "CSV EXCEEDS {MAX_PORTFOLIO_ROWS} ROWS"
        )));
    }
    if records
        .iter()
        .any(|record| record.len() > MAX_PORTFOLIO_COLUMNS)
    {
        return Err(PortfolioError::InvalidCsv(format!(
            "CSV EXCEEDS {MAX_PORTFOLIO_COLUMNS} COLUMNS"
        )));
    }

    let (header_index, columns) = records
        .iter()
        .enumerate()
        .find_map(|(index, record)| Columns::from_header(record).map(|columns| (index, columns)))
        .ok_or_else(|| PortfolioError::InvalidCsv(
            "NO POSITIONS HEADER FOUND · NEED SYMBOL/TICKER, QUANTITY/SHARES, AND MARKET VALUE OR PRICE"
                .to_owned(),
        ))?;

    let mut positions = BTreeMap::<String, AggregatePosition>::new();
    let mut rejected_currency = None;
    let mut rejected_row_count = 0;
    let mut rejected_symbols = Vec::new();
    for record in records.iter().skip(header_index + 1) {
        if record.iter().all(|field| field.trim().is_empty()) {
            continue;
        }
        if let Some(currency) = columns
            .field(record, columns.currency)
            .filter(|value| !value.is_empty())
        {
            let currency = currency.trim().to_ascii_uppercase();
            if !matches!(currency.as_str(), "USD" | "US DOLLAR" | "$" | "--" | "N/A") {
                rejected_currency = Some(currency);
                break;
            }
        }
        let description = columns
            .field(record, columns.description)
            .unwrap_or_default();
        let raw_symbol = columns.field(record, columns.symbol).unwrap_or_default();
        let cash = is_cash(raw_symbol, description);
        let symbol = normalize_symbol(raw_symbol, description, cash);
        if symbol.is_empty() || is_total_row(&symbol, description) {
            continue;
        }
        let position_candidate = cash || looks_like_position_symbol(raw_symbol);
        let quantity = parse_number(record.get(columns.quantity)).or_else(|| cash.then_some(1.0));
        let Some(quantity) = quantity else {
            if position_candidate {
                record_rejected_position(&mut rejected_row_count, &mut rejected_symbols, &symbol);
            }
            continue;
        };
        let market_value =
            parse_number(columns.field(record, columns.market_value)).or_else(|| {
                parse_number(columns.field(record, columns.price)).map(|price| price * quantity)
            });
        let Some(market_value) = market_value else {
            if position_candidate {
                record_rejected_position(&mut rejected_row_count, &mut rejected_symbols, &symbol);
            }
            continue;
        };
        if !quantity.is_finite() || !market_value.is_finite() {
            if position_candidate {
                record_rejected_position(&mut rejected_row_count, &mut rejected_symbols, &symbol);
            }
            continue;
        }

        let average_cost = parse_number(columns.field(record, columns.average_cost));
        let cost_basis = parse_number(columns.field(record, columns.cost_basis))
            .or_else(|| average_cost.map(|average| average * quantity.abs()));
        let pnl_percent = parse_number(columns.field(record, columns.pnl_percent));
        let aggregate = positions.entry(symbol).or_default();
        aggregate.quantity += quantity;
        aggregate.market_value += market_value;
        aggregate.cash |= cash;
        if let Some(cost_basis) = cost_basis {
            aggregate.cost_basis += cost_basis;
            aggregate.has_cost_basis = true;
        }
        if let Some(pnl_percent) = pnl_percent {
            let weight = market_value.abs();
            aggregate.weighted_pnl_percent += pnl_percent * weight;
            aggregate.pnl_weight += weight;
        }
    }
    if let Some(currency) = rejected_currency {
        return Err(PortfolioError::InvalidCsv(format!(
            "MULTI-CURRENCY IMPORT IS NOT YET SAFE · FOUND {currency}; CONVERT THE EXPORT TO USD"
        )));
    }
    if rejected_row_count > 0 {
        return Err(PortfolioError::InvalidCsv(format!(
            "REFUSED PARTIAL IMPORT · COULD NOT PARSE QUANTITY/VALUE FOR {rejected_row_count} POSITION ROW(S): {}",
            rejected_symbols.join(", ")
        )));
    }
    if positions.is_empty() {
        return Err(PortfolioError::InvalidCsv(
            "NO POSITION ROWS WITH NUMERIC QUANTITY AND VALUE WERE FOUND".to_owned(),
        ));
    }

    let net_asset_value = positions
        .values()
        .map(|position| position.market_value)
        .sum::<f64>();
    let available_cash = positions
        .values()
        .filter(|position| position.cash)
        .map(|position| position.market_value)
        .sum::<f64>();
    let mut aggregated = positions.into_iter().collect::<Vec<_>>();
    aggregated.sort_by(|left, right| {
        right
            .1
            .market_value
            .abs()
            .total_cmp(&left.1.market_value.abs())
            .then_with(|| left.0.cmp(&right.0))
    });
    let rows = aggregated
        .into_iter()
        .map(|(symbol, position)| {
            let average_cost = (position.has_cost_basis && position.quantity.abs() > f64::EPSILON)
                .then(|| position.cost_basis / position.quantity.abs());
            let pnl_percent = if position.pnl_weight > f64::EPSILON {
                Some(position.weighted_pnl_percent / position.pnl_weight)
            } else if position.has_cost_basis && position.cost_basis.abs() > f64::EPSILON {
                Some(
                    (position.market_value - position.cost_basis) / position.cost_basis.abs()
                        * 100.0,
                )
            } else {
                None
            };
            let weight = (net_asset_value.abs() > f64::EPSILON)
                .then_some(position.market_value / net_asset_value * 100.0);
            Position {
                symbol,
                quantity: format_quantity(position.quantity),
                average_cost: average_cost
                    .map(format_decimal)
                    .unwrap_or_else(|| "—".to_owned()),
                market_value: format_currency(position.market_value),
                pnl: pnl_percent
                    .map(format_signed_percent)
                    .unwrap_or_else(|| "—".to_owned()),
                weight: weight.map(format_percent).unwrap_or_else(|| "—".to_owned()),
            }
        })
        .collect();

    Ok(PortfolioSnapshot {
        positions: rows,
        net_asset_value: format_currency(net_asset_value),
        ytd_return: "N/A".to_owned(),
        available_cash: format_currency(available_cash),
        sharpe: "N/A".to_owned(),
        source: format!("CSV · {source_name}"),
        as_of: Utc::now().format("%Y-%m-%d %H:%M UTC").to_string(),
    })
}

impl Columns {
    fn from_header(header: &StringRecord) -> Option<Self> {
        let names = header.iter().map(normalize_header).collect::<Vec<_>>();
        let symbol = find_column(
            &names,
            &["symbol", "ticker", "tickersymbol", "securitysymbol"],
        );
        let description = find_column(
            &names,
            &["description", "securitydescription", "name", "instrument"],
        );
        let quantity = find_column(&names, &["quantity", "qty", "shares", "units"])?;
        let market_value = find_column(
            &names,
            &[
                "currentvalue",
                "marketvalue",
                "mktval",
                "totalvalue",
                "positionvalue",
                "value",
            ],
        );
        let price = find_column(
            &names,
            &["lastprice", "price", "shareprice", "currentprice"],
        );
        if symbol.is_none() && description.is_none() || market_value.is_none() && price.is_none() {
            return None;
        }
        Some(Self {
            symbol,
            description,
            quantity,
            market_value,
            price,
            average_cost: find_column(
                &names,
                &[
                    "averagecostbasis",
                    "averagecost",
                    "avgcost",
                    "costpershare",
                    "averageprice",
                ],
            ),
            cost_basis: find_column(
                &names,
                &[
                    "costbasistotal",
                    "costbasis",
                    "totalcostbasis",
                    "totalcost",
                    "bookvalue",
                ],
            ),
            pnl_percent: find_column(
                &names,
                &[
                    "totalgainlosspercent",
                    "totalgainlosspercentage",
                    "gainlosspercent",
                    "gainlosspercentage",
                    "returnpercent",
                ],
            ),
            currency: find_column(&names, &["currency", "ccy"]),
        })
    }

    fn field<'a>(&self, record: &'a StringRecord, column: Option<usize>) -> Option<&'a str> {
        column.and_then(|index| record.get(index)).map(str::trim)
    }
}

fn find_column(headers: &[String], aliases: &[&str]) -> Option<usize> {
    headers
        .iter()
        .position(|header| aliases.contains(&header.as_str()))
}

fn normalize_header(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn normalize_symbol(symbol: &str, description: &str, cash: bool) -> String {
    let symbol = symbol
        .trim()
        .trim_matches(|character| character == '*' || character == '"');
    if cash {
        return if symbol.is_empty() {
            "CASH".to_owned()
        } else {
            symbol.to_ascii_uppercase()
        };
    }
    symbol
        .split_whitespace()
        .next()
        .unwrap_or(description)
        .to_ascii_uppercase()
}

fn is_cash(symbol: &str, description: &str) -> bool {
    let combined = format!("{symbol} {description}").to_ascii_uppercase();
    combined.contains("CASH")
        || combined.contains("MONEY MARKET")
        || combined.contains("CORE POSITION")
}

fn is_total_row(symbol: &str, description: &str) -> bool {
    let combined = format!("{symbol} {description}").to_ascii_uppercase();
    symbol.eq_ignore_ascii_case("TOTAL")
        || combined.contains("ACCOUNT TOTAL")
        || combined.contains("GRAND TOTAL")
}

fn looks_like_position_symbol(value: &str) -> bool {
    let value = value
        .trim()
        .trim_matches(|character| character == '*' || character == '"');
    !value.is_empty()
        && value.len() <= 32
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '/' | '^' | '_')
        })
}

fn record_rejected_position(count: &mut usize, symbols: &mut Vec<String>, symbol: &str) {
    *count += 1;
    if symbols.len() < 5 && !symbols.iter().any(|existing| existing == symbol) {
        symbols.push(symbol.to_owned());
    }
}

fn parse_number(value: Option<&str>) -> Option<f64> {
    let value = value?.trim();
    if value.is_empty()
        || matches!(
            value.to_ascii_uppercase().as_str(),
            "N/A" | "NA" | "--" | "—"
        )
    {
        return None;
    }
    let negative = value.starts_with('(') && value.ends_with(')');
    let cleaned = value
        .chars()
        .filter(|character| {
            character.is_ascii_digit() || matches!(character, '.' | '-' | '+' | 'e' | 'E')
        })
        .collect::<String>();
    let parsed = cleaned.parse::<f64>().ok()?;
    Some(if negative { -parsed.abs() } else { parsed })
}

fn format_quantity(value: f64) -> String {
    if value.fract().abs() < 0.000_001 {
        format_grouped(value, 0)
    } else {
        format_grouped(value, 4)
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_owned()
    }
}

fn format_currency(value: f64) -> String {
    format!("${}", format_grouped(value, 2))
}

fn format_decimal(value: f64) -> String {
    format_grouped(value, 2)
}

fn format_percent(value: f64) -> String {
    format!("{value:.2}%")
}

fn format_signed_percent(value: f64) -> String {
    format!("{value:+.2}%")
}

fn format_grouped(value: f64, decimals: usize) -> String {
    let negative = value.is_sign_negative();
    let formatted = format!("{:.*}", decimals, value.abs());
    let (whole, fraction) = formatted.split_once('.').unwrap_or((&formatted, ""));
    let mut grouped = String::new();
    for (index, character) in whole.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(character);
    }
    let mut result = grouped.chars().rev().collect::<String>();
    if decimals > 0 {
        result.push('.');
        result.push_str(fraction);
    }
    if negative {
        result.insert(0, '-');
    }
    result
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("portfolio.csv")
        .to_owned()
}

fn expand_home(path: PathBuf) -> PathBuf {
    let Some(value) = path.to_str() else {
        return path;
    };
    if value == "~" {
        return env::var_os("HOME").map(PathBuf::from).unwrap_or(path);
    }
    if let Some(relative) = value.strip_prefix("~/") {
        if let Some(home) = env::var_os("HOME") {
            return PathBuf::from(home).join(relative);
        }
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn imports_fidelity_style_export_and_aggregates_accounts() {
        let csv = br#"Account Number,Symbol,Description,Quantity,Last Price,Current Value,Total Gain/Loss Percent,Cost Basis Total,Currency
111,AAPL,APPLE INC,2,$200.00,$400.00,25%,$320.00,USD
222,AAPL,APPLE INC,3,$200.00,$600.00,20%,$500.00,USD
111,SPAXX**,FIDELITY GOVERNMENT MONEY MARKET,1,$125.50,$125.50,0%,$125.50,USD
"#;

        let snapshot = parse_portfolio_csv(csv, "positions.csv".to_owned()).unwrap();

        assert_eq!(snapshot.positions.len(), 2);
        assert_eq!(snapshot.positions[0].symbol, "AAPL");
        assert_eq!(snapshot.positions[0].quantity, "5");
        assert_eq!(snapshot.positions[0].market_value, "$1,000.00");
        assert_eq!(snapshot.net_asset_value, "$1,125.50");
        assert_eq!(snapshot.available_cash, "$125.50");
    }

    #[test]
    fn finds_schwab_style_header_after_a_preamble() {
        let csv = br#"Positions for account XXXX-1234
Exported today
Symbol,Description,Qty,Price,Mkt Val,Cost Basis,Gain/Loss %,Currency
MSFT,MICROSOFT CORP,4,$500.00,"$2,000.00","$1,600.00",25%,USD
"#;

        let snapshot = parse_portfolio_csv(csv, "schwab.csv".to_owned()).unwrap();

        assert_eq!(snapshot.positions[0].symbol, "MSFT");
        assert_eq!(snapshot.positions[0].average_cost, "400.00");
        assert_eq!(snapshot.positions[0].pnl, "+25.00%");
    }

    #[test]
    fn rejects_non_usd_totals_instead_of_misstating_nav() {
        let csv = b"Symbol,Quantity,Market Value,Currency\nSAP,2,400,EUR\n";

        let error = parse_portfolio_csv(csv, "multi.csv".to_owned()).unwrap_err();

        assert!(error.to_string().contains("MULTI-CURRENCY"));
    }

    #[test]
    fn rejects_the_entire_import_when_a_position_row_cannot_be_parsed() {
        let csv = b"Symbol,Quantity,Market Value,Currency\nAAPL,2,400,USD\nMSFT,N/A,1500,USD\n";

        let error = parse_portfolio_csv(csv, "partial.csv".to_owned()).unwrap_err();

        assert!(error.to_string().contains("REFUSED PARTIAL IMPORT"));
        assert!(error.to_string().contains("MSFT"));
    }

    #[test]
    fn repository_remembers_and_reloads_the_imported_file() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = env::temp_dir().join(format!("market-terminal-portfolio-{unique}.csv"));
        let repository = CsvPortfolioRepository {
            snapshot: RwLock::new(PortfolioSnapshot::empty("TEST")),
            path: RwLock::new(None),
        };
        fs::write(&path, "Symbol,Quantity,Market Value\nAAPL,2,400\n").unwrap();
        repository.import_csv(&path).unwrap();
        fs::write(&path, "Symbol,Quantity,Market Value\nMSFT,3,1500\n").unwrap();

        let reloaded = repository.reload().unwrap();

        assert_eq!(reloaded.positions[0].symbol, "MSFT");
        assert_eq!(reloaded.net_asset_value, "$1,500.00");
        fs::remove_file(path).unwrap();
    }
}
