use std::collections::BTreeMap;

use chrono::NaiveDate;
use csv::StringRecord;

use crate::{
    features::portfolio::{
        calculate_contribution, PortfolioAccountId, PortfolioContributionInput,
        PortfolioContributionInputRow, PortfolioContributionSnapshot, PortfolioError,
    },
    foundation::{Currency, InstrumentId, Money},
};

use super::portfolio_csv::{csv_input_version, parse_currency, parse_scaled};

pub(super) const MAX_CONTRIBUTION_ROWS: usize = 25_000;
pub(super) const MAX_CONTRIBUTION_COLUMNS: usize = 64;

#[derive(Debug)]
struct ContributionColumns {
    account: Option<usize>,
    symbol: usize,
    period_start: usize,
    period_end: usize,
    beginning_value: usize,
    external_flow: Option<usize>,
    ending_value: usize,
    benchmark_beginning_value: Option<usize>,
    benchmark_ending_value: Option<usize>,
    currency: Option<usize>,
}

pub(super) fn parse_portfolio_contribution_csv(
    bytes: &[u8],
    source_name: String,
) -> Result<PortfolioContributionSnapshot, PortfolioError> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(bytes);
    let records = reader
        .records()
        .take(MAX_CONTRIBUTION_ROWS + 33)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| PortfolioError::InvalidCsv(format!("CSV PARSE ERROR · {error}")))?;
    if records
        .iter()
        .any(|record| record.len() > MAX_CONTRIBUTION_COLUMNS)
    {
        return Err(PortfolioError::InvalidCsv(format!(
            "CONTRIBUTION CSV EXCEEDS {MAX_CONTRIBUTION_COLUMNS} COLUMNS"
        )));
    }
    let (header_index, columns) = records
        .iter()
        .take(32)
        .enumerate()
        .find_map(|(index, record)| {
            ContributionColumns::from_header(record).map(|columns| (index, columns))
        })
        .ok_or_else(|| {
            PortfolioError::InvalidCsv(
                "NO CONTRIBUTION HEADER FOUND · NEED PERIOD START/END, SYMBOL, BEGINNING VALUE, AND ENDING VALUE"
                    .to_owned(),
            )
        })?;
    if columns.benchmark_beginning_value.is_some() != columns.benchmark_ending_value.is_some() {
        return Err(PortfolioError::InvalidCsv(
            "BENCHMARK BEGINNING AND ENDING VALUE COLUMNS MUST BE SUPPLIED TOGETHER".to_owned(),
        ));
    }
    if records.len().saturating_sub(header_index + 1) > MAX_CONTRIBUTION_ROWS {
        return Err(PortfolioError::InvalidCsv(format!(
            "CONTRIBUTION CSV EXCEEDS {MAX_CONTRIBUTION_ROWS} DATA ROWS"
        )));
    }

    let usd = Currency::new("USD").expect("USD is a valid currency");
    let benchmark_present = columns.benchmark_beginning_value.is_some();
    let mut account_aliases = BTreeMap::<String, PortfolioAccountId>::new();
    let mut rows = Vec::new();
    let mut rejected = Vec::new();
    let mut period = None::<(String, String)>;
    let mut defaulted_currency = false;
    let mut defaulted_flows = 0_usize;

    for (record_index, record) in records.iter().enumerate().skip(header_index + 1) {
        if record.iter().all(|field| field.trim().is_empty()) {
            continue;
        }
        let row_number = record_index + 1;
        let Some(period_start) = parse_date(record.get(columns.period_start).unwrap_or_default())
        else {
            reject(&mut rejected, row_number, "PERIOD START");
            continue;
        };
        let Some(period_end) = parse_date(record.get(columns.period_end).unwrap_or_default())
        else {
            reject(&mut rejected, row_number, "PERIOD END");
            continue;
        };
        let period_start = period_start.format("%Y-%m-%d").to_string();
        let period_end = period_end.format("%Y-%m-%d").to_string();
        let current_period = (period_start.clone(), period_end.clone());
        if let Some(expected) = &period {
            if expected != &current_period {
                reject(&mut rejected, row_number, "MIXED CONTRIBUTION PERIOD");
                continue;
            }
        } else {
            period = Some(current_period);
        }
        let Some(symbol) = normalize_symbol(record.get(columns.symbol).unwrap_or_default()) else {
            reject(&mut rejected, row_number, "SYMBOL");
            continue;
        };
        let currency = match columns.field(record, columns.currency) {
            Some(value) if !value.trim().is_empty() => match parse_currency(value) {
                Ok(currency) => currency,
                Err(_) => {
                    reject(&mut rejected, row_number, "CURRENCY");
                    continue;
                }
            },
            _ => {
                defaulted_currency = true;
                usd
            }
        };
        let decimals = currency.minor_unit_digits();
        let Some(beginning_minor) = parse_scaled(record.get(columns.beginning_value), decimals)
        else {
            reject(&mut rejected, row_number, "BEGINNING VALUE");
            continue;
        };
        let external_flow_minor = match columns.field(record, columns.external_flow) {
            Some(value) if !value.trim().is_empty() => {
                let Some(value) = parse_scaled(Some(value), decimals) else {
                    reject(&mut rejected, row_number, "EXTERNAL FLOW");
                    continue;
                };
                value
            }
            _ => {
                defaulted_flows += 1;
                0
            }
        };
        let Some(ending_minor) = parse_scaled(record.get(columns.ending_value), decimals) else {
            reject(&mut rejected, row_number, "ENDING VALUE");
            continue;
        };
        let benchmark_beginning_minor = match columns.benchmark_beginning_value {
            Some(column) => {
                let Some(value) = parse_scaled(record.get(column), decimals) else {
                    reject(&mut rejected, row_number, "BENCHMARK BEGINNING VALUE");
                    continue;
                };
                Some(value)
            }
            None => None,
        };
        let benchmark_ending_minor = match columns.benchmark_ending_value {
            Some(column) => {
                let Some(value) = parse_scaled(record.get(column), decimals) else {
                    reject(&mut rejected, row_number, "BENCHMARK ENDING VALUE");
                    continue;
                };
                Some(value)
            }
            None => None,
        };
        debug_assert_eq!(
            benchmark_beginning_minor.is_some(),
            benchmark_ending_minor.is_some()
        );
        let raw_account = columns
            .field(record, columns.account)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("DEFAULT");
        let next_account = account_aliases.len() + 1;
        let account_id = account_aliases
            .entry(raw_account.to_owned())
            .or_insert_with(|| PortfolioAccountId::new(format!("ACCOUNT {next_account}")))
            .clone();
        rows.push(PortfolioContributionInputRow {
            account_id,
            instrument_id: InstrumentId::new(format!(
                "unresolved:portfolio:{}",
                symbol.to_ascii_lowercase()
            )),
            symbol,
            currency,
            beginning_value: Money::from_minor_units(beginning_minor, currency),
            external_flow: Money::from_minor_units(external_flow_minor, currency),
            ending_value: Money::from_minor_units(ending_minor, currency),
            benchmark_beginning_value: benchmark_beginning_minor
                .map(|value| Money::from_minor_units(value, currency)),
            benchmark_ending_value: benchmark_ending_minor
                .map(|value| Money::from_minor_units(value, currency)),
        });
    }

    if !rejected.is_empty() {
        return Err(PortfolioError::InvalidCsv(format!(
            "REFUSED PARTIAL CONTRIBUTION IMPORT · INVALID {}",
            rejected.join(", ")
        )));
    }
    if rows.is_empty() {
        return Err(PortfolioError::InvalidCsv(
            "NO POSITION CONTRIBUTION ROWS WERE FOUND".to_owned(),
        ));
    }
    let (period_start, period_end) = period.expect("non-empty rows have a period");
    let mut disclosures = vec![
        "BROKER ACCOUNT IDENTIFIERS REPLACED WITH IMPORT-LOCAL LABELS".to_owned(),
        "TICKERS REMAIN UNRESOLVED UNTIL INSTRUMENT-MASTER MATCHING".to_owned(),
        "CSV ROWS ARE ONE VERIFIED POSITION PERIOD · NOT JOINED FROM UNRELATED SNAPSHOTS"
            .to_owned(),
    ];
    if defaulted_currency {
        disclosures.push("MISSING CURRENCY DEFAULTED TO USD".to_owned());
    }
    if defaulted_flows > 0 {
        disclosures.push(format!(
            "{defaulted_flows} MISSING EXTERNAL FLOW VALUE(S) DEFAULTED TO ZERO"
        ));
    }
    if !benchmark_present {
        disclosures.push("NO BENCHMARK COLUMNS · ACTIVE ATTRIBUTION UNAVAILABLE".to_owned());
    }

    calculate_contribution(PortfolioContributionInput {
        rows,
        source: format!("CSV · {source_name}"),
        period_start,
        period_end,
        input_version: csv_input_version(bytes),
        disclosures,
    })
    .map_err(|error| PortfolioError::InvalidCsv(format!("CONTRIBUTION INPUT INVALID · {error}")))
}

