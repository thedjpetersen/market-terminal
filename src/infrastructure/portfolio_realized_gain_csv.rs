use std::collections::BTreeMap;

use chrono::NaiveDate;
use csv::StringRecord;

use crate::{
    features::portfolio::{
        PortfolioAccountId, PortfolioClosedLot, PortfolioError, PortfolioRealizedGainCurrencyTotal,
        PortfolioRealizedGainSnapshot, PositionQuantity, TaxLotHoldingPeriod,
    },
    foundation::{Currency, InstrumentId, Money},
};

use super::portfolio_csv::{csv_input_version, parse_currency, parse_scaled};

pub(super) const MAX_REALIZED_GAIN_ROWS: usize = 100_000;
pub(super) const MAX_REALIZED_GAIN_COLUMNS: usize = 128;

#[derive(Debug)]
struct ClosedLotColumns {
    account: Option<usize>,
    symbol: usize,
    acquired_date: usize,
    disposed_date: usize,
    holding_period: Option<usize>,
    quantity: usize,
    proceeds: usize,
    cost_basis: usize,
    realized_gain: Option<usize>,
    currency: Option<usize>,
}

#[derive(Debug, Default)]
struct AggregateCurrency {
    lots: usize,
    proceeds_minor: i128,
    cost_basis_minor: i128,
    realized_gain_minor: i128,
    short_term_gain_minor: i128,
    long_term_gain_minor: i128,
    unknown_term_gain_minor: i128,
}

