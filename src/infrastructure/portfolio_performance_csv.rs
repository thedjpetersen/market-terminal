use std::collections::{BTreeMap, BTreeSet};

use chrono::NaiveDate;
use csv::StringRecord;

use crate::{
    features::portfolio::{
        PortfolioError, PortfolioPerformanceSeries, PortfolioPerformanceSnapshot,
        PortfolioValuationPoint,
    },
    foundation::{Currency, Money},
};

use super::portfolio_csv::{csv_input_version, parse_currency, parse_scaled};

pub(super) const MAX_PERFORMANCE_ROWS: usize = 100_000;
pub(super) const MAX_PERFORMANCE_COLUMNS: usize = 64;
const RETURN_SCALE: i128 = 1_000_000_000;

#[derive(Debug)]
struct PerformanceColumns {
    date: usize,
    ending_value: usize,
    external_flow: Option<usize>,
    benchmark_value: Option<usize>,
    currency: Option<usize>,
}

pub(super) fn parse_portfolio_performance_csv(
    bytes: &[u8],
    source_name: String,
) -> Result<PortfolioPerformanceSnapshot, PortfolioError> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(bytes);
    let records = reader
        .records()
        .take(MAX_PERFORMANCE_ROWS + 33)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| PortfolioError::InvalidCsv(format!("CSV PARSE ERROR · {error}")))?;
    if records
        .iter()
        .any(|record| record.len() > MAX_PERFORMANCE_COLUMNS)
    {
        return Err(PortfolioError::InvalidCsv(format!(
            "PERFORMANCE CSV EXCEEDS {MAX_PERFORMANCE_COLUMNS} COLUMNS"
        )));
    }

    let (header_index, columns) = records
        .iter()
        .take(32)
        .enumerate()
        .find_map(|(index, record)| {
            PerformanceColumns::from_header(record).map(|columns| (index, columns))
        })
        .ok_or_else(|| {
            PortfolioError::InvalidCsv(
                "NO PERFORMANCE HEADER FOUND · NEED DATE AND PORTFOLIO VALUE/NAV".to_owned(),
            )
        })?;
    if records.len().saturating_sub(header_index + 1) > MAX_PERFORMANCE_ROWS {
        return Err(PortfolioError::InvalidCsv(format!(
            "PERFORMANCE CSV EXCEEDS {MAX_PERFORMANCE_ROWS} DATA ROWS"
        )));
    }

    let usd = Currency::new("USD").expect("USD is a valid currency");
    let mut grouped = BTreeMap::<Currency, Vec<(NaiveDate, PortfolioValuationPoint)>>::new();
    let mut rejected = Vec::new();
    let mut defaulted_currency = false;
    for (record_index, record) in records.iter().enumerate().skip(header_index + 1) {
        if record.iter().all(|field| field.trim().is_empty()) {
            continue;
        }
        let row = record_index + 1;
        let Some(date) = parse_date(record.get(columns.date).unwrap_or_default()) else {
            rejected.push(format!("ROW {row} DATE"));
            continue;
        };
        let currency = match columns.field(record, columns.currency) {
            Some(value) if !value.trim().is_empty() => match parse_currency(value) {
                Ok(currency) => currency,
                Err(_) => {
                    rejected.push(format!("ROW {row} CURRENCY"));
                    continue;
                }
            },
            _ => {
                defaulted_currency = true;
                usd
            }
        };
        let decimals = currency.minor_unit_digits();
        let Some(ending_minor) = parse_scaled(record.get(columns.ending_value), decimals) else {
            rejected.push(format!("ROW {row} PORTFOLIO VALUE"));
            continue;
        };
        if ending_minor <= 0 {
            rejected.push(format!("ROW {row} NON-POSITIVE PORTFOLIO VALUE"));
            continue;
        }
        let external_flow_minor = match columns.field(record, columns.external_flow) {
            Some(value) if !value.trim().is_empty() => {
                let Some(value) = parse_scaled(Some(value), decimals) else {
                    rejected.push(format!("ROW {row} EXTERNAL FLOW"));
                    continue;
                };
                value
            }
            _ => 0,
        };
        let benchmark_value = match columns.benchmark_value {
            Some(column) => {
                let Some(value) = parse_scaled(record.get(column), decimals) else {
                    rejected.push(format!("ROW {row} BENCHMARK VALUE"));
                    continue;
                };
                if value <= 0 {
                    rejected.push(format!("ROW {row} NON-POSITIVE BENCHMARK VALUE"));
                    continue;
                }
                Some(Money::from_minor_units(value, currency))
            }
            None => None,
        };
        grouped.entry(currency).or_default().push((
            date,
            PortfolioValuationPoint {
                date: date.format("%Y-%m-%d").to_string(),
                currency,
                ending_value: Money::from_minor_units(ending_minor, currency),
                external_flow: Money::from_minor_units(external_flow_minor, currency),
                benchmark_value,
            },
        ));
    }
    if !rejected.is_empty() {
        return Err(PortfolioError::InvalidCsv(format!(
            "REFUSED PARTIAL PERFORMANCE IMPORT · INVALID {}",
            rejected.into_iter().take(8).collect::<Vec<_>>().join(", ")
        )));
    }
    if grouped.is_empty() {
        return Err(PortfolioError::InvalidCsv(
            "NO DATED PORTFOLIO VALUATIONS WERE FOUND".to_owned(),
        ));
    }

    let mut first_date = None::<NaiveDate>;
    let mut last_date = None::<NaiveDate>;
    let mut series = Vec::with_capacity(grouped.len());
    for (currency, mut dated_points) in grouped {
        dated_points.sort_by_key(|(date, _)| *date);
        let unique_dates = dated_points
            .iter()
            .map(|(date, _)| *date)
            .collect::<BTreeSet<_>>();
        if unique_dates.len() != dated_points.len() {
            return Err(PortfolioError::InvalidCsv(format!(
                "DUPLICATE VALUATION DATE IN {currency} SERIES"
            )));
        }
        if dated_points.len() < 2 {
            return Err(PortfolioError::InvalidCsv(format!(
                "{currency} PERFORMANCE NEEDS AT LEAST TWO DATED VALUATIONS"
            )));
        }
        if dated_points[0].1.external_flow.minor_units() != 0 {
            return Err(PortfolioError::InvalidCsv(format!(
                "FIRST {currency} VALUATION CANNOT HAVE AN EXTERNAL FLOW"
            )));
        }
        first_date = Some(first_date.map_or(dated_points[0].0, |date| date.min(dated_points[0].0)));
        last_date = Some(
            last_date.map_or(dated_points.last().expect("two points").0, |date| {
                date.max(dated_points.last().expect("two points").0)
            }),
        );
        let points = dated_points
            .into_iter()
            .map(|(_, point)| point)
            .collect::<Vec<_>>();
        let time_weighted_return_bps = linked_return_bps(&points, false)?;
        let benchmark_return_bps = columns
            .benchmark_value
            .map(|_| linked_return_bps(&points, true))
            .transpose()?;
        let active_return_bps = benchmark_return_bps
            .map(|benchmark| {
                time_weighted_return_bps
                    .checked_sub(benchmark)
                    .ok_or_else(|| PortfolioError::InvalidCsv("ACTIVE RETURN OVERFLOW".to_owned()))
            })
            .transpose()?;
        series.push(PortfolioPerformanceSeries {
            currency,
            points,
            time_weighted_return_bps,
            benchmark_return_bps,
            active_return_bps,
        });
    }

    let mut disclosures = vec![
        "TWR LINKS SUB-PERIOD RETURNS USING END-OF-PERIOD EXTERNAL FLOWS".to_owned(),
        "VALUATIONS AND FLOWS ARE PRESERVED IN THEIR REPORTING CURRENCY".to_owned(),
        "NO POSITION CONTRIBUTION OR FACTOR ATTRIBUTION IS INFERRED".to_owned(),
    ];
    if columns.benchmark_value.is_none() {
        disclosures
            .push("NO BENCHMARK COLUMN · BENCHMARK AND ACTIVE RETURN UNAVAILABLE".to_owned());
    }
    if defaulted_currency {
        disclosures.push("MISSING CURRENCY DEFAULTED TO USD".to_owned());
    }
    if series.len() > 1 {
        disclosures.push("NO FX CONVERSION · RETURNS REMAIN SEPARATE BY CURRENCY".to_owned());
    }

    Ok(PortfolioPerformanceSnapshot {
        series,
        source: format!("CSV · {source_name}"),
        period: match (first_date, last_date) {
            (Some(first), Some(last)) => {
                format!("{} — {}", first.format("%Y-%m-%d"), last.format("%Y-%m-%d"))
            }
            _ => "—".to_owned(),
        },
        input_version: csv_input_version(bytes),
        methodology: "EXACT MONEY INPUTS · END-OF-PERIOD FLOW-ADJUSTED TWR · PER-CURRENCY LINKING"
            .to_owned(),
        disclosures,
    })
}

