use std::collections::{BTreeMap, BTreeSet};

use chrono::NaiveDate;

use crate::foundation::{Currency, InstrumentId, Money};

use super::{format_money, PortfolioAccountId};

const MAX_CONTRIBUTION_ROWS: usize = 25_000;
/// One unit is one hundredth of a basis point (0.0001%).
pub(super) const CONTRIBUTION_SCALE: i128 = 1_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortfolioContributionInputRow {
    pub account_id: PortfolioAccountId,
    pub instrument_id: InstrumentId,
    pub symbol: String,
    pub currency: Currency,
    pub beginning_value: Money,
    /// Net external capital applied at the end of the period. Contributions are
    /// positive and withdrawals are negative.
    pub external_flow: Money,
    pub ending_value: Money,
    pub benchmark_beginning_value: Option<Money>,
    pub benchmark_ending_value: Option<Money>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortfolioContributionInput {
    pub rows: Vec<PortfolioContributionInputRow>,
    pub source: String,
    pub period_start: String,
    pub period_end: String,
    pub input_version: String,
    pub disclosures: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortfolioContributionRow {
    pub account_id: PortfolioAccountId,
    pub instrument_id: InstrumentId,
    pub symbol: String,
    pub currency: Currency,
    pub beginning_value: Money,
    pub external_flow: Money,
    pub ending_value: Money,
    pub gain_loss: Money,
    pub contribution_centibps: i64,
    pub benchmark_contribution_centibps: Option<i64>,
    pub active_contribution_centibps: Option<i64>,
}

impl PortfolioContributionRow {
    pub fn beginning_value_label(&self) -> String {
        format_money(self.beginning_value)
    }

    pub fn external_flow_label(&self) -> String {
        format_money(self.external_flow)
    }

    pub fn ending_value_label(&self) -> String {
        format_money(self.ending_value)
    }

    pub fn gain_loss_label(&self) -> String {
        format_money(self.gain_loss)
    }

    pub fn contribution_label(&self) -> String {
        format_centibps(self.contribution_centibps)
    }

    pub fn benchmark_contribution_label(&self) -> String {
        self.benchmark_contribution_centibps
            .map(format_centibps)
            .unwrap_or_else(|| "N/A".to_owned())
    }

    pub fn active_contribution_label(&self) -> String {
        self.active_contribution_centibps
            .map(format_centibps)
            .unwrap_or_else(|| "N/A".to_owned())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortfolioContributionCurrencyTotal {
    pub currency: Currency,
    pub positions: usize,
    pub beginning_value: Money,
    pub external_flow: Money,
    pub ending_value: Money,
    pub gain_loss: Money,
    pub portfolio_return_centibps: i64,
    pub contribution_rounding_residual_centibps: i64,
    pub benchmark_beginning_value: Option<Money>,
    pub benchmark_ending_value: Option<Money>,
    pub benchmark_return_centibps: Option<i64>,
    pub benchmark_rounding_residual_centibps: Option<i64>,
    pub active_return_centibps: Option<i64>,
    pub active_rounding_residual_centibps: Option<i64>,
}

impl PortfolioContributionCurrencyTotal {
    pub fn portfolio_return_label(&self) -> String {
        format_centibps(self.portfolio_return_centibps)
    }

    pub fn benchmark_return_label(&self) -> String {
        self.benchmark_return_centibps
            .map(format_centibps)
            .unwrap_or_else(|| "N/A".to_owned())
    }

    pub fn active_return_label(&self) -> String {
        self.active_return_centibps
            .map(format_centibps)
            .unwrap_or_else(|| "N/A".to_owned())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortfolioContributionSnapshot {
    pub rows: Vec<PortfolioContributionRow>,
    pub currency_totals: Vec<PortfolioContributionCurrencyTotal>,
    pub source: String,
    pub period: String,
    pub input_version: String,
    pub methodology: String,
    pub disclosures: Vec<String>,
}

impl PortfolioContributionSnapshot {
    pub fn empty(source: impl Into<String>) -> Self {
        Self {
            rows: Vec::new(),
            currency_totals: Vec::new(),
            source: source.into(),
            period: "—".to_owned(),
            input_version: "—".to_owned(),
            methodology: "NO VERIFIED POSITION-PERIOD INPUT".to_owned(),
            disclosures: vec![
                "IMPORT VERIFIED CONTRIBUTION HISTORY TO CALCULATE SECURITY CONTRIBUTION"
                    .to_owned(),
                "UNRELATED POSITION SNAPSHOTS ARE NOT JOINED INTO PERFORMANCE HISTORY".to_owned(),
            ],
        }
    }

    pub fn portfolio_return_label(&self) -> String {
        match self.currency_totals.as_slice() {
            [] => "N/A".to_owned(),
            [total] => total.portfolio_return_label(),
            totals => format!("{} CCY · SEE CONTRIBUTION", totals.len()),
        }
    }

    pub fn active_return_label(&self) -> String {
        match self.currency_totals.as_slice() {
            [] => "N/A".to_owned(),
            [total] => total.active_return_label(),
            totals => format!("{} CCY · SEE CONTRIBUTION", totals.len()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortfolioContributionError(String);

impl std::fmt::Display for PortfolioContributionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for PortfolioContributionError {}

#[derive(Debug, Default)]
struct CurrencyAggregate {
    beginning_minor: i128,
    flow_minor: i128,
    ending_minor: i128,
    gain_minor: i128,
    benchmark_beginning_minor: i128,
    benchmark_ending_minor: i128,
    benchmark_gain_minor: i128,
    positions: usize,
}

/// Calculates additive, single-period security contribution and optional
/// benchmark-active attribution without reading Portfolio storage.
///
/// For each row, gain/loss is `ending - external flow - beginning`. Security
/// contribution divides that gain/loss by the currency portfolio's aggregate
/// beginning value. Benchmark contribution uses the same additive construction
/// over benchmark beginning and ending values. Results never cross currencies.
pub fn calculate_contribution(
    input: PortfolioContributionInput,
) -> Result<PortfolioContributionSnapshot, PortfolioContributionError> {
    validate_input(&input)?;
    let benchmark_present = input.rows[0].benchmark_beginning_value.is_some();
    let mut aggregates = BTreeMap::<Currency, CurrencyAggregate>::new();
    let mut gains = Vec::with_capacity(input.rows.len());

    for row in &input.rows {
        validate_row(row, benchmark_present)?;
        let gain_minor = row
            .ending_value
            .minor_units()
            .checked_sub(row.external_flow.minor_units())
            .and_then(|value| value.checked_sub(row.beginning_value.minor_units()))
            .ok_or_else(|| contribution_error("position gain/loss overflow"))?;
        let benchmark_gain_minor = match (row.benchmark_beginning_value, row.benchmark_ending_value)
        {
            (Some(beginning), Some(ending)) => Some(
                ending
                    .minor_units()
                    .checked_sub(beginning.minor_units())
                    .ok_or_else(|| contribution_error("benchmark gain/loss overflow"))?,
            ),
            (None, None) => None,
            _ => unreachable!("benchmark pairing is validated"),
        };
        let aggregate = aggregates.entry(row.currency).or_default();
        aggregate.beginning_minor = checked_add(
            aggregate.beginning_minor,
            row.beginning_value.minor_units(),
            "beginning value total overflow",
        )?;
        aggregate.flow_minor = checked_add(
            aggregate.flow_minor,
            row.external_flow.minor_units(),
            "external flow total overflow",
        )?;
        aggregate.ending_minor = checked_add(
            aggregate.ending_minor,
            row.ending_value.minor_units(),
            "ending value total overflow",
        )?;
        aggregate.gain_minor =
            checked_add(aggregate.gain_minor, gain_minor, "gain/loss total overflow")?;
        if let (Some(beginning), Some(ending), Some(gain)) = (
            row.benchmark_beginning_value,
            row.benchmark_ending_value,
            benchmark_gain_minor,
        ) {
            aggregate.benchmark_beginning_minor = checked_add(
                aggregate.benchmark_beginning_minor,
                beginning.minor_units(),
                "benchmark beginning total overflow",
            )?;
            aggregate.benchmark_ending_minor = checked_add(
                aggregate.benchmark_ending_minor,
                ending.minor_units(),
                "benchmark ending total overflow",
            )?;
            aggregate.benchmark_gain_minor = checked_add(
                aggregate.benchmark_gain_minor,
                gain,
                "benchmark gain/loss total overflow",
            )?;
        }
        aggregate.positions += 1;
        gains.push((gain_minor, benchmark_gain_minor));
    }

    for (currency, aggregate) in &aggregates {
        if aggregate.beginning_minor <= 0 {
            return Err(contribution_error(format!(
                "{currency} aggregate beginning value must be positive"
            )));
        }
        let reconciled_gain = aggregate
            .ending_minor
            .checked_sub(aggregate.flow_minor)
            .and_then(|value| value.checked_sub(aggregate.beginning_minor))
            .ok_or_else(|| contribution_error("currency reconciliation overflow"))?;
        if reconciled_gain != aggregate.gain_minor {
            return Err(contribution_error(format!(
                "{currency} position gains do not reconcile"
            )));
        }
        if benchmark_present && aggregate.benchmark_beginning_minor <= 0 {
            return Err(contribution_error(format!(
                "{currency} aggregate benchmark beginning value must be positive"
            )));
        }
        if benchmark_present {
            let reconciled_benchmark_gain = aggregate
                .benchmark_ending_minor
                .checked_sub(aggregate.benchmark_beginning_minor)
                .ok_or_else(|| contribution_error("benchmark reconciliation overflow"))?;
            if reconciled_benchmark_gain != aggregate.benchmark_gain_minor {
                return Err(contribution_error(format!(
                    "{currency} benchmark gains do not reconcile"
                )));
            }
        }
    }

    let mut contribution_sums = BTreeMap::<Currency, i64>::new();
    let mut benchmark_sums = BTreeMap::<Currency, i64>::new();
    let mut rows = input
        .rows
        .into_iter()
        .zip(gains)
        .map(|(row, (gain_minor, benchmark_gain_minor))| {
            let aggregate = aggregates.get(&row.currency).expect("known currency");
            let contribution_centibps = ratio_centibps(
                gain_minor,
                aggregate.beginning_minor,
                "position contribution exceeds its typed range",
            )?;
            let contribution_sum = contribution_sums.entry(row.currency).or_default();
            *contribution_sum = checked_add_i64(
                *contribution_sum,
                contribution_centibps,
                "contribution sum overflow",
            )?;
            let benchmark_contribution_centibps = benchmark_gain_minor
                .map(|gain| {
                    ratio_centibps(
                        gain,
                        aggregate.benchmark_beginning_minor,
                        "benchmark contribution exceeds its typed range",
                    )
                })
                .transpose()?;
            if let Some(value) = benchmark_contribution_centibps {
                let benchmark_sum = benchmark_sums.entry(row.currency).or_default();
                *benchmark_sum =
                    checked_add_i64(*benchmark_sum, value, "benchmark contribution sum overflow")?;
            }
            let active_contribution_centibps = benchmark_contribution_centibps
                .map(|benchmark| {
                    contribution_centibps
                        .checked_sub(benchmark)
                        .ok_or_else(|| contribution_error("active contribution overflow"))
                })
                .transpose()?;
            Ok(PortfolioContributionRow {
                account_id: row.account_id,
                instrument_id: row.instrument_id,
                symbol: row.symbol,
                currency: row.currency,
                beginning_value: row.beginning_value,
                external_flow: row.external_flow,
                ending_value: row.ending_value,
                gain_loss: Money::from_minor_units(gain_minor, row.currency),
                contribution_centibps,
                benchmark_contribution_centibps,
                active_contribution_centibps,
            })
        })
        .collect::<Result<Vec<_>, PortfolioContributionError>>()?;

    rows.sort_by(|left, right| {
        left.currency
            .cmp(&right.currency)
            .then_with(|| {
                right
                    .contribution_centibps
                    .unsigned_abs()
                    .cmp(&left.contribution_centibps.unsigned_abs())
            })
            .then_with(|| left.symbol.cmp(&right.symbol))
            .then_with(|| left.account_id.cmp(&right.account_id))
    });

    let mut currency_totals = Vec::with_capacity(aggregates.len());
    for (currency, aggregate) in aggregates {
        let portfolio_return_centibps = ratio_centibps(
            aggregate.gain_minor,
            aggregate.beginning_minor,
            "portfolio return exceeds its typed range",
        )?;
        let contribution_sum = contribution_sums
            .get(&currency)
            .copied()
            .unwrap_or_default();
        let contribution_rounding_residual_centibps = portfolio_return_centibps
            .checked_sub(contribution_sum)
            .ok_or_else(|| contribution_error("contribution rounding residual overflow"))?;
        let benchmark_return_centibps = benchmark_present
            .then(|| {
                ratio_centibps(
                    aggregate.benchmark_gain_minor,
                    aggregate.benchmark_beginning_minor,
                    "benchmark return exceeds its typed range",
                )
            })
            .transpose()?;
        let benchmark_rounding_residual_centibps = benchmark_return_centibps
            .map(|benchmark_return| {
                benchmark_return
                    .checked_sub(benchmark_sums.get(&currency).copied().unwrap_or_default())
                    .ok_or_else(|| contribution_error("benchmark rounding residual overflow"))
            })
            .transpose()?;
        let active_return_centibps = benchmark_return_centibps
            .map(|benchmark_return| {
                portfolio_return_centibps
                    .checked_sub(benchmark_return)
                    .ok_or_else(|| contribution_error("active return overflow"))
            })
            .transpose()?;
        let active_rounding_residual_centibps =
            match (benchmark_rounding_residual_centibps, active_return_centibps) {
                (Some(benchmark_residual), Some(_)) => Some(
                    contribution_rounding_residual_centibps
                        .checked_sub(benchmark_residual)
                        .ok_or_else(|| contribution_error("active rounding residual overflow"))?,
                ),
                _ => None,
            };
        currency_totals.push(PortfolioContributionCurrencyTotal {
            currency,
            positions: aggregate.positions,
            beginning_value: Money::from_minor_units(aggregate.beginning_minor, currency),
            external_flow: Money::from_minor_units(aggregate.flow_minor, currency),
            ending_value: Money::from_minor_units(aggregate.ending_minor, currency),
            gain_loss: Money::from_minor_units(aggregate.gain_minor, currency),
            portfolio_return_centibps,
            contribution_rounding_residual_centibps,
            benchmark_beginning_value: benchmark_present
                .then(|| Money::from_minor_units(aggregate.benchmark_beginning_minor, currency)),
            benchmark_ending_value: benchmark_present
                .then(|| Money::from_minor_units(aggregate.benchmark_ending_minor, currency)),
            benchmark_return_centibps,
            benchmark_rounding_residual_centibps,
            active_return_centibps,
            active_rounding_residual_centibps,
        });
    }

    let mut disclosures = input.disclosures;
    disclosures.extend([
        "GAIN/LOSS = ENDING VALUE − END-OF-PERIOD EXTERNAL FLOW − BEGINNING VALUE".to_owned(),
        "SECURITY CONTRIBUTION DIVIDES GAIN/LOSS BY PER-CURRENCY BEGINNING VALUE".to_owned(),
        "RESULTS USE EXACT MONEY INPUTS AND DISCLOSE CENTIBASIS-POINT ROUNDING RESIDUALS"
            .to_owned(),
        "SINGLE-PERIOD CONTRIBUTION IS NOT MULTI-PERIOD TWR ATTRIBUTION".to_owned(),
    ]);
    if benchmark_present {
        disclosures.push(
            "ACTIVE CONTRIBUTION = PORTFOLIO CONTRIBUTION − BENCHMARK CONTRIBUTION".to_owned(),
        );
    } else {
        disclosures.push("NO BENCHMARK INPUT · ACTIVE ATTRIBUTION UNAVAILABLE".to_owned());
    }
    if currency_totals.len() > 1 {
        disclosures.push("NO FX CONVERSION · CONTRIBUTION REMAINS SEPARATE BY CURRENCY".to_owned());
    }

    Ok(PortfolioContributionSnapshot {
        rows,
        currency_totals,
        source: input.source,
        period: format!("{} — {}", input.period_start, input.period_end),
        input_version: input.input_version,
        methodology: "SINGLE-PERIOD ADDITIVE SECURITY CONTRIBUTION · OPTIONAL BENCHMARK-ACTIVE ATTRIBUTION · PER-CURRENCY"
            .to_owned(),
        disclosures,
    })
}

fn validate_input(input: &PortfolioContributionInput) -> Result<(), PortfolioContributionError> {
    if input.rows.is_empty() {
        return Err(contribution_error("contribution input has no rows"));
    }
    if input.rows.len() > MAX_CONTRIBUTION_ROWS {
        return Err(contribution_error(format!(
            "contribution input exceeds {MAX_CONTRIBUTION_ROWS} rows"
        )));
    }
    for (field, value) in [
        ("source", input.source.as_str()),
        ("input version", input.input_version.as_str()),
    ] {
        if value.trim().is_empty() || value.len() > 1_024 {
            return Err(contribution_error(format!("{field} is empty or too long")));
        }
    }
    let start = parse_iso_date(&input.period_start, "period start")?;
    let end = parse_iso_date(&input.period_end, "period end")?;
    if start >= end {
        return Err(contribution_error(
            "contribution period start must precede period end",
        ));
    }
    let benchmark_present = input.rows[0].benchmark_beginning_value.is_some();
    let mut identities = BTreeSet::new();
    for row in &input.rows {
        if row.benchmark_beginning_value.is_some() != benchmark_present
            || row.benchmark_ending_value.is_some() != benchmark_present
        {
            return Err(contribution_error(
                "benchmark coverage must be complete for every contribution row",
            ));
        }
        if !identities.insert((row.currency, row.account_id.clone(), row.symbol.clone())) {
            return Err(contribution_error(format!(
                "duplicate contribution identity for {} · {} · {}",
                row.account_id.as_str(),
                row.symbol,
                row.currency
            )));
        }
    }
    if input.disclosures.len() > 128
        || input
            .disclosures
            .iter()
            .any(|value| value.trim().is_empty() || value.len() > 1_024)
    {
        return Err(contribution_error(
            "contribution disclosures are empty, too long, or exceed 128 entries",
        ));
    }
    Ok(())
}

fn validate_row(
    row: &PortfolioContributionInputRow,
    benchmark_present: bool,
) -> Result<(), PortfolioContributionError> {
    if row.symbol.trim().is_empty() || row.symbol.len() > 64 || row.account_id.as_str().len() > 128
    {
        return Err(contribution_error(
            "position account or symbol is empty or too long",
        ));
    }
    for (field, value) in [
        ("beginning value", row.beginning_value),
        ("external flow", row.external_flow),
        ("ending value", row.ending_value),
    ] {
        if value.currency() != row.currency {
            return Err(contribution_error(format!(
                "{} {field} contains a currency mismatch",
                row.symbol
            )));
        }
    }
    if row.beginning_value.minor_units() < 0 || row.ending_value.minor_units() < 0 {
        return Err(contribution_error(format!(
            "{} beginning and ending values cannot be negative",
            row.symbol
        )));
    }
    match (
        row.benchmark_beginning_value,
        row.benchmark_ending_value,
        benchmark_present,
    ) {
        (Some(beginning), Some(ending), true) => {
            if beginning.currency() != row.currency || ending.currency() != row.currency {
                return Err(contribution_error(format!(
                    "{} benchmark values contain a currency mismatch",
                    row.symbol
                )));
            }
            if beginning.minor_units() < 0 || ending.minor_units() < 0 {
                return Err(contribution_error(format!(
                    "{} benchmark values cannot be negative",
                    row.symbol
                )));
            }
        }
        (None, None, false) => {}
        _ => {
            return Err(contribution_error(
                "benchmark beginning and ending values must be supplied together",
            ));
        }
    }
    Ok(())
}

fn parse_iso_date(value: &str, field: &str) -> Result<NaiveDate, PortfolioContributionError> {
    NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d")
        .map_err(|_| contribution_error(format!("{field} must use YYYY-MM-DD")))
}

fn ratio_centibps(
    numerator: i128,
    denominator: i128,
    message: &str,
) -> Result<i64, PortfolioContributionError> {
    if denominator <= 0 {
        return Err(contribution_error(
            "contribution denominator must be positive",
        ));
    }
    let scaled = numerator
        .checked_mul(CONTRIBUTION_SCALE)
        .ok_or_else(|| contribution_error(message))?;
    let quotient = scaled / denominator;
    let remainder = scaled % denominator;
    let rounds_away = remainder.unsigned_abs().saturating_mul(2) >= denominator.unsigned_abs();
    let rounded = if rounds_away {
        quotient
            .checked_add(if scaled.signum() == denominator.signum() {
                1
            } else {
                -1
            })
            .ok_or_else(|| contribution_error(message))?
    } else {
        quotient
    };
    i64::try_from(rounded).map_err(|_| contribution_error(message))
}

fn checked_add(left: i128, right: i128, message: &str) -> Result<i128, PortfolioContributionError> {
    left.checked_add(right)
        .ok_or_else(|| contribution_error(message))
}

fn checked_add_i64(
    left: i64,
    right: i64,
    message: &str,
) -> Result<i64, PortfolioContributionError> {
    left.checked_add(right)
        .ok_or_else(|| contribution_error(message))
}

pub(super) fn format_centibps(value: i64) -> String {
    let sign = if value > 0 {
        "+"
    } else if value < 0 {
        "-"
    } else {
        ""
    };
    let absolute = value.unsigned_abs();
    format!("{sign}{}.{:04}%", absolute / 10_000, absolute % 10_000)
}

fn contribution_error(message: impl Into<String>) -> PortfolioContributionError {
    PortfolioContributionError(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usd() -> Currency {
        Currency::new("USD").unwrap()
    }

    fn eur() -> Currency {
        Currency::new("EUR").unwrap()
    }

    fn money(value: i128, currency: Currency) -> Money {
        Money::from_minor_units(value, currency)
    }

    fn row(
        account: &str,
        symbol: &str,
        currency: Currency,
        beginning: i128,
        flow: i128,
        ending: i128,
        benchmark: Option<(i128, i128)>,
    ) -> PortfolioContributionInputRow {
        PortfolioContributionInputRow {
            account_id: PortfolioAccountId::new(account),
            instrument_id: InstrumentId::new(format!(
                "test:instrument:{}",
                symbol.to_ascii_lowercase()
            )),
            symbol: symbol.to_owned(),
            currency,
            beginning_value: money(beginning, currency),
            external_flow: money(flow, currency),
            ending_value: money(ending, currency),
            benchmark_beginning_value: benchmark.map(|(beginning, _)| money(beginning, currency)),
            benchmark_ending_value: benchmark.map(|(_, ending)| money(ending, currency)),
        }
    }

    fn input(rows: Vec<PortfolioContributionInputRow>) -> PortfolioContributionInput {
        PortfolioContributionInput {
            rows,
            source: "TEST FIXTURE".to_owned(),
            period_start: "2026-01-01".to_owned(),
            period_end: "2026-01-31".to_owned(),
            input_version: "TEST-V1".to_owned(),
            disclosures: vec!["TEST DATA".to_owned()],
        }
    }

    #[test]
    fn calculates_exact_additive_contribution_and_active_attribution() {
        let snapshot = calculate_contribution(input(vec![
            row(
                "ACCOUNT 1",
                "ALPHA",
                usd(),
                60_000,
                0,
                66_000,
                Some((50_000, 52_000)),
            ),
            row(
                "ACCOUNT 1",
                "BETA",
                usd(),
                40_000,
                10_000,
                45_000,
                Some((50_000, 51_000)),
            ),
        ]))
        .unwrap();

        assert_eq!(snapshot.rows[0].symbol, "ALPHA");
        assert_eq!(snapshot.rows[0].gain_loss.minor_units(), 6_000);
        assert_eq!(snapshot.rows[0].contribution_label(), "+6.0000%");
        assert_eq!(snapshot.rows[0].benchmark_contribution_label(), "+2.0000%");
        assert_eq!(snapshot.rows[0].active_contribution_label(), "+4.0000%");
        assert_eq!(snapshot.rows[1].gain_loss.minor_units(), -5_000);
        assert_eq!(snapshot.rows[1].contribution_label(), "-5.0000%");
        let total = &snapshot.currency_totals[0];
        assert_eq!(total.beginning_value.minor_units(), 100_000);
        assert_eq!(total.external_flow.minor_units(), 10_000);
        assert_eq!(total.ending_value.minor_units(), 111_000);
        assert_eq!(total.gain_loss.minor_units(), 1_000);
        assert_eq!(total.portfolio_return_label(), "+1.0000%");
        assert_eq!(total.benchmark_return_label(), "+3.0000%");
        assert_eq!(total.active_return_label(), "-2.0000%");
        assert_eq!(total.contribution_rounding_residual_centibps, 0);
        assert_eq!(total.benchmark_rounding_residual_centibps, Some(0));
        assert_eq!(total.active_rounding_residual_centibps, Some(0));
    }

    #[test]
    fn keeps_currencies_separate_and_discloses_missing_benchmark() {
        let snapshot = calculate_contribution(input(vec![
            row("ACCOUNT 1", "ALPHA", usd(), 10_000, 0, 11_000, None),
            row("ACCOUNT 2", "BETA", eur(), 20_000, 0, 18_000, None),
        ]))
        .unwrap();

        assert_eq!(snapshot.currency_totals.len(), 2);
        assert_eq!(
            snapshot.currency_totals[0].portfolio_return_label(),
            "-10.0000%"
        );
        assert_eq!(
            snapshot.currency_totals[1].portfolio_return_label(),
            "+10.0000%"
        );
        assert_eq!(
            snapshot.portfolio_return_label(),
            "2 CCY · SEE CONTRIBUTION"
        );
        assert!(snapshot
            .disclosures
            .iter()
            .any(|value| value.contains("NO FX CONVERSION")));
        assert!(snapshot
            .disclosures
            .iter()
            .any(|value| value.contains("ACTIVE ATTRIBUTION UNAVAILABLE")));
    }

    #[test]
    fn reports_rounding_residuals_instead_of_hiding_them() {
        let snapshot = calculate_contribution(input(vec![
            row("ACCOUNT 1", "A", usd(), 1, 0, 2, None),
            row("ACCOUNT 1", "B", usd(), 2, 0, 3, None),
            row("ACCOUNT 1", "C", usd(), 3, 0, 4, None),
        ]))
        .unwrap();

        let total = &snapshot.currency_totals[0];
        assert_eq!(total.portfolio_return_centibps, 500_000);
        assert_eq!(total.contribution_rounding_residual_centibps, -1);
    }

    #[test]
    fn rejects_partial_benchmark_duplicate_identity_and_currency_mismatch() {
        let mut partial = input(vec![
            row("ACCOUNT 1", "A", usd(), 100, 0, 110, Some((100, 105))),
            row("ACCOUNT 1", "B", usd(), 100, 0, 110, None),
        ]);
        assert!(calculate_contribution(partial.clone())
            .unwrap_err()
            .to_string()
            .contains("benchmark coverage"));

        partial.rows = vec![
            row("ACCOUNT 1", "A", usd(), 100, 0, 110, None),
            row("ACCOUNT 1", "A", usd(), 100, 0, 110, None),
        ];
        assert!(calculate_contribution(partial)
            .unwrap_err()
            .to_string()
            .contains("duplicate contribution identity"));

        let mut mismatch = row("ACCOUNT 1", "A", usd(), 100, 0, 110, None);
        mismatch.ending_value = money(110, eur());
        assert!(calculate_contribution(input(vec![mismatch]))
            .unwrap_err()
            .to_string()
            .contains("currency mismatch"));
    }

    #[test]
    fn rejects_non_positive_denominators_and_invalid_periods() {
        assert!(calculate_contribution(input(vec![row(
            "ACCOUNT 1",
            "A",
            usd(),
            0,
            100,
            100,
            None,
        )]))
        .unwrap_err()
        .to_string()
        .contains("beginning value must be positive"));

        let mut invalid_period = input(vec![row("ACCOUNT 1", "A", usd(), 100, 0, 110, None)]);
        invalid_period.period_end = invalid_period.period_start.clone();
        assert!(calculate_contribution(invalid_period)
            .unwrap_err()
            .to_string()
            .contains("must precede"));
    }
}