pub(super) fn parse_portfolio_realized_gain_csv(
    bytes: &[u8],
    source_name: String,
) -> Result<PortfolioRealizedGainSnapshot, PortfolioError> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(bytes);
    let records = reader
        .records()
        .take(MAX_REALIZED_GAIN_ROWS + 33)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| PortfolioError::InvalidCsv(format!("CSV PARSE ERROR · {error}")))?;
    if records
        .iter()
        .any(|record| record.len() > MAX_REALIZED_GAIN_COLUMNS)
    {
        return Err(PortfolioError::InvalidCsv(format!(
            "CLOSED-LOT CSV EXCEEDS {MAX_REALIZED_GAIN_COLUMNS} COLUMNS"
        )));
    }

    let (header_index, columns) = records
        .iter()
        .take(32)
        .enumerate()
        .find_map(|(index, record)| {
            ClosedLotColumns::from_header(record).map(|columns| (index, columns))
        })
        .ok_or_else(|| {
            PortfolioError::InvalidCsv(
                "NO CLOSED-LOT HEADER FOUND · NEED SYMBOL, ACQUIRED DATE, SOLD DATE, QUANTITY, PROCEEDS, AND COST BASIS"
                    .to_owned(),
            )
        })?;
    if records.len().saturating_sub(header_index + 1) > MAX_REALIZED_GAIN_ROWS {
        return Err(PortfolioError::InvalidCsv(format!(
            "CLOSED-LOT CSV EXCEEDS {MAX_REALIZED_GAIN_ROWS} DATA ROWS"
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
        let Some(disposed_date) = parse_date(record.get(columns.disposed_date).unwrap_or_default())
        else {
            reject(&mut rejected, row, "SOLD DATE");
            continue;
        };
        if disposed_date < acquired_date {
            reject(&mut rejected, row, "SOLD BEFORE ACQUIRED");
            continue;
        }
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
        let Some(proceeds) = parse_scaled(record.get(columns.proceeds), decimals) else {
            reject(&mut rejected, row, "PROCEEDS");
            continue;
        };
        if proceeds < 0 {
            reject(&mut rejected, row, "NEGATIVE PROCEEDS");
            continue;
        }
        let Some(cost_basis) = parse_scaled(record.get(columns.cost_basis), decimals) else {
            reject(&mut rejected, row, "COST BASIS");
            continue;
        };
        if cost_basis < 0 {
            reject(&mut rejected, row, "NEGATIVE COST BASIS");
            continue;
        }
        let Some(realized_gain) = proceeds.checked_sub(cost_basis) else {
            return Err(PortfolioError::InvalidCsv(
                "CLOSED-LOT GAIN OVERFLOW".to_owned(),
            ));
        };
        if let Some(reported) = columns.field(record, columns.realized_gain) {
            if !reported.trim().is_empty()
                && parse_scaled(Some(reported), decimals) != Some(realized_gain)
            {
                reject(&mut rejected, row, "GAIN DOES NOT RECONCILE");
                continue;
            }
        }
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
        let realized_return_bps = (cost_basis > 0)
            .then(|| return_bps(realized_gain, cost_basis))
            .transpose()?;
        lots.push(PortfolioClosedLot {
            lot_id: format!("CLOSED-{row:06}"),
            account_id,
            instrument_id: InstrumentId::new(format!(
                "unresolved:portfolio:{}",
                symbol.to_ascii_lowercase()
            )),
            symbol,
            acquired_date,
            disposed_date,
            holding_period,
            currency,
            quantity: PositionQuantity::from_scaled_units(quantity),
            proceeds: Money::from_minor_units(proceeds, currency),
            cost_basis: Money::from_minor_units(cost_basis, currency),
            realized_gain: Money::from_minor_units(realized_gain, currency),
            realized_return_bps,
        });
    }
    if !rejected.is_empty() {
        return Err(PortfolioError::InvalidCsv(format!(
            "REFUSED PARTIAL CLOSED-LOT IMPORT · INVALID {}",
            rejected.join(", ")
        )));
    }
    if lots.is_empty() {
        return Err(PortfolioError::InvalidCsv(
            "NO CLOSED TAX LOTS WERE FOUND".to_owned(),
        ));
    }

    lots.sort_by(|left, right| {
        right
            .disposed_date
            .cmp(&left.disposed_date)
            .then_with(|| left.currency.cmp(&right.currency))
            .then_with(|| left.symbol.cmp(&right.symbol))
            .then_with(|| left.lot_id.cmp(&right.lot_id))
    });
    let period = format!(
        "{} — {}",
        lots.iter()
            .map(|lot| lot.disposed_date.as_str())
            .min()
            .unwrap(),
        lots.iter()
            .map(|lot| lot.disposed_date.as_str())
            .max()
            .unwrap()
    );
    let currency_totals = reconcile(&lots)?;
    let mut disclosures = vec![
        "BROKER CLOSED LOTS ONLY · NO LOT-MATCHING INFERENCE FROM CASH ACTIVITY".to_owned(),
        "PROCEEDS LESS COST BASIS MUST EQUAL EACH REPORTED GAIN OR LOSS".to_owned(),
        "BROKER ACCOUNT IDENTIFIERS REPLACED WITH IMPORT-LOCAL LABELS".to_owned(),
        "TICKERS REMAIN UNRESOLVED UNTIL INSTRUMENT-MASTER MATCHING".to_owned(),
        "REALIZED GAINS ARE PROVIDER DATA · NOT TAX ADVICE".to_owned(),
    ];
    if defaulted_currency {
        disclosures.push("MISSING CURRENCY DEFAULTED TO USD".to_owned());
    }
    if unknown_holding_periods > 0 {
        disclosures.push(format!(
            "{unknown_holding_periods} CLOSED LOT(S) HAVE UNKNOWN PROVIDER HOLDING PERIOD"
        ));
    }
    if currency_totals.len() > 1 {
        disclosures.push("NO FX CONVERSION · REALIZED GAINS RECONCILE BY CURRENCY".to_owned());
    }

    Ok(PortfolioRealizedGainSnapshot {
        lots,
        currency_totals,
        source: format!("CSV · {source_name}"),
        period,
        input_version: csv_input_version(bytes),
        methodology:
            "BROKER-REPORTED CLOSED LOTS · PROCEEDS − BASIS · EXACT PER-CURRENCY SUM · NO FX"
                .to_owned(),
        disclosures,
    })
}

fn reconcile(
    lots: &[PortfolioClosedLot],
) -> Result<Vec<PortfolioRealizedGainCurrencyTotal>, PortfolioError> {
    let mut totals = BTreeMap::<Currency, AggregateCurrency>::new();
    for lot in lots {
        let total = totals.entry(lot.currency).or_default();
        total.lots += 1;
        total.proceeds_minor = checked_add(total.proceeds_minor, lot.proceeds.minor_units())?;
        total.cost_basis_minor = checked_add(total.cost_basis_minor, lot.cost_basis.minor_units())?;
        total.realized_gain_minor =
            checked_add(total.realized_gain_minor, lot.realized_gain.minor_units())?;
        let term_total = match lot.holding_period {
            TaxLotHoldingPeriod::ShortTerm => &mut total.short_term_gain_minor,
            TaxLotHoldingPeriod::LongTerm => &mut total.long_term_gain_minor,
            TaxLotHoldingPeriod::Unknown => &mut total.unknown_term_gain_minor,
        };
        *term_total = checked_add(*term_total, lot.realized_gain.minor_units())?;
    }
    Ok(totals
        .into_iter()
        .map(|(currency, total)| PortfolioRealizedGainCurrencyTotal {
            currency,
            lots: total.lots,
            proceeds: Money::from_minor_units(total.proceeds_minor, currency),
            cost_basis: Money::from_minor_units(total.cost_basis_minor, currency),
            realized_gain: Money::from_minor_units(total.realized_gain_minor, currency),
            short_term_gain: Money::from_minor_units(total.short_term_gain_minor, currency),
            long_term_gain: Money::from_minor_units(total.long_term_gain_minor, currency),
            unknown_term_gain: Money::from_minor_units(total.unknown_term_gain_minor, currency),
        })
        .collect())
}

fn checked_add(left: i128, right: i128) -> Result<i128, PortfolioError> {
    left.checked_add(right)
        .ok_or_else(|| PortfolioError::InvalidCsv("CLOSED-LOT TOTAL OVERFLOW".to_owned()))
}

fn return_bps(gain: i128, cost_basis: i128) -> Result<i32, PortfolioError> {
    let numerator = gain
        .checked_mul(10_000)
        .ok_or_else(|| PortfolioError::InvalidCsv("CLOSED-LOT RETURN OVERFLOW".to_owned()))?;
    let quotient = numerator / cost_basis;
    let remainder = numerator % cost_basis;
    let rounded = if remainder.unsigned_abs().saturating_mul(2) >= cost_basis.unsigned_abs() {
        quotient + numerator.signum()
    } else {
        quotient
    };
    i32::try_from(rounded).map_err(|_| {
        PortfolioError::InvalidCsv("CLOSED-LOT RETURN EXCEEDS SUPPORTED RANGE".to_owned())
    })
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

impl ClosedLotColumns {
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
                    "dateacquiredorreceived",
                    "opendate",
                ],
            )?,
            disposed_date: find(
                &names,
                &[
                    "datesold",
                    "solddate",
                    "datedisposed",
                    "disposaldate",
                    "closeddate",
                    "datesoldordisposed",
                    "closedate",
                ],
            )?,
            holding_period: find(
                &names,
                &["holdingperiod", "term", "taxterm", "gainlossterm"],
            ),
            quantity: find(&names, &["quantity", "qty", "shares", "units"])?,
            proceeds: find(&names, &["proceeds", "saleproceeds", "netproceeds"])?,
            cost_basis: find(
                &names,
                &[
                    "costbasis",
                    "totalcostbasis",
                    "adjustedcostbasis",
                    "costorotherbasis",
                ],
            )?,
            realized_gain: find(
                &names,
                &["realizedgainloss", "gainloss", "realizedgain", "gainorloss"],
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
    fn reconciles_closed_lots_and_term_buckets_exactly() {
        let csv = b"Account,Symbol,Date Acquired,Date Sold,Term,Quantity,Proceeds,Cost Basis,Gain/Loss,Currency\nBroker 123,AAPL,2024-01-02,2026-03-01,Long Term,2.5,500.00,300.00,200.00,USD\nBroker 123,MSFT,08/01/2026,08/20/2026,Short,1,120.00,150.00,-30.00,USD\nBroker 456,SAP,2023-03-04,2025-01-02,,3,400.00,300.00,100.00,EUR\n";

        let snapshot = parse_portfolio_realized_gain_csv(csv, "closed.csv".to_owned()).unwrap();

        assert_eq!(snapshot.lots.len(), 3);
        assert_eq!(snapshot.currency_totals.len(), 2);
        let usd = snapshot
            .currency_totals
            .iter()
            .find(|total| total.currency.as_str() == "USD")
            .unwrap();
        assert_eq!(usd.proceeds.minor_units(), 62_000);
        assert_eq!(usd.cost_basis.minor_units(), 45_000);
        assert_eq!(usd.realized_gain.minor_units(), 17_000);
        assert_eq!(usd.long_term_gain.minor_units(), 20_000);
        assert_eq!(usd.short_term_gain.minor_units(), -3_000);
        assert!(snapshot
            .lots
            .iter()
            .all(|lot| lot.account_id.as_str().starts_with("ACCOUNT ")));
        assert_eq!(snapshot.period, "2025-01-02 — 2026-08-20");
    }

    #[test]
    fn refuses_unreconciled_or_chronologically_invalid_rows() {
        let bad_gain = b"Symbol,Date Acquired,Date Sold,Quantity,Proceeds,Cost Basis,Gain/Loss\nAAPL,2025-01-01,2026-01-01,1,200,100,99\n";
        let bad_dates = b"Symbol,Date Acquired,Date Sold,Quantity,Proceeds,Cost Basis\nAAPL,2026-01-02,2026-01-01,1,200,100\n";

        assert!(
            parse_portfolio_realized_gain_csv(bad_gain, "bad.csv".to_owned())
                .unwrap_err()
                .to_string()
                .contains("GAIN DOES NOT RECONCILE")
        );
        assert!(
            parse_portfolio_realized_gain_csv(bad_dates, "bad.csv".to_owned())
                .unwrap_err()
                .to_string()
                .contains("SOLD BEFORE ACQUIRED")
        );
    }

    #[test]
    fn zero_basis_lot_keeps_gain_but_does_not_invent_a_return() {
        let csv = b"Symbol,Date Acquired,Date Sold,Quantity,Proceeds,Cost Basis\nGIFT,2020-01-01,2026-01-01,1,100,0\n";

        let snapshot = parse_portfolio_realized_gain_csv(csv, "gift.csv".to_owned()).unwrap();

        assert_eq!(snapshot.lots[0].realized_gain.minor_units(), 10_000);
        assert_eq!(snapshot.lots[0].realized_return_bps, None);
        assert_eq!(snapshot.realized_gain_label(), "$100.00");
    }
}