impl ContributionColumns {
    fn from_header(header: &StringRecord) -> Option<Self> {
        let names = header.iter().map(normalize_header).collect::<Vec<_>>();
        Some(Self {
            account: find(&names, &["account", "accountname", "accountnumber"]),
            symbol: find(
                &names,
                &["symbol", "ticker", "tickersymbol", "securitysymbol"],
            )?,
            period_start: find(
                &names,
                &["periodstart", "startdate", "beginningdate", "fromdate"],
            )?,
            period_end: find(
                &names,
                &["periodend", "enddate", "endingdate", "todate", "asofdate"],
            )?,
            beginning_value: find(
                &names,
                &[
                    "beginningvalue",
                    "startingvalue",
                    "beginningmarketvalue",
                    "startvalue",
                ],
            )?,
            external_flow: find(
                &names,
                &["externalflow", "netexternalflow", "netflow", "capitalflow"],
            ),
            ending_value: find(
                &names,
                &[
                    "endingvalue",
                    "endingmarketvalue",
                    "endvalue",
                    "marketvalue",
                ],
            )?,
            benchmark_beginning_value: find(
                &names,
                &[
                    "benchmarkbeginningvalue",
                    "benchmarkstartingvalue",
                    "benchmarkstartvalue",
                ],
            ),
            benchmark_ending_value: find(
                &names,
                &[
                    "benchmarkendingvalue",
                    "benchmarkendvalue",
                    "benchmarkmarketvalue",
                ],
            ),
            currency: find(&names, &["reportingcurrency", "currency", "ccy"]),
        })
    }

