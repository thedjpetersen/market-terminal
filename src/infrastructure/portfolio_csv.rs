use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

use chrono::Utc;
use csv::StringRecord;

use crate::features::portfolio::{
    PortfolioAccountId, PortfolioCurrencyTotal, PortfolioError, PortfolioImportStateStore,
    PortfolioRepository, PortfolioSnapshot, Position, PositionQuantity,
};
use crate::foundation::{Currency, InstrumentId, Money};

const MAX_PORTFOLIO_BYTES: u64 = 10 * 1024 * 1024;
const MAX_PORTFOLIO_ROWS: usize = 25_000;
const MAX_PORTFOLIO_COLUMNS: usize = 256;

pub struct CsvPortfolioRepository {
    snapshot: RwLock<PortfolioSnapshot>,
    path: RwLock<Option<PathBuf>>,
    state_store: Option<Arc<dyn PortfolioImportStateStore>>,
}

impl CsvPortfolioRepository {
    #[cfg(test)]
    pub fn from_env() -> Self {
        Self::configured(None)
    }

    pub fn persistent(state_store: Arc<dyn PortfolioImportStateStore>) -> Self {
        Self::configured(Some(state_store))
    }

    fn configured(state_store: Option<Arc<dyn PortfolioImportStateStore>>) -> Self {
        let repository = Self {
            snapshot: RwLock::new(PortfolioSnapshot::empty(
                "NO PORTFOLIO IMPORTED · USE PORT IMPORT <FILE.CSV>",
            )),
            path: RwLock::new(None),
            state_store,
        };
        let env_path = env::var_os("MARKET_TERMINAL_PORTFOLIO_CSV")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from);
        let stored_path = if env_path.is_none() {
            repository
                .state_store
                .as_ref()
                .map(|store| store.load_import_path())
                .transpose()
        } else {
            Ok(None)
        };
        let startup_path = match stored_path {
            Ok(stored) => env_path.or(stored.flatten()),
            Err(error) => {
                repository
                    .snapshot
                    .write()
                    .expect("portfolio snapshot lock")
                    .source = format!("IMPORT STATE ERROR · {error}");
                env_path
            }
        };
        if let Some(path) = startup_path {
            let path = expand_home(path);
            if let Err(error) = repository.import_path(&path) {
                *repository.path.write().expect("portfolio path lock") = Some(path);
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
        if let Some(store) = &self.state_store {
            store.save_import_path(path)?;
        }
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
    quantity_scaled: i128,
    market_value_minor: i128,
    missing_market_value: bool,
    cost_basis_minor: i128,
    has_cost_basis: bool,
    weighted_pnl_bps: i128,
    pnl_weight_minor: i128,
    cash: bool,
}

#[derive(Debug, Default)]
struct AggregateCurrency {
    net_asset_value_minor: i128,
    available_cash_minor: i128,
    priced_positions: usize,
    unpriced_positions: usize,
}

#[derive(Debug)]
struct Columns {
    account: Option<usize>,
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
        .ok_or_else(|| {
            PortfolioError::InvalidCsv(
            "NO POSITIONS HEADER FOUND · NEED SYMBOL/TICKER (OR DESCRIPTION) AND QUANTITY/SHARES"
                .to_owned(),
        )
        })?;

    let mut positions =
        BTreeMap::<(PortfolioAccountId, Currency, String), AggregatePosition>::new();
    let mut account_aliases = BTreeMap::<String, PortfolioAccountId>::new();
    let mut rejected_row_count = 0;
    let mut rejected_symbols = Vec::new();
    let mut defaulted_currency = false;
    for record in records.iter().skip(header_index + 1) {
        if record.iter().all(|field| field.trim().is_empty()) {
            continue;
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
        let currency = columns
            .field(record, columns.currency)
            .filter(|value| !is_missing_provider_value(value))
            .map(parse_currency)
            .transpose()?
            .unwrap_or_else(|| {
                defaulted_currency = true;
                Currency::new("USD").expect("USD is valid")
            });
        let money_decimals = currency.minor_unit_digits();
        let account = columns
            .field(record, columns.account)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| "UNALLOCATED".to_owned());
        let next_account = account_aliases.len() + 1;
        let account_id = account_aliases
            .entry(account)
            .or_insert_with(|| PortfolioAccountId::new(format!("ACCOUNT {next_account}")))
            .clone();
        let position_candidate = cash || looks_like_position_symbol(raw_symbol);
        let quantity_scaled =
            parse_scaled(record.get(columns.quantity), 6).or_else(|| cash.then_some(1_000_000));
        let Some(quantity_scaled) = quantity_scaled else {
            if position_candidate {
                record_rejected_position(&mut rejected_row_count, &mut rejected_symbols, &symbol);
            }
            continue;
        };
        let market_value_minor =
            parse_scaled(columns.field(record, columns.market_value), money_decimals).or_else(
                || {
                    parse_scaled(columns.field(record, columns.price), 6).and_then(|price_scaled| {
                        scaled_product_to_minor(price_scaled, quantity_scaled, money_decimals)
                    })
                },
            );
        let average_cost_scaled = parse_scaled(columns.field(record, columns.average_cost), 6);
        let cost_basis_minor =
            parse_scaled(columns.field(record, columns.cost_basis), money_decimals).or_else(|| {
                average_cost_scaled.and_then(|average| {
                    scaled_product_to_minor(
                        average,
                        quantity_scaled.unsigned_abs() as i128,
                        money_decimals,
                    )
                })
            });
        let pnl_bps = parse_scaled(columns.field(record, columns.pnl_percent), 2)
            .and_then(|value| i32::try_from(value).ok());
        let aggregate = positions.entry((account_id, currency, symbol)).or_default();
        aggregate.quantity_scaled = aggregate
            .quantity_scaled
            .checked_add(quantity_scaled)
            .ok_or_else(|| PortfolioError::InvalidCsv("QUANTITY OVERFLOW".to_owned()))?;
        aggregate.cash |= cash;
        if let Some(market_value_minor) = market_value_minor {
            aggregate.market_value_minor = aggregate
                .market_value_minor
                .checked_add(market_value_minor)
                .ok_or_else(|| PortfolioError::InvalidCsv("MARKET VALUE OVERFLOW".to_owned()))?;
        } else {
            aggregate.missing_market_value = true;
        }
        if let Some(cost_basis_minor) = cost_basis_minor {
            aggregate.cost_basis_minor =
                aggregate
                    .cost_basis_minor
                    .checked_add(cost_basis_minor)
                    .ok_or_else(|| PortfolioError::InvalidCsv("COST BASIS OVERFLOW".to_owned()))?;
            aggregate.has_cost_basis = true;
        }
        if let (Some(pnl_bps), Some(market_value_minor)) = (pnl_bps, market_value_minor) {
            let weight = market_value_minor.abs();
            let weighted_pnl = i128::from(pnl_bps)
                .checked_mul(weight)
                .ok_or_else(|| PortfolioError::InvalidCsv("P&L OVERFLOW".to_owned()))?;
            aggregate.weighted_pnl_bps = aggregate
                .weighted_pnl_bps
                .checked_add(weighted_pnl)
                .ok_or_else(|| PortfolioError::InvalidCsv("P&L OVERFLOW".to_owned()))?;
            aggregate.pnl_weight_minor = aggregate
                .pnl_weight_minor
                .checked_add(weight)
                .ok_or_else(|| PortfolioError::InvalidCsv("P&L WEIGHT OVERFLOW".to_owned()))?;
        }
    }
    if rejected_row_count > 0 {
        return Err(PortfolioError::InvalidCsv(format!(
            "REFUSED PARTIAL IMPORT · COULD NOT PARSE QUANTITY FOR {rejected_row_count} POSITION ROW(S): {}",
            rejected_symbols.join(", ")
        )));
    }
    if positions.is_empty() {
        return Err(PortfolioError::InvalidCsv(
            "NO POSITION ROWS WITH NUMERIC QUANTITY WERE FOUND".to_owned(),
        ));
    }

    let mut currencies = BTreeMap::<Currency, AggregateCurrency>::new();
    for ((_, currency, _), position) in &positions {
        let total = currencies.entry(*currency).or_default();
        if position.missing_market_value {
            total.unpriced_positions += 1;
        } else {
            total.priced_positions += 1;
            total.net_asset_value_minor = total
                .net_asset_value_minor
                .checked_add(position.market_value_minor)
                .ok_or_else(|| PortfolioError::InvalidCsv("PORTFOLIO NAV OVERFLOW".to_owned()))?;
            if position.cash {
                total.available_cash_minor = total
                    .available_cash_minor
                    .checked_add(position.market_value_minor)
                    .ok_or_else(|| PortfolioError::InvalidCsv("CASH TOTAL OVERFLOW".to_owned()))?;
            }
        }
    }
    let mut aggregated = positions.into_iter().collect::<Vec<_>>();
    aggregated.sort_by(|left, right| {
        left.0
             .1
            .cmp(&right.0 .1)
            .then_with(|| {
                left.1
                    .missing_market_value
                    .cmp(&right.1.missing_market_value)
            })
            .then_with(|| {
                right
                    .1
                    .market_value_minor
                    .abs()
                    .cmp(&left.1.market_value_minor.abs())
            })
            .then_with(|| left.0.cmp(&right.0))
    });
    let rows = aggregated
        .into_iter()
        .map(|((account_id, currency, symbol), position)| {
            let average_cost_minor = if position.has_cost_basis && position.quantity_scaled != 0 {
                let numerator = position
                    .cost_basis_minor
                    .checked_mul(1_000_000)
                    .ok_or_else(|| {
                        PortfolioError::InvalidCsv("AVERAGE COST OVERFLOW".to_owned())
                    })?;
                Some(rounded_division(numerator, position.quantity_scaled.abs()))
            } else {
                None
            };
            let pnl_bps = if position.pnl_weight_minor > 0 {
                i32::try_from(rounded_division(
                    position.weighted_pnl_bps,
                    position.pnl_weight_minor,
                ))
                .ok()
            } else if !position.missing_market_value
                && position.has_cost_basis
                && position.cost_basis_minor != 0
            {
                let numerator = position
                    .market_value_minor
                    .checked_sub(position.cost_basis_minor)
                    .and_then(|difference| difference.checked_mul(10_000))
                    .ok_or_else(|| PortfolioError::InvalidCsv("P&L OVERFLOW".to_owned()))?;
                i32::try_from(rounded_division(numerator, position.cost_basis_minor.abs())).ok()
            } else {
                None
            };
            let currency_nav = currencies
                .get(&currency)
                .map(|total| total.net_asset_value_minor)
                .unwrap_or_default();
            let weight_bps = (!position.missing_market_value && currency_nav != 0)
                .then(|| {
                    let numerator =
                        position
                            .market_value_minor
                            .checked_mul(10_000)
                            .ok_or_else(|| {
                                PortfolioError::InvalidCsv("POSITION WEIGHT OVERFLOW".to_owned())
                            })?;
                    Ok(i32::try_from(rounded_division(numerator, currency_nav)).ok())
                })
                .transpose()?
                .flatten();
            let instrument_id = if position.cash {
                InstrumentId::new(format!("cash:{}", currency.as_str().to_ascii_lowercase()))
            } else {
                InstrumentId::new(format!(
                    "unresolved:portfolio:{}",
                    symbol.to_ascii_lowercase()
                ))
            };
            Ok(Position {
                instrument_id,
                account_id,
                symbol,
                currency,
                quantity: PositionQuantity::from_scaled_units(position.quantity_scaled),
                average_cost: average_cost_minor
                    .map(|value| Money::from_minor_units(value, currency)),
                market_value: (!position.missing_market_value)
                    .then(|| Money::from_minor_units(position.market_value_minor, currency)),
                unrealized_return_bps: pnl_bps,
                weight_bps,
                cash: position.cash,
            })
        })
        .collect::<Result<Vec<_>, PortfolioError>>()?;
    let currency_totals = currencies
        .into_iter()
        .map(|(currency, total)| PortfolioCurrencyTotal {
            currency,
            net_asset_value: Money::from_minor_units(total.net_asset_value_minor, currency),
            available_cash: Money::from_minor_units(total.available_cash_minor, currency),
            priced_positions: total.priced_positions,
            unpriced_positions: total.unpriced_positions,
        })
        .collect::<Vec<_>>();
    let unpriced_positions = currency_totals
        .iter()
        .map(|total| total.unpriced_positions)
        .sum::<usize>();
    let mut disclosures = vec![
        "POINT-IN-TIME POSITION SNAPSHOT · NO RETURN SERIES".to_owned(),
        "BROKER ACCOUNT IDENTIFIERS REPLACED WITH IMPORT-LOCAL LABELS".to_owned(),
        "TICKERS REMAIN UNRESOLVED UNTIL INSTRUMENT-MASTER MATCHING".to_owned(),
    ];
    if defaulted_currency {
        disclosures.push("MISSING CURRENCY DEFAULTED TO USD".to_owned());
    }
    if currency_totals.len() > 1 {
        disclosures.push("NO FX CONVERSION · TOTALS RECONCILE BY CURRENCY".to_owned());
    }
    if unpriced_positions > 0 {
        disclosures.push(format!(
            "{unpriced_positions} POSITION(S) EXCLUDED FROM NAV FOR MISSING PRICE/VALUE"
        ));
    }

    Ok(PortfolioSnapshot {
        positions: rows,
        currency_totals,
        ytd_return_bps: None,
        sharpe_hundredths: None,
        source: format!("CSV · {source_name}"),
        as_of: Utc::now().format("%Y-%m-%d %H:%M UTC").to_string(),
        input_version: csv_input_version(bytes),
        methodology: "BROKER-REPORTED VALUES · PER-CURRENCY SUM · NO FX CONVERSION".to_owned(),
        disclosures,
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
        if symbol.is_none() && description.is_none() {
            return None;
        }
        Some(Self {
            account: find_column(
                &names,
                &[
                    "account",
                    "accountname",
                    "accountnumber",
                    "accountid",
                    "portfolio",
                ],
            ),
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

fn parse_scaled(value: Option<&str>, decimals: u32) -> Option<i128> {
    let value = value?.trim();
    if is_missing_provider_value(value) {
        return None;
    }
    let parenthesized = value.starts_with('(') || value.ends_with(')');
    if parenthesized && !(value.starts_with('(') && value.ends_with(')')) {
        return None;
    }
    let mut number = if parenthesized {
        value[1..value.len() - 1].trim()
    } else {
        value
    };
    let mut had_currency_symbol = false;
    if let Some(first) = number
        .chars()
        .next()
        .filter(|character| matches!(character, '$' | '€' | '£'))
    {
        number = number[first.len_utf8()..].trim_start();
        had_currency_symbol = true;
    }
    let mut negative = parenthesized;
    if let Some(rest) = number.strip_prefix('-') {
        if parenthesized {
            return None;
        }
        negative = true;
        number = rest.trim_start();
    } else if let Some(rest) = number.strip_prefix('+') {
        if parenthesized {
            return None;
        }
        number = rest.trim_start();
    }
    if let Some(first) = number
        .chars()
        .next()
        .filter(|character| matches!(character, '$' | '€' | '£'))
    {
        if had_currency_symbol {
            return None;
        }
        number = number[first.len_utf8()..].trim_start();
    }
    if let Some(rest) = number.strip_suffix('%') {
        number = rest.trim_end();
    }
    if !number.chars().all(|character| {
        character.is_ascii_digit()
            || matches!(character, '.' | ',')
            || character.is_ascii_whitespace()
    }) {
        return None;
    }
    let cleaned = number
        .chars()
        .filter(|character| character.is_ascii_digit() || *character == '.')
        .collect::<String>();
    if !cleaned.chars().any(|character| character.is_ascii_digit())
        || cleaned.matches('.').count() > 1
    {
        return None;
    }
    let (whole, fraction) = cleaned.split_once('.').unwrap_or((&cleaned, ""));
    let scale = 10_i128.checked_pow(decimals)?;
    let whole = if whole.is_empty() {
        0
    } else {
        whole.parse::<i128>().ok()?
    };
    let decimals = decimals as usize;
    let retained = fraction.chars().take(decimals).collect::<String>();
    let mut fraction_value = if retained.is_empty() {
        0
    } else {
        retained.parse::<i128>().ok()?
    };
    fraction_value *= 10_i128.checked_pow((decimals.saturating_sub(retained.len())) as u32)?;
    if fraction
        .as_bytes()
        .get(decimals)
        .is_some_and(|digit| *digit >= b'5')
    {
        fraction_value += 1;
    }
    let mut result = whole.checked_mul(scale)?.checked_add(fraction_value)?;
    if negative {
        result = result.checked_neg()?;
    }
    Some(result)
}

fn is_missing_provider_value(value: &str) -> bool {
    value.trim().is_empty()
        || matches!(
            value.trim().to_ascii_uppercase().as_str(),
            "N/A" | "NA" | "--" | "—"
        )
}

fn parse_currency(value: &str) -> Result<Currency, PortfolioError> {
    let normalized = value.trim().to_ascii_uppercase();
    let normalized = match normalized.as_str() {
        "$" | "US DOLLAR" | "US DOLLARS" => "USD",
        "€" | "EURO" | "EUROS" => "EUR",
        "£" | "POUND" | "POUNDS" | "STERLING" => "GBP",
        value => value,
    };
    Currency::new(normalized).map_err(|_| {
        PortfolioError::InvalidCsv(format!("INVALID OR MISSING ISO CURRENCY · {value}"))
    })
}

fn scaled_product_to_minor(
    price_scaled: i128,
    quantity_scaled: i128,
    minor_unit_digits: u32,
) -> Option<i128> {
    let divisor = 10_i128.checked_pow(12_u32.checked_sub(minor_unit_digits)?)?;
    price_scaled
        .checked_mul(quantity_scaled)
        .map(|product| rounded_division(product, divisor))
}

fn rounded_division(numerator: i128, denominator: i128) -> i128 {
    debug_assert_ne!(denominator, 0);
    let quotient = numerator / denominator;
    let remainder = numerator % denominator;
    let rounds_away = remainder.unsigned_abs().saturating_mul(2) >= denominator.unsigned_abs();
    if rounds_away {
        quotient
            + if numerator.signum() == denominator.signum() {
                1
            } else {
                -1
            }
    } else {
        quotient
    }
}

fn csv_input_version(bytes: &[u8]) -> String {
    let hash = bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    });
    format!("CSV-FNV1A64-{hash:016X}")
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
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[derive(Default)]
    struct MemoryImportState {
        path: Mutex<Option<PathBuf>>,
    }

    impl PortfolioImportStateStore for MemoryImportState {
        fn load_import_path(&self) -> Result<Option<PathBuf>, PortfolioError> {
            Ok(self
                .path
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clone())
        }

        fn save_import_path(&self, path: &Path) -> Result<(), PortfolioError> {
            *self.path.lock().unwrap_or_else(|error| error.into_inner()) = Some(path.to_path_buf());
            Ok(())
        }
    }

    #[test]
    fn decimal_parser_converts_provider_text_to_exact_scaled_units() {
        assert_eq!(parse_scaled(Some("$1,234.567"), 2), Some(123_457));
        assert_eq!(parse_scaled(Some("."), 2), None);
        assert_eq!(parse_scaled(Some("—"), 2), None);
        assert_eq!(parse_scaled(Some("(12.3456789)"), 6), Some(-12_345_679));
        assert_eq!(parse_scaled(Some("25%"), 2), Some(2_500));
        assert_eq!(parse_scaled(Some("N/A"), 2), None);
        assert_eq!(parse_scaled(Some("1-2"), 2), None);
        assert_eq!(parse_scaled(Some("12 junk"), 2), None);
        assert_eq!(parse_scaled(Some("-$1,234.50"), 2), Some(-123_450));
    }

    #[test]
    fn imports_fidelity_style_export_without_aggregating_away_accounts() {
        let csv = br#"Account Number,Symbol,Description,Quantity,Last Price,Current Value,Total Gain/Loss Percent,Cost Basis Total,Currency
111,AAPL,APPLE INC,2,$200.00,$400.00,25%,$320.00,USD
222,AAPL,APPLE INC,3,$200.00,$600.00,20%,$500.00,USD
111,SPAXX**,FIDELITY GOVERNMENT MONEY MARKET,1,$125.50,$125.50,0%,$125.50,USD
"#;

        let snapshot = parse_portfolio_csv(csv, "positions.csv".to_owned()).unwrap();

        assert_eq!(snapshot.positions.len(), 3);
        assert_eq!(snapshot.positions[0].symbol, "AAPL");
        assert_eq!(snapshot.positions[0].quantity_label(), "3");
        assert_eq!(snapshot.positions[0].market_value_label(), "$600.00");
        assert_eq!(snapshot.positions[0].account_id.as_str(), "ACCOUNT 2");
        assert_eq!(snapshot.positions[1].account_id.as_str(), "ACCOUNT 1");
        assert!(snapshot
            .positions
            .iter()
            .all(|position| !matches!(position.account_id.as_str(), "111" | "222")));
        assert_eq!(snapshot.net_asset_value_label(), "$1,125.50");
        assert_eq!(snapshot.available_cash_label(), "$125.50");
        assert!(snapshot.input_version.starts_with("CSV-FNV1A64-"));
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
        assert_eq!(snapshot.positions[0].average_cost_label(), "$400.00");
        assert_eq!(snapshot.positions[0].pnl_label(), "+25.00%");
    }

    #[test]
    fn reconciles_multiple_currencies_without_inventing_an_fx_conversion() {
        let csv = b"Symbol,Quantity,Market Value,Currency\nSAP,2,400,EUR\nAAPL,1,200,USD\nSONY,1,7200,JPY\n";

        let snapshot = parse_portfolio_csv(csv, "multi.csv".to_owned()).unwrap();

        assert_eq!(snapshot.currency_totals.len(), 3);
        assert_eq!(snapshot.net_asset_value_label(), "3 CURRENCIES · SEE PORT");
        let jpy = snapshot
            .currency_totals
            .iter()
            .find(|total| total.currency.as_str() == "JPY")
            .unwrap();
        assert_eq!(jpy.net_asset_value.minor_units(), 7_200);
        assert_eq!(
            crate::features::portfolio::format_money(jpy.net_asset_value),
            "JPY 7,200"
        );
        assert!(snapshot
            .disclosures
            .iter()
            .any(|value| value.contains("NO FX CONVERSION")));
    }

    #[test]
    fn retains_unpriced_positions_and_discloses_the_incomplete_nav() {
        let csv = b"Symbol,Quantity,Market Value,Currency\nAAPL,2,400,USD\nMSFT,3,N/A,USD\n";

        let snapshot = parse_portfolio_csv(csv, "missing.csv".to_owned()).unwrap();

        assert_eq!(snapshot.positions.len(), 2);
        let unpriced = snapshot
            .positions
            .iter()
            .find(|position| position.symbol == "MSFT")
            .unwrap();
        assert_eq!(unpriced.market_value, None);
        assert_eq!(unpriced.market_value_label(), "UNPRICED");
        assert_eq!(
            snapshot.net_asset_value_label(),
            "$400.00 PRICED · 1 UNPRICED"
        );
        assert!(snapshot
            .disclosures
            .iter()
            .any(|value| value.contains("EXCLUDED FROM NAV")));
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
            state_store: None,
        };
        fs::write(&path, "Symbol,Quantity,Market Value\nAAPL,2,400\n").unwrap();
        repository.import_csv(&path).unwrap();
        fs::write(&path, "Symbol,Quantity,Market Value\nMSFT,3,1500\n").unwrap();

        let reloaded = repository.reload().unwrap();

        assert_eq!(reloaded.positions[0].symbol, "MSFT");
        assert_eq!(reloaded.net_asset_value_label(), "$1,500.00");
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn persistent_repository_restores_the_last_successful_import_path() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = env::temp_dir().join(format!("market-terminal-portfolio-state-{unique}.csv"));
        fs::write(&path, "Symbol,Quantity,Market Value\nAAPL,2,400\n").unwrap();
        let state = Arc::new(MemoryImportState::default());
        let first = CsvPortfolioRepository::persistent(state.clone());

        first.import_csv(&path).unwrap();
        let restored = CsvPortfolioRepository::persistent(state);

        assert_eq!(restored.load_portfolio().positions[0].symbol, "AAPL");
        assert_eq!(restored.load_portfolio().net_asset_value_label(), "$400.00");
        fs::remove_file(path).unwrap();
    }

    #[test]
    #[ignore = "requires MARKET_TERMINAL_PORTFOLIO_CSV pointing to a real user export"]
    fn live_configured_portfolio_import_contains_no_demo_positions() {
        let _ = dotenvy::dotenv();
        let repository = Arc::new(CsvPortfolioRepository::from_env());
        let snapshot = repository.load_portfolio();

        assert!(!snapshot.positions.is_empty(), "{}", snapshot.source);
        assert!(snapshot.source.starts_with("CSV ·"), "{}", snapshot.source);
        assert!(!snapshot.source.contains("DEMO"));
        assert!(snapshot.input_version.starts_with("CSV-FNV1A64-"));
        assert!(!snapshot.currency_totals.is_empty());
        assert!(snapshot.positions.iter().all(|position| {
            position
                .instrument_id
                .as_str()
                .starts_with("unresolved:portfolio:")
                || position.instrument_id.as_str().starts_with("cash:")
        }));

        use crate::app::Workspace;
        use crate::features::portfolio::PortfolioWorkspace;
        use ratatui::{backend::TestBackend, Terminal};
        let workspace = PortfolioWorkspace::new(repository);
        let backend = TestBackend::new(160, 48);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| workspace.render(frame, frame.area()))
            .unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("PORT"));
        assert!(rendered.contains("CSV-FNV1A64"));
    }
}
