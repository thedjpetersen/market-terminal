use std::collections::BTreeMap;

use chrono::{NaiveDate, Utc};
use csv::StringRecord;

use crate::{
    features::portfolio::{
        PortfolioAccountId, PortfolioError, PortfolioTaxLot, PortfolioTaxLotCurrencyTotal,
        PortfolioTaxLotSnapshot, PositionQuantity, TaxLotHoldingPeriod,
    },
    foundation::{Currency, InstrumentId, Money},
};

use super::portfolio_csv::{csv_input_version, parse_currency, parse_scaled};

pub(super) const MAX_TAX_LOT_ROWS: usize = 100_000;
pub(super) const MAX_TAX_LOT_COLUMNS: usize = 128;

#[derive(Debug)]
struct TaxLotColumns {
    account: Option<usize>,
    symbol: usize,
    acquired_date: usize,
    holding_period: Option<usize>,
    quantity: usize,
    cost_basis: usize,
    current_value: Option<usize>,
    currency: Option<usize>,
}

#[derive(Debug, Default)]
struct AggregateCurrency {
    lots: usize,
    cost_basis_minor: i128,
    priced_cost_basis_minor: i128,
    current_value_minor: i128,
    unrealized_gain_minor: i128,
    unpriced_lots: usize,
}

pub(super) fn parse_portfolio_tax_lot_csv(
    bytes: &[u8],
    source_name: String,
) -> Result<PortfolioTaxLotSnapshot, PortfolioError> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(bytes);
    let records = reader
        .records()
        .take(MAX_TAX_LOT_ROWS + 33)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| PortfolioError::InvalidCsv(format!("CSV PARSE ERROR · {error}")))?;
    if records
        .iter()
        .any(|record| record.len() > MAX_TAX_LOT_COLUMNS)
    {
        return Err(PortfolioError::InvalidCsv(format!(
            "TAX-LOT CSV EXCEEDS {MAX_TAX_LOT_COLUMNS} COLUMNS"
        )));
    }

    let (header_index, columns) = records
        .iter()
        .take(32)
        .enumerate()
        .find_map(|(index, record)| {
            TaxLotColumns::from_header(record).map(|columns| (index, columns))
        })
        .ok_or_else(|| {
            PortfolioError::InvalidCsv(
                "NO TAX-LOT HEADER FOUND · NEED SYMBOL, ACQUIRED DATE, QUANTITY, AND COST BASIS"
                    .to_owned(),
            )
        })?;
    if records.len().saturating_sub(header_index + 1) > MAX_TAX_LOT_ROWS {
        return Err(PortfolioError::InvalidCsv(format!(
            "TAX-LOT CSV EXCEEDS {MAX_TAX_LOT_ROWS} DATA ROWS"
        )));
    }

    let usd = Currency::new("USD").expect("USD is a valid currency");
    let mut lots = Vec::new();
    let mut account_aliases = BTreeMap::<String, PortfolioAccountId>::new();
    let mut rejected = Vec::new();
    let mut defaulted_currency = false;
    let mut unknown_holding_periods = 0_usize;

    for (record_index, record) in records.iter().enumerate().skip(header_index + 1) {
        if record.iter().all(|field| field.trim().is_empty()) {
            continue;
        }
        let row = record_index + 1;
        let Some(symbol) = normalize_symbol(record.get(columns.symbol).unwrap_or_default()) else {
            reject(&mut rejected, row, "SYMBOL");
            continue;
        };
        let Some(acquired_date) = parse_date(record.get(columns.acquired_date).unwrap_or_default())
        else {
            reject(&mut rejected, row, "ACQUIRED DATE");
            continue;
        };
        let currency = match columns.field(record, columns.currency) {
            Some(value) if !value.trim().is_empty() => match parse_currency(value) {
                Ok(currency) => currency,
                Err(_) => {
                    reject(&mut rejected, row, "CURRENCY");
                    continue;
                }
            },
            _ => {
                defaulted_currency = true;
                usd
            }
        };
        let Some(quantity) = parse_scaled(record.get(columns.quantity), 6) else {
            reject(&mut rejected, row, "QUANTITY");
            continue;
        };
        if quantity <= 0 {
            reject(&mut rejected, row, "NON-POSITIVE QUANTITY");
            continue;
        }
        let decimals = currency.minor_unit_digits();
        let Some(cost_basis) = parse_scaled(record.get(columns.cost_basis), decimals) else {
            reject(&mut rejected, row, "COST BASIS");
            continue;
        };
        if cost_basis < 0 {
            reject(&mut rejected, row, "NEGATIVE COST BASIS");
            continue;
        }
        let current_value = match columns.field(record, columns.current_value) {
            Some(value) if !value.trim().is_empty() => {
                let Some(value) = parse_scaled(Some(value), decimals) else {
                    reject(&mut rejected, row, "CURRENT VALUE");
                    continue;
                };
                if value < 0 {
                    reject(&mut rejected, row, "NEGATIVE CURRENT VALUE");
                    continue;
                }
                Some(value)
            }
            _ => None,
        };
        let holding_period = columns
            .field(record, columns.holding_period)
            .map(parse_holding_period)
            .unwrap_or(TaxLotHoldingPeriod::Unknown);
        if holding_period == TaxLotHoldingPeriod::Unknown {
            unknown_holding_periods += 1;
        }
        let raw_account = columns
            .field(record, columns.account)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("DEFAULT");
        let next_account = account_aliases.len() + 1;
        let account_id = account_aliases
            .entry(raw_account.to_owned())
            .or_insert_with(|| PortfolioAccountId::new(format!("ACCOUNT {next_account}")))
            .clone();
        let unrealized_gain = current_value
            .map(|value| {
                value
                    .checked_sub(cost_basis)
                    .ok_or_else(|| PortfolioError::InvalidCsv("LOT GAIN OVERFLOW".to_owned()))
            })
            .transpose()?;
        let unrealized_return_bps = unrealized_gain
            .filter(|_| cost_basis > 0)
            .map(|gain| return_bps(gain, cost_basis))
            .transpose()?;
        lots.push(PortfolioTaxLot {
            lot_id: format!("LOT-{row:06}"),
            account_id,
            instrument_id: InstrumentId::new(format!(
                "unresolved:portfolio:{}",
                symbol.to_ascii_lowercase()
            )),
            symbol,
            acquired_date,
            holding_period,
            currency,
            quantity: PositionQuantity::from_scaled_units(quantity),
            cost_basis: Money::from_minor_units(cost_basis, currency),
            current_value: current_value.map(|value| Money::from_minor_units(value, currency)),
            unrealized_gain: unrealized_gain.map(|value| Money::from_minor_units(value, currency)),
            unrealized_return_bps,
        });
    }
    if !rejected.is_empty() {
        return Err(PortfolioError::InvalidCsv(format!(
            "REFUSED PARTIAL TAX-LOT IMPORT · INVALID {}",
            rejected.join(", ")
        )));
    }
    if lots.is_empty() {
        return Err(PortfolioError::InvalidCsv(
            "NO OPEN TAX LOTS WERE FOUND".to_owned(),
        ));
    }

    lots.sort_by(|left, right| {
        left.currency
            .cmp(&right.currency)
            .then_with(|| left.symbol.cmp(&right.symbol))
            .then_with(|| left.acquired_date.cmp(&right.acquired_date))
            .then_with(|| left.lot_id.cmp(&right.lot_id))
    });
    let currency_totals = reconcile(&lots)?;
    let mut disclosures = vec![
        "OPEN LOTS ONLY · NOT A CLOSED-TRADE OR REALIZED-GAIN LEDGER".to_owned(),
        "BROKER ACCOUNT IDENTIFIERS REPLACED WITH IMPORT-LOCAL LABELS".to_owned(),
        "TICKERS REMAIN UNRESOLVED UNTIL INSTRUMENT-MASTER MATCHING".to_owned(),
        "AS-OF IS LOCAL IMPORT TIME · PROVIDER EXPORT HAS NO VALUATION TIMESTAMP".to_owned(),
    ];
    if defaulted_currency {
        disclosures.push("MISSING CURRENCY DEFAULTED TO USD".to_owned());
    }
    if unknown_holding_periods > 0 {
        disclosures.push(format!(
            "{unknown_holding_periods} LOT(S) HAVE UNKNOWN PROVIDER HOLDING PERIOD"
        ));
    }
    if currency_totals.len() > 1 {
        disclosures.push("NO FX CONVERSION · LOT TOTALS RECONCILE BY CURRENCY".to_owned());
    }
    let unpriced_lots = currency_totals
        .iter()
        .map(|total| total.unpriced_lots)
        .sum::<usize>();
    if unpriced_lots > 0 {
        disclosures.push(format!(
            "{unpriced_lots} LOT(S) EXCLUDED FROM CURRENT VALUE AND UNREALIZED GAIN"
        ));
    }

    Ok(PortfolioTaxLotSnapshot {
        lots,
        currency_totals,
        source: format!("CSV · {source_name}"),
        as_of: Utc::now().format("%Y-%m-%d %H:%M UTC").to_string(),
        input_version: csv_input_version(bytes),
        methodology: "BROKER-REPORTED OPEN-LOT BASIS AND VALUE · EXACT PER-CURRENCY SUM · NO FX"
            .to_owned(),
        disclosures,
    })
}