    fn field<'a>(&self, record: &'a StringRecord, column: Option<usize>) -> Option<&'a str> {
        column.and_then(|column| record.get(column))
    }
}

fn parse_date(value: &str) -> Option<NaiveDate> {
    ["%Y-%m-%d", "%m/%d/%Y", "%m/%d/%y"]
        .into_iter()
        .find_map(|format| NaiveDate::parse_from_str(value.trim(), format).ok())
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

fn reject(rejected: &mut Vec<String>, row: usize, field: &str) {
    if rejected.len() < 8 {
        rejected.push(format!("ROW {row} {field}"));
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
    fn imports_exact_contribution_and_active_attribution_with_anonymity() {
        let csv = b"Account,Symbol,Start Date,End Date,Beginning Value,External Flow,Ending Value,Benchmark Beginning Value,Benchmark Ending Value,Currency\nBROKER-123,ALPHA,2026-01-01,2026-01-31,600,0,660,500,520,USD\nBROKER-123,BETA,2026-01-01,2026-01-31,400,100,450,500,510,USD\n";

        let snapshot =
            parse_portfolio_contribution_csv(csv, "contribution.csv".to_owned()).unwrap();

        assert_eq!(snapshot.rows.len(), 2);
        assert_eq!(snapshot.period, "2026-01-01 — 2026-01-31");
        assert_eq!(snapshot.rows[0].symbol, "ALPHA");
        assert_eq!(snapshot.rows[0].account_id.as_str(), "ACCOUNT 1");
        assert_eq!(snapshot.rows[0].contribution_label(), "+6.0000%");
        assert_eq!(snapshot.rows[0].active_contribution_label(), "+4.0000%");
        assert_eq!(
            snapshot.currency_totals[0].portfolio_return_label(),
            "+1.0000%"
        );
        assert_eq!(
            snapshot.currency_totals[0].active_return_label(),
            "-2.0000%"
        );
        assert!(!format!("{snapshot:?}").contains("BROKER-123"));
        assert!(snapshot.input_version.starts_with("CSV-FNV1A64-"));
    }

    #[test]
    fn defaults_optional_flow_and_currency_with_disclosures() {
        let csv = b"Symbol,Period Start,Period End,Beginning Value,Ending Value\nALPHA,2026-01-01,2026-01-31,100,110\n";

        let snapshot = parse_portfolio_contribution_csv(csv, "simple.csv".to_owned()).unwrap();

        assert_eq!(snapshot.rows[0].external_flow.minor_units(), 0);
        assert_eq!(snapshot.portfolio_return_label(), "+10.0000%");
        assert!(snapshot
            .disclosures
            .iter()
            .any(|value| value.contains("DEFAULTED TO USD")));
        assert!(snapshot
            .disclosures
            .iter()
            .any(|value| value.contains("DEFAULTED TO ZERO")));
    }

    #[test]
    fn refuses_mixed_periods_partial_benchmark_and_bad_values() {
        let mixed = b"Symbol,Start Date,End Date,Beginning Value,Ending Value\nA,2026-01-01,2026-01-31,100,110\nB,2026-02-01,2026-02-28,100,110\n";
        let partial_benchmark = b"Symbol,Start Date,End Date,Beginning Value,Ending Value,Benchmark Beginning Value\nA,2026-01-01,2026-01-31,100,110,100\n";
        let bad_value = b"Symbol,Start Date,End Date,Beginning Value,Ending Value\nA,2026-01-01,2026-01-31,not-money,110\n";

        assert!(
            parse_portfolio_contribution_csv(mixed, "mixed.csv".to_owned())
                .unwrap_err()
                .to_string()
                .contains("MIXED CONTRIBUTION PERIOD")
        );
        assert!(
            parse_portfolio_contribution_csv(partial_benchmark, "partial.csv".to_owned())
                .unwrap_err()
                .to_string()
                .contains("SUPPLIED TOGETHER")
        );
        assert!(
            parse_portfolio_contribution_csv(bad_value, "bad.csv".to_owned())
                .unwrap_err()
                .to_string()
                .contains("REFUSED PARTIAL")
        );
    }

    #[test]
    fn keeps_currencies_separate() {
        let csv = b"Symbol,Start Date,End Date,Beginning Value,Ending Value,Currency\nUSD-ASSET,2026-01-01,2026-01-31,100,110,USD\nEUR-ASSET,2026-01-01,2026-01-31,200,180,EUR\n";

        let snapshot = parse_portfolio_contribution_csv(csv, "multi.csv".to_owned()).unwrap();

        assert_eq!(snapshot.currency_totals.len(), 2);
        assert_eq!(
            snapshot.portfolio_return_label(),
            "2 CCY · SEE CONTRIBUTION"
        );
        assert!(snapshot
            .disclosures
            .iter()
            .any(|value| value.contains("NO FX CONVERSION")));
    }
}