fn linked_return_bps(
    points: &[PortfolioValuationPoint],
    benchmark: bool,
) -> Result<i32, PortfolioError> {
    let mut linked = RETURN_SCALE;
    for pair in points.windows(2) {
        let begin = if benchmark {
            pair[0]
                .benchmark_value
                .expect("validated benchmark series")
                .minor_units()
        } else {
            pair[0].ending_value.minor_units()
        };
        let end = if benchmark {
            pair[1]
                .benchmark_value
                .expect("validated benchmark series")
                .minor_units()
        } else {
            pair[1]
                .ending_value
                .minor_units()
                .checked_sub(pair[1].external_flow.minor_units())
                .ok_or_else(|| PortfolioError::InvalidCsv("FLOW ADJUSTMENT OVERFLOW".to_owned()))?
        };
        if begin <= 0 || end <= 0 {
            return Err(PortfolioError::InvalidCsv(
                "FLOW-ADJUSTED SUB-PERIOD VALUE MUST REMAIN POSITIVE".to_owned(),
            ));
        }
        let factor = rounded_ratio(end, begin, RETURN_SCALE, "SUB-PERIOD RETURN OVERFLOW")?;
        linked = rounded_ratio(linked, RETURN_SCALE, factor, "LINKED RETURN OVERFLOW")?;
    }
    let return_scaled = linked
        .checked_sub(RETURN_SCALE)
        .ok_or_else(|| PortfolioError::InvalidCsv("RETURN OVERFLOW".to_owned()))?;
    let bps = rounded_ratio(return_scaled, RETURN_SCALE, 10_000, "RETURN OVERFLOW")?;
    i32::try_from(bps)
        .map_err(|_| PortfolioError::InvalidCsv("RETURN EXCEEDS SUPPORTED RANGE".to_owned()))
}