fn reconcile(
    lots: &[PortfolioTaxLot],
) -> Result<Vec<PortfolioTaxLotCurrencyTotal>, PortfolioError> {
    let mut totals = BTreeMap::<Currency, AggregateCurrency>::new();
    for lot in lots {
        let total = totals.entry(lot.currency).or_default();
        total.lots += 1;
        total.cost_basis_minor = checked_add(
            total.cost_basis_minor,
            lot.cost_basis.minor_units(),
            "LOT COST BASIS OVERFLOW",
        )?;
        match (lot.current_value, lot.unrealized_gain) {
            (Some(current), Some(gain)) => {
                total.priced_cost_basis_minor = checked_add(
                    total.priced_cost_basis_minor,
                    lot.cost_basis.minor_units(),
                    "PRICED LOT BASIS OVERFLOW",
                )?;
                total.current_value_minor = checked_add(
                    total.current_value_minor,
                    current.minor_units(),
                    "LOT CURRENT VALUE OVERFLOW",
                )?;
                total.unrealized_gain_minor = checked_add(
                    total.unrealized_gain_minor,
                    gain.minor_units(),
                    "LOT UNREALIZED GAIN OVERFLOW",
                )?;
            }
            _ => total.unpriced_lots += 1,
        }
    }
    Ok(totals
        .into_iter()
        .map(|(currency, total)| PortfolioTaxLotCurrencyTotal {
            currency,
            lots: total.lots,
            cost_basis: Money::from_minor_units(total.cost_basis_minor, currency),
            priced_cost_basis: Money::from_minor_units(total.priced_cost_basis_minor, currency),
            current_value: Money::from_minor_units(total.current_value_minor, currency),
            unrealized_gain: Money::from_minor_units(total.unrealized_gain_minor, currency),
            unpriced_lots: total.unpriced_lots,
        })
        .collect())
}

fn return_bps(gain: i128, cost_basis: i128) -> Result<i32, PortfolioError> {
    let numerator = gain
        .checked_mul(10_000)
        .ok_or_else(|| PortfolioError::InvalidCsv("LOT RETURN OVERFLOW".to_owned()))?;
    i32::try_from(rounded_division(numerator, cost_basis))
        .map_err(|_| PortfolioError::InvalidCsv("LOT RETURN EXCEEDS SUPPORTED RANGE".to_owned()))
}

fn rounded_division(numerator: i128, denominator: i128) -> i128 {
    let quotient = numerator / denominator;
    let remainder = numerator % denominator;
    let round_away = remainder.unsigned_abs().saturating_mul(2) >= denominator.unsigned_abs();
    if round_away {
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

fn checked_add(left: i128, right: i128, message: &str) -> Result<i128, PortfolioError> {
    left.checked_add(right)
        .ok_or_else(|| PortfolioError::InvalidCsv(message.to_owned()))
}

fn parse_date(value: &str) -> Option<String> {
    ["%Y-%m-%d", "%m/%d/%Y", "%m/%d/%y"]
        .into_iter()
        .find_map(|format| NaiveDate::parse_from_str(value.trim(), format).ok())
        .map(|date| date.format("%Y-%m-%d").to_string())
}

fn normalize_symbol(value: &str) -> Option<String> {
    let value = value.trim().trim_start_matches('$').to_ascii_uppercase();
    let value = value.replace('/', ".");
    (!value.is_empty()
        && value.len() <= 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-')))
    .then_some(value)
}

fn parse_holding_period(value: &str) -> TaxLotHoldingPeriod {
    let normalized = normalize_header(value);
    if matches!(normalized.as_str(), "short" | "shortterm" | "st") {
        TaxLotHoldingPeriod::ShortTerm
    } else if matches!(normalized.as_str(), "long" | "longterm" | "lt") {
        TaxLotHoldingPeriod::LongTerm
    } else {
        TaxLotHoldingPeriod::Unknown
    }
}

fn reject(rejected: &mut Vec<String>, row: usize, field: &str) {
    if rejected.len() < 8 {
        rejected.push(format!("ROW {row} {field}"));
    }
}

impl TaxLotColumns {
    fn from_header(header: &StringRecord) -> Option<Self> {
        let names = header.iter().map(normalize_header).collect::<Vec<_>>();
        Some(Self {
            account: find(&names, &["account", "accountname", "accountnumber"]),
            symbol: find(
                &names,
                &["symbol", "ticker", "tickersymbol", "securitysymbol"],
            )?,
            acquired_date: find(
                &names,
                &[
                    "dateacquired",
                    "acquireddate",
                    "acquisitiondate",
                    "purchasedate",
                    "lotdate",
                ],
            )?,
            holding_period: find(
                &names,
                &["holdingperiod", "term", "taxterm", "gainlossterm"],
            ),
            quantity: find(&names, &["quantity", "qty", "shares", "units"])?,
            cost_basis: find(
                &names,
                &["costbasis", "totalcostbasis", "costbasistotal", "totalcost"],
            )?,
            current_value: find(
                &names,
                &[
                    "currentvalue",
                    "marketvalue",
                    "mktval",
                    "totalvalue",
                    "value",
                ],
            ),
            currency: find(&names, &["currency", "currencycode", "ccy"]),
        })
    }

    fn field<'a>(&self, record: &'a StringRecord, column: Option<usize>) -> Option<&'a str> {
        column.and_then(|column| record.get(column))
    }
}

fn normalize_header(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn find(names: &[String], candidates: &[&str]) -> Option<usize> {
    candidates
        .iter()
        .find_map(|candidate| names.iter().position(|name| name == candidate))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconciles_open_lots_with_exact_gain_and_holding_periods() {
        let csv = b"Account,Symbol,Date Acquired,Term,Quantity,Cost Basis,Current Value,Currency\nBroker 123,AAPL,2024-01-02,Long Term,2.5,250.00,400.00,USD\nBroker 123,AAPL,08/01/2026,Short,1,180.00,160.00,USD\nBroker 456,SAP,2023-03-04,LT,3,300.00,,EUR\n";

        let snapshot = parse_portfolio_tax_lot_csv(csv, "lots.csv".to_owned()).unwrap();

        assert_eq!(snapshot.lots.len(), 3);
        assert_eq!(snapshot.currency_totals.len(), 2);
        let usd = snapshot
            .currency_totals
            .iter()
            .find(|total| total.currency.as_str() == "USD")
            .unwrap();
        assert_eq!(usd.cost_basis.minor_units(), 43_000);
        assert_eq!(usd.current_value.minor_units(), 56_000);
        assert_eq!(usd.unrealized_gain.minor_units(), 13_000);
        let first_apple_lot = snapshot
            .lots
            .iter()
            .find(|lot| lot.symbol == "AAPL" && lot.acquired_date == "2024-01-02")
            .unwrap();
        assert_eq!(first_apple_lot.unrealized_return_bps, Some(6_000));
        assert!(snapshot
            .lots
            .iter()
            .all(|lot| lot.account_id.as_str().starts_with("ACCOUNT ")));
        assert!(snapshot
            .disclosures
            .iter()
            .any(|disclosure| disclosure.contains("NO FX CONVERSION")));
    }

    #[test]
    fn refuses_partial_imports_and_invalid_open_lots() {
        let bad_date =
            b"Symbol,Date Acquired,Quantity,Cost Basis\nAAPL,2026-01-02,2,100\nMSFT,Various,1,50\n";
        let negative_quantity =
            b"Symbol,Date Acquired,Quantity,Cost Basis\nAAPL,2026-01-02,-2,100\n";

        assert!(parse_portfolio_tax_lot_csv(bad_date, "bad.csv".to_owned())
            .unwrap_err()
            .to_string()
            .contains("REFUSED PARTIAL"));
        assert!(
            parse_portfolio_tax_lot_csv(negative_quantity, "bad.csv".to_owned())
                .unwrap_err()
                .to_string()
                .contains("NON-POSITIVE QUANTITY")
        );
    }

    #[test]
    fn preserves_unpriced_and_zero_basis_lots_without_inventing_returns() {
        let csv = b"Symbol,Date Acquired,Quantity,Cost Basis,Currency\nGIFT,2020-01-01,1,0,USD\n";

        let snapshot = parse_portfolio_tax_lot_csv(csv, "gift.csv".to_owned()).unwrap();

        assert_eq!(snapshot.currency_totals[0].unpriced_lots, 1);
        assert_eq!(snapshot.lots[0].unrealized_return_bps, None);
        assert_eq!(snapshot.lots[0].current_value, None);
        assert_eq!(snapshot.current_value_label(), "N/A");
        assert_eq!(snapshot.unrealized_gain_label(), "N/A");
    }

    #[test]
    fn labels_partly_priced_currency_totals_as_partial() {
        let csv = b"Symbol,Date Acquired,Quantity,Cost Basis,Current Value,Currency\nAAPL,2024-01-01,2,250,400,USD\nMSFT,2025-01-01,1,100,,USD\n";

        let snapshot = parse_portfolio_tax_lot_csv(csv, "partial.csv".to_owned()).unwrap();

        assert_eq!(snapshot.cost_basis_label(), "$350.00");
        assert_eq!(snapshot.current_value_label(), "$400.00 · PARTIAL");
        assert_eq!(snapshot.unrealized_gain_label(), "$150.00 · PARTIAL");
        assert_eq!(
            snapshot.currency_totals[0].priced_cost_basis.minor_units(),
            25_000
        );
    }
}