fn rounded_ratio(
    numerator: i128,
    denominator: i128,
    multiplier: i128,
    message: &str,
) -> Result<i128, PortfolioError> {
    let scaled = numerator
        .checked_mul(multiplier)
        .ok_or_else(|| PortfolioError::InvalidCsv(message.to_owned()))?;
    let quotient = scaled / denominator;
    let remainder = scaled % denominator;
    let rounds_away = remainder.unsigned_abs().saturating_mul(2) >= denominator.unsigned_abs();
    Ok(if rounds_away {
        quotient
            + if scaled.signum() == denominator.signum() {
                1
            } else {
                -1
            }
    } else {
        quotient
    })
}

fn parse_date(value: &str) -> Option<NaiveDate> {
    ["%Y-%m-%d", "%m/%d/%Y", "%m/%d/%y"]
        .into_iter()
        .find_map(|format| NaiveDate::parse_from_str(value.trim(), format).ok())
}

impl PerformanceColumns {
    fn from_header(header: &StringRecord) -> Option<Self> {
        let names = header.iter().map(normalize_header).collect::<Vec<_>>();
        Some(Self {
            date: find(&names, &["valuationdate", "asofdate", "date"])?,
            ending_value: find(
                &names,
                &[
                    "portfoliovalue",
                    "endingvalue",
                    "marketvalue",
                    "netassetvalue",
                    "nav",
                ],
            )?,
            external_flow: find(
                &names,
                &[
                    "externalflow",
                    "netexternalflow",
                    "netflow",
                    "cashflow",
                    "capitalflow",
                ],
            ),
            benchmark_value: find(
                &names,
                &[
                    "benchmarkvalue",
                    "benchmarklevel",
                    "indexvalue",
                    "indexlevel",
                ],
            ),
            currency: find(&names, &["reportingcurrency", "currency", "ccy"]),
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
    fn calculates_flow_adjusted_twr_and_benchmark_from_exact_values() {
        let csv = b"Date,Portfolio Value,External Flow,Benchmark Value,Currency\n2026-01-01,1000,0,100,USD\n2026-02-01,1200,100,102,USD\n2026-03-01,1210,0,104,USD\n";

        let performance =
            parse_portfolio_performance_csv(csv, "performance.csv".to_owned()).unwrap();

        assert_eq!(performance.point_count(), 3);
        assert_eq!(performance.series[0].time_weighted_return_bps, 1_092);
        assert_eq!(performance.series[0].benchmark_return_bps, Some(400));
        assert_eq!(performance.series[0].active_return_bps, Some(692));
        assert_eq!(
            performance.series[0].time_weighted_return_label(),
            "+10.92%"
        );
        assert!(performance.input_version.starts_with("CSV-FNV1A64-"));
    }

    #[test]
    fn rejects_duplicate_dates_and_partial_rows() {
        let duplicate = b"Date,NAV\n2026-01-01,100\n2026-01-01,101\n";
        let invalid = b"Date,NAV\n2026-01-01,100\nnot-a-date,101\n";

        assert!(
            parse_portfolio_performance_csv(duplicate, "duplicate.csv".to_owned())
                .unwrap_err()
                .to_string()
                .contains("DUPLICATE")
        );
        assert!(
            parse_portfolio_performance_csv(invalid, "invalid.csv".to_owned())
                .unwrap_err()
                .to_string()
                .contains("REFUSED PARTIAL")
        );
    }

    #[test]
    fn rejects_first_point_flow_and_non_positive_flow_adjusted_value() {
        let first_flow = b"Date,NAV,External Flow\n2026-01-01,100,10\n2026-02-01,110,0\n";
        let exhausted = b"Date,NAV,External Flow\n2026-01-01,100,0\n2026-02-01,10,20\n";

        assert!(
            parse_portfolio_performance_csv(first_flow, "flow.csv".to_owned())
                .unwrap_err()
                .to_string()
                .contains("FIRST USD")
        );
        assert!(
            parse_portfolio_performance_csv(exhausted, "flow.csv".to_owned())
                .unwrap_err()
                .to_string()
                .contains("MUST REMAIN POSITIVE")
        );
    }

    #[test]
    fn keeps_currencies_separate_and_discloses_a_missing_benchmark() {
        let csv = b"Date,NAV,Currency\n2026-01-01,100,USD\n2026-02-01,110,USD\n2026-01-01,200,EUR\n2026-02-01,180,EUR\n";

        let performance =
            parse_portfolio_performance_csv(csv, "multi-currency.csv".to_owned()).unwrap();

        assert_eq!(performance.series.len(), 2);
        assert_eq!(performance.time_weighted_return_label(), "2 CCY · SEE PERF");
        assert!(performance
            .series
            .iter()
            .all(|series| series.benchmark_return_bps.is_none()));
        assert!(performance
            .disclosures
            .iter()
            .any(|disclosure| disclosure.contains("NO BENCHMARK COLUMN")));
        assert!(performance
            .disclosures
            .iter()
            .any(|disclosure| disclosure.contains("NO FX CONVERSION")));
    }
}
