use std::collections::{BTreeMap, BTreeSet};

use chrono::NaiveDate;

use crate::foundation::{Currency, InstrumentId};

use super::contribution::{
    calculate_contribution, format_centibps, PortfolioContributionInput,
    PortfolioContributionSnapshot, CONTRIBUTION_SCALE,
};
use super::PortfolioAccountId;

const MAX_ATTRIBUTION_PERIODS: usize = 3_660;
const MAX_ATTRIBUTION_ROWS: usize = 250_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortfolioAttributionInput {
    pub periods: Vec<PortfolioContributionInput>,
    pub source: String,
    pub input_version: String,
    pub disclosures: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortfolioAttributionRow {
    pub account_id: PortfolioAccountId,
    pub instrument_id: InstrumentId,
    pub symbol: String,
    pub currency: Currency,
    pub periods_present: usize,
    pub linked_contribution_centibps: i64,
    pub linked_benchmark_contribution_centibps: Option<i64>,
    pub linked_active_contribution_centibps: Option<i64>,
}

impl PortfolioAttributionRow {
    pub fn linked_contribution_label(&self) -> String {
        format_centibps(self.linked_contribution_centibps)
    }

    pub fn linked_benchmark_contribution_label(&self) -> String {
        self.linked_benchmark_contribution_centibps
            .map(format_centibps)
            .unwrap_or_else(|| "N/A".to_owned())
    }

    pub fn linked_active_contribution_label(&self) -> String {
        self.linked_active_contribution_centibps
            .map(format_centibps)
            .unwrap_or_else(|| "N/A".to_owned())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortfolioAttributionCurrencyTotal {
    pub currency: Currency,
    pub periods: usize,
    pub securities: usize,
    pub linked_return_centibps: i64,
    pub contribution_rounding_residual_centibps: i64,
    pub linked_benchmark_return_centibps: Option<i64>,
    pub benchmark_rounding_residual_centibps: Option<i64>,
    pub linked_active_return_centibps: Option<i64>,
    pub active_rounding_residual_centibps: Option<i64>,
}

impl PortfolioAttributionCurrencyTotal {
    pub fn linked_return_label(&self) -> String {
        format_centibps(self.linked_return_centibps)
    }

    pub fn linked_benchmark_return_label(&self) -> String {
        self.linked_benchmark_return_centibps
            .map(format_centibps)
            .unwrap_or_else(|| "N/A".to_owned())
    }

    pub fn linked_active_return_label(&self) -> String {
        self.linked_active_return_centibps
            .map(format_centibps)
            .unwrap_or_else(|| "N/A".to_owned())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortfolioAttributionSnapshot {
    pub rows: Vec<PortfolioAttributionRow>,
    pub currency_totals: Vec<PortfolioAttributionCurrencyTotal>,
    pub source: String,
    pub period: String,
    pub input_version: String,
    pub methodology: String,
    pub disclosures: Vec<String>,
}

impl PortfolioAttributionSnapshot {
    pub fn empty(source: impl Into<String>) -> Self {
        Self {
            rows: Vec::new(),
            currency_totals: Vec::new(),
            source: source.into(),
            period: "—".to_owned(),
            input_version: "—".to_owned(),
            methodology: "NO VERIFIED MULTI-PERIOD POSITION HISTORY".to_owned(),
            disclosures: vec![
                "IMPORT VERIFIED ATTRIBUTION HISTORY TO LINK SECURITY CONTRIBUTIONS".to_owned(),
                "UNRELATED SINGLE-PERIOD EXPORTS ARE NOT SILENTLY JOINED".to_owned(),
            ],
        }
    }

    pub fn linked_return_label(&self) -> String {
        match self.currency_totals.as_slice() {
            [] => "N/A".to_owned(),
            [total] => total.linked_return_label(),
            totals => format!("{} CCY · SEE ATTRIBUTION", totals.len()),
        }
    }

    pub fn linked_active_return_label(&self) -> String {
        match self.currency_totals.as_slice() {
            [] => "N/A".to_owned(),
            [total] => total.linked_active_return_label(),
            totals => format!("{} CCY · SEE ATTRIBUTION", totals.len()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortfolioAttributionError(String);

impl std::fmt::Display for PortfolioAttributionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for PortfolioAttributionError {}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct AttributionIdentity {
    currency: Currency,
    account_id: PortfolioAccountId,
    symbol: String,
}

#[derive(Debug)]
struct AttributionAccumulator {
    instrument_id: InstrumentId,
    periods_present: usize,
    linked_contribution: i128,
    linked_benchmark_contribution: Option<i128>,
}

/// Links verified single-period security contribution with the order-dependent
/// Frongello method, without reading Portfolio storage.
///
/// Each period contribution is scaled by the portfolio growth accumulated
/// before that period. Benchmark contributions are linked independently by
/// prior benchmark growth; linked active contribution is their difference.
/// Periods must be ordered, contiguous, and reconcile ending to next beginning
/// value within each currency. Results never cross currencies.
pub fn calculate_multi_period_attribution(
    input: PortfolioAttributionInput,
) -> Result<PortfolioAttributionSnapshot, PortfolioAttributionError> {
    validate_attribution_input(&input)?;
    let dates = input
        .periods
        .iter()
        .map(|period| {
            Ok((
                parse_iso_date(&period.period_start, "period start")?,
                parse_iso_date(&period.period_end, "period end")?,
            ))
        })
        .collect::<Result<Vec<_>, PortfolioAttributionError>>()?;
    validate_period_sequence(&dates)?;

    let snapshots = input
        .periods
        .into_iter()
        .enumerate()
        .map(|(index, period)| {
            calculate_contribution(period).map_err(|error| {
                attribution_error(format!("attribution period {}: {error}", index + 1))
            })
        })
        .collect::<Result<Vec<_>, PortfolioAttributionError>>()?;
    let benchmark_present = snapshots[0].rows[0]
        .benchmark_contribution_centibps
        .is_some();
    validate_snapshot_sequence(&snapshots, benchmark_present)?;

    let currencies = snapshots[0]
        .currency_totals
        .iter()
        .map(|total| total.currency)
        .collect::<Vec<_>>();
    let mut portfolio_factors = currencies
        .iter()
        .copied()
        .map(|currency| (currency, CONTRIBUTION_SCALE))
        .collect::<BTreeMap<_, _>>();
    let mut benchmark_factors = benchmark_present.then(|| portfolio_factors.clone());
    let mut accumulators = BTreeMap::<AttributionIdentity, AttributionAccumulator>::new();

    for snapshot in &snapshots {
        for row in &snapshot.rows {
            let identity = AttributionIdentity {
                currency: row.currency,
                account_id: row.account_id.clone(),
                symbol: row.symbol.clone(),
            };
            let portfolio_factor = portfolio_factors
                .get(&row.currency)
                .copied()
                .expect("validated currency");
            let linked_contribution = multiply_scaled(
                i128::from(row.contribution_centibps),
                portfolio_factor,
                "linked contribution overflow",
            )?;
            let linked_benchmark_contribution = match (
                row.benchmark_contribution_centibps,
                benchmark_factors.as_ref(),
            ) {
                (Some(contribution), Some(factors)) => Some(multiply_scaled(
                    i128::from(contribution),
                    factors
                        .get(&row.currency)
                        .copied()
                        .expect("validated benchmark currency"),
                    "linked benchmark contribution overflow",
                )?),
                (None, None) => None,
                _ => unreachable!("benchmark coverage is validated"),
            };
            let accumulator =
                accumulators
                    .entry(identity)
                    .or_insert_with(|| AttributionAccumulator {
                        instrument_id: row.instrument_id.clone(),
                        periods_present: 0,
                        linked_contribution: 0,
                        linked_benchmark_contribution: benchmark_present.then_some(0),
                    });
            if accumulator.instrument_id != row.instrument_id {
                return Err(attribution_error(format!(
                    "{} changes instrument identity across attribution periods",
                    row.symbol
                )));
            }
            accumulator.periods_present += 1;
            accumulator.linked_contribution = checked_add(
                accumulator.linked_contribution,
                linked_contribution,
                "linked contribution sum overflow",
            )?;
            accumulator.linked_benchmark_contribution = match (
                accumulator.linked_benchmark_contribution,
                linked_benchmark_contribution,
            ) {
                (Some(total), Some(value)) => Some(checked_add(
                    total,
                    value,
                    "linked benchmark contribution sum overflow",
                )?),
                (None, None) => None,
                _ => unreachable!("benchmark coverage is validated"),
            };
        }

        for total in &snapshot.currency_totals {
            let growth = growth_factor(total.portfolio_return_centibps, "portfolio")?;
            let factor = portfolio_factors
                .get_mut(&total.currency)
                .expect("validated currency");
            *factor = multiply_scaled(*factor, growth, "linked portfolio return overflow")?;
            if let (Some(benchmark_return), Some(factors)) =
                (total.benchmark_return_centibps, benchmark_factors.as_mut())
            {
                let benchmark_growth = growth_factor(benchmark_return, "benchmark")?;
                let benchmark_factor = factors
                    .get_mut(&total.currency)
                    .expect("validated benchmark currency");
                *benchmark_factor = multiply_scaled(
                    *benchmark_factor,
                    benchmark_growth,
                    "linked benchmark return overflow",
                )?;
            }
        }
    }

    let mut rows = accumulators
        .into_iter()
        .map(|(identity, accumulator)| {
            let linked_contribution_centibps = to_i64(
                accumulator.linked_contribution,
                "linked contribution exceeds its typed range",
            )?;
            let linked_benchmark_contribution_centibps = accumulator
                .linked_benchmark_contribution
                .map(|value| {
                    to_i64(
                        value,
                        "linked benchmark contribution exceeds its typed range",
                    )
                })
                .transpose()?;
            let linked_active_contribution_centibps = linked_benchmark_contribution_centibps
                .map(|benchmark| {
                    linked_contribution_centibps
                        .checked_sub(benchmark)
                        .ok_or_else(|| attribution_error("linked active contribution overflow"))
                })
                .transpose()?;
            Ok(PortfolioAttributionRow {
                account_id: identity.account_id,
                instrument_id: accumulator.instrument_id,
                symbol: identity.symbol,
                currency: identity.currency,
                periods_present: accumulator.periods_present,
                linked_contribution_centibps,
                linked_benchmark_contribution_centibps,
                linked_active_contribution_centibps,
            })
        })
        .collect::<Result<Vec<_>, PortfolioAttributionError>>()?;
    rows.sort_by(|left, right| {
        left.currency
            .cmp(&right.currency)
            .then_with(|| {
                right
                    .linked_contribution_centibps
                    .unsigned_abs()
                    .cmp(&left.linked_contribution_centibps.unsigned_abs())
            })
            .then_with(|| left.symbol.cmp(&right.symbol))
            .then_with(|| left.account_id.cmp(&right.account_id))
    });

    let mut currency_totals = Vec::with_capacity(currencies.len());
    for currency in currencies {
        let linked_return_centibps = factor_return(
            *portfolio_factors.get(&currency).expect("known currency"),
            "linked portfolio return exceeds its typed range",
        )?;
        let contribution_sum = sum_rows(&rows, currency, |row| {
            Some(row.linked_contribution_centibps)
        })?;
        let contribution_rounding_residual_centibps = linked_return_centibps
            .checked_sub(contribution_sum)
            .ok_or_else(|| attribution_error("linked contribution residual overflow"))?;
        let linked_benchmark_return_centibps = benchmark_factors
            .as_ref()
            .map(|factors| {
                factor_return(
                    *factors.get(&currency).expect("known benchmark currency"),
                    "linked benchmark return exceeds its typed range",
                )
            })
            .transpose()?;
        let benchmark_sum = benchmark_present
            .then(|| {
                sum_rows(&rows, currency, |row| {
                    row.linked_benchmark_contribution_centibps
                })
            })
            .transpose()?;
        let benchmark_rounding_residual_centibps =
            match (linked_benchmark_return_centibps, benchmark_sum) {
                (Some(linked_return), Some(sum)) => Some(
                    linked_return
                        .checked_sub(sum)
                        .ok_or_else(|| attribution_error("linked benchmark residual overflow"))?,
                ),
                (None, None) => None,
                _ => unreachable!("benchmark coverage is validated"),
            };
        let linked_active_return_centibps = linked_benchmark_return_centibps
            .map(|benchmark_return| {
                linked_return_centibps
                    .checked_sub(benchmark_return)
                    .ok_or_else(|| attribution_error("linked active return overflow"))
            })
            .transpose()?;
        let active_sum = benchmark_present
            .then(|| {
                sum_rows(&rows, currency, |row| {
                    row.linked_active_contribution_centibps
                })
            })
            .transpose()?;
        let active_rounding_residual_centibps = match (linked_active_return_centibps, active_sum) {
            (Some(linked_return), Some(sum)) => Some(
                linked_return
                    .checked_sub(sum)
                    .ok_or_else(|| attribution_error("linked active residual overflow"))?,
            ),
            (None, None) => None,
            _ => unreachable!("benchmark coverage is validated"),
        };
        currency_totals.push(PortfolioAttributionCurrencyTotal {
            currency,
            periods: snapshots.len(),
            securities: rows.iter().filter(|row| row.currency == currency).count(),
            linked_return_centibps,
            contribution_rounding_residual_centibps,
            linked_benchmark_return_centibps,
            benchmark_rounding_residual_centibps,
            linked_active_return_centibps,
            active_rounding_residual_centibps,
        });
    }

    let mut disclosures = input.disclosures;
    disclosures.extend([
        "FRONGELLO LINKING SCALES EACH PERIOD CONTRIBUTION BY CUMULATIVE PRIOR PORTFOLIO GROWTH"
            .to_owned(),
        "PERIODS ARE ORDERED, CONTIGUOUS, AND ENDING VALUES RECONCILE TO NEXT BEGINNING VALUES"
            .to_owned(),
        "LINKED RETURNS COMPOUND PERIOD RETURNS GEOMETRICALLY WITH ORDER-DEPENDENT CONTRIBUTIONS"
            .to_owned(),
        "CENTIBASIS-POINT INPUT ROUNDING AND LINKING RESIDUALS ARE DISCLOSED, NOT ALLOCATED"
            .to_owned(),
    ]);
    if benchmark_present {
        disclosures.push(
            "BENCHMARK CONTRIBUTIONS LINK ON PRIOR BENCHMARK GROWTH · ACTIVE IS LINKED PORTFOLIO MINUS LINKED BENCHMARK"
                .to_owned(),
        );
    } else {
        disclosures.push("NO BENCHMARK INPUT · LINKED ACTIVE ATTRIBUTION UNAVAILABLE".to_owned());
    }
    if currency_totals.len() > 1 {
        disclosures.push("NO FX CONVERSION · ATTRIBUTION REMAINS SEPARATE BY CURRENCY".to_owned());
    }

    Ok(PortfolioAttributionSnapshot {
        rows,
        currency_totals,
        source: input.source,
        period: format!(
            "{} — {}",
            dates.first().expect("validated periods").0,
            dates.last().expect("validated periods").1
        ),
        input_version: input.input_version,
        methodology:
            "MULTI-PERIOD FRONGELLO-LINKED SECURITY CONTRIBUTION · OPTIONAL BENCHMARK-ACTIVE ATTRIBUTION · PER-CURRENCY"
                .to_owned(),
        disclosures,
    })
}

fn validate_attribution_input(
    input: &PortfolioAttributionInput,
) -> Result<(), PortfolioAttributionError> {
    if input.periods.len() < 2 {
        return Err(attribution_error(
            "multi-period attribution requires at least two periods",
        ));
    }
    if input.periods.len() > MAX_ATTRIBUTION_PERIODS {
        return Err(attribution_error(format!(
            "multi-period attribution exceeds {MAX_ATTRIBUTION_PERIODS} periods"
        )));
    }
    let total_rows = input.periods.iter().try_fold(0_usize, |total, period| {
        total
            .checked_add(period.rows.len())
            .ok_or_else(|| attribution_error("attribution row count overflow"))
    })?;
    if total_rows > MAX_ATTRIBUTION_ROWS {
        return Err(attribution_error(format!(
            "multi-period attribution exceeds {MAX_ATTRIBUTION_ROWS} rows"
        )));
    }
    for (field, value) in [
        ("source", input.source.as_str()),
        ("input version", input.input_version.as_str()),
    ] {
        if value.trim().is_empty() || value.len() > 1_024 {
            return Err(attribution_error(format!("{field} is empty or too long")));
        }
    }
    if input.disclosures.len() > 128
        || input
            .disclosures
            .iter()
            .any(|value| value.trim().is_empty() || value.len() > 1_024)
    {
        return Err(attribution_error(
            "attribution disclosures are empty, too long, or exceed 128 entries",
        ));
    }
    Ok(())
}

fn validate_period_sequence(
    dates: &[(NaiveDate, NaiveDate)],
) -> Result<(), PortfolioAttributionError> {
    for (index, (start, end)) in dates.iter().copied().enumerate() {
        if start >= end {
            return Err(attribution_error(format!(
                "attribution period {} start must precede period end",
                index + 1
            )));
        }
    }
    for (index, pair) in dates.windows(2).enumerate() {
        if pair[0].1 != pair[1].0 {
            return Err(attribution_error(format!(
                "attribution periods {} and {} must be ordered and contiguous",
                index + 1,
                index + 2
            )));
        }
    }
    Ok(())
}

fn validate_snapshot_sequence(
    snapshots: &[PortfolioContributionSnapshot],
    benchmark_present: bool,
) -> Result<(), PortfolioAttributionError> {
    let expected_currencies = currency_set(&snapshots[0]);
    for (index, snapshot) in snapshots.iter().enumerate() {
        if currency_set(snapshot) != expected_currencies {
            return Err(attribution_error(format!(
                "attribution period {} changes the currency set",
                index + 1
            )));
        }
        if snapshot.rows[0].benchmark_contribution_centibps.is_some() != benchmark_present {
            return Err(attribution_error(format!(
                "attribution period {} changes benchmark coverage",
                index + 1
            )));
        }
        for total in &snapshot.currency_totals {
            growth_factor(total.portfolio_return_centibps, "portfolio")?;
            if let Some(benchmark_return) = total.benchmark_return_centibps {
                growth_factor(benchmark_return, "benchmark")?;
            }
        }
    }
    for (index, pair) in snapshots.windows(2).enumerate() {
        for currency in &expected_currencies {
            let previous = currency_total(&pair[0], *currency);
            let next = currency_total(&pair[1], *currency);
            if previous.ending_value != next.beginning_value {
                return Err(attribution_error(format!(
                    "{} ending value in period {} does not reconcile to period {} beginning value",
                    currency,
                    index + 1,
                    index + 2
                )));
            }
            if previous.benchmark_ending_value != next.benchmark_beginning_value {
                return Err(attribution_error(format!(
                    "{} benchmark ending value in period {} does not reconcile to period {} beginning value",
                    currency,
                    index + 1,
                    index + 2
                )));
            }
        }
    }
    Ok(())
}

fn currency_set(snapshot: &PortfolioContributionSnapshot) -> BTreeSet<Currency> {
    snapshot
        .currency_totals
        .iter()
        .map(|total| total.currency)
        .collect()
}

fn currency_total(
    snapshot: &PortfolioContributionSnapshot,
    currency: Currency,
) -> &super::PortfolioContributionCurrencyTotal {
    snapshot
        .currency_totals
        .iter()
        .find(|total| total.currency == currency)
        .expect("validated currency")
}

fn growth_factor(return_centibps: i64, label: &str) -> Result<i128, PortfolioAttributionError> {
    let growth = CONTRIBUTION_SCALE
        .checked_add(i128::from(return_centibps))
        .ok_or_else(|| attribution_error(format!("{label} growth factor overflow")))?;
    if growth <= 0 {
        return Err(attribution_error(format!(
            "{label} period return must be greater than -100% for geometric linking"
        )));
    }
    Ok(growth)
}

fn multiply_scaled(
    left: i128,
    right: i128,
    message: &str,
) -> Result<i128, PortfolioAttributionError> {
    let product = left
        .checked_mul(right)
        .ok_or_else(|| attribution_error(message))?;
    let quotient = product / CONTRIBUTION_SCALE;
    let remainder = product % CONTRIBUTION_SCALE;
    let rounds_away =
        remainder.unsigned_abs().saturating_mul(2) >= CONTRIBUTION_SCALE.unsigned_abs();
    if rounds_away {
        quotient
            .checked_add(product.signum())
            .ok_or_else(|| attribution_error(message))
    } else {
        Ok(quotient)
    }
}

fn factor_return(factor: i128, message: &str) -> Result<i64, PortfolioAttributionError> {
    let value = factor
        .checked_sub(CONTRIBUTION_SCALE)
        .ok_or_else(|| attribution_error(message))?;
    to_i64(value, message)
}

fn sum_rows(
    rows: &[PortfolioAttributionRow],
    currency: Currency,
    value: impl Fn(&PortfolioAttributionRow) -> Option<i64>,
) -> Result<i64, PortfolioAttributionError> {
    rows.iter()
        .filter(|row| row.currency == currency)
        .filter_map(value)
        .try_fold(0_i64, |total, value| {
            total
                .checked_add(value)
                .ok_or_else(|| attribution_error("linked row sum overflow"))
        })
}

fn parse_iso_date(value: &str, field: &str) -> Result<NaiveDate, PortfolioAttributionError> {
    NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d")
        .map_err(|_| attribution_error(format!("{field} must use YYYY-MM-DD")))
}

fn checked_add(left: i128, right: i128, message: &str) -> Result<i128, PortfolioAttributionError> {
    left.checked_add(right)
        .ok_or_else(|| attribution_error(message))
}

fn to_i64(value: i128, message: &str) -> Result<i64, PortfolioAttributionError> {
    i64::try_from(value).map_err(|_| attribution_error(message))
}

fn attribution_error(message: impl Into<String>) -> PortfolioAttributionError {
    PortfolioAttributionError(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::portfolio::PortfolioContributionInputRow;
    use crate::foundation::Money;

    fn usd() -> Currency {
        Currency::new("USD").unwrap()
    }

    fn eur() -> Currency {
        Currency::new("EUR").unwrap()
    }

    fn row(
        symbol: &str,
        currency: Currency,
        beginning: i128,
        ending: i128,
        benchmark: Option<(i128, i128)>,
    ) -> PortfolioContributionInputRow {
        PortfolioContributionInputRow {
            account_id: PortfolioAccountId::new("ACCOUNT 1"),
            instrument_id: InstrumentId::new(format!(
                "test:instrument:{}",
                symbol.to_ascii_lowercase()
            )),
            symbol: symbol.to_owned(),
            currency,
            beginning_value: Money::from_minor_units(beginning, currency),
            external_flow: Money::from_minor_units(0, currency),
            ending_value: Money::from_minor_units(ending, currency),
            benchmark_beginning_value: benchmark
                .map(|(value, _)| Money::from_minor_units(value, currency)),
            benchmark_ending_value: benchmark
                .map(|(_, value)| Money::from_minor_units(value, currency)),
        }
    }

    fn period(
        start: &str,
        end: &str,
        rows: Vec<PortfolioContributionInputRow>,
    ) -> PortfolioContributionInput {
        PortfolioContributionInput {
            rows,
            source: "PERIOD FIXTURE".to_owned(),
            period_start: start.to_owned(),
            period_end: end.to_owned(),
            input_version: format!("{start}:{end}"),
            disclosures: vec!["VERIFIED TEST PERIOD".to_owned()],
        }
    }

    fn input(periods: Vec<PortfolioContributionInput>) -> PortfolioAttributionInput {
        PortfolioAttributionInput {
            periods,
            source: "VERIFIED HISTORY".to_owned(),
            input_version: "HISTORY-V1".to_owned(),
            disclosures: vec!["TEST HISTORY".to_owned()],
        }
    }

    fn linked_fixture() -> PortfolioAttributionInput {
        input(vec![
            period(
                "2026-01-01",
                "2026-02-01",
                vec![
                    row("ALPHA", usd(), 60_000, 66_000, Some((50_000, 52_500))),
                    row("BETA", usd(), 40_000, 44_000, Some((50_000, 52_500))),
                ],
            ),
            period(
                "2026-02-01",
                "2026-03-01",
                vec![
                    row("ALPHA", usd(), 66_000, 59_400, Some((52_500, 51_975))),
                    row("BETA", usd(), 44_000, 48_400, Some((52_500, 51_975))),
                ],
            ),
        ])
    }

    #[test]
    fn links_ordered_contributions_and_benchmark_with_frongello_scaling() {
        let snapshot = calculate_multi_period_attribution(linked_fixture()).unwrap();

        assert_eq!(snapshot.period, "2026-01-01 — 2026-03-01");
        assert!(snapshot.methodology.contains("FRONGELLO"));
        let alpha = snapshot
            .rows
            .iter()
            .find(|row| row.symbol == "ALPHA")
            .unwrap();
        let beta = snapshot
            .rows
            .iter()
            .find(|row| row.symbol == "BETA")
            .unwrap();
        assert_eq!(alpha.linked_contribution_label(), "-0.6000%");
        assert_eq!(alpha.linked_benchmark_contribution_label(), "+1.9750%");
        assert_eq!(alpha.linked_active_contribution_label(), "-2.5750%");
        assert_eq!(beta.linked_contribution_label(), "+8.4000%");
        assert_eq!(beta.linked_benchmark_contribution_label(), "+1.9750%");
        assert_eq!(beta.linked_active_contribution_label(), "+6.4250%");
        let total = &snapshot.currency_totals[0];
        assert_eq!(total.linked_return_label(), "+7.8000%");
        assert_eq!(total.linked_benchmark_return_label(), "+3.9500%");
        assert_eq!(total.linked_active_return_label(), "+3.8500%");
        assert_eq!(total.contribution_rounding_residual_centibps, 0);
        assert_eq!(total.benchmark_rounding_residual_centibps, Some(0));
        assert_eq!(total.active_rounding_residual_centibps, Some(0));
    }

    #[test]
    fn permits_changing_security_membership_and_keeps_currencies_separate() {
        let snapshot = calculate_multi_period_attribution(input(vec![
            period(
                "2026-01-01",
                "2026-02-01",
                vec![
                    row("ALPHA", usd(), 100, 110, None),
                    row("EURO", eur(), 200, 220, None),
                ],
            ),
            period(
                "2026-02-01",
                "2026-03-01",
                vec![
                    row("BETA", usd(), 110, 121, None),
                    row("EURO", eur(), 220, 198, None),
                ],
            ),
        ]))
        .unwrap();

        assert_eq!(snapshot.rows.len(), 3);
        assert_eq!(
            snapshot
                .rows
                .iter()
                .find(|row| row.symbol == "ALPHA")
                .unwrap()
                .periods_present,
            1
        );
        assert_eq!(snapshot.currency_totals.len(), 2);
        assert!(snapshot
            .disclosures
            .iter()
            .any(|value| value.contains("NO FX CONVERSION")));
    }

    #[test]
    fn exposes_every_fixed_point_reconciliation_residual() {
        let snapshot = calculate_multi_period_attribution(input(vec![
            period(
                "2026-01-01",
                "2026-02-01",
                vec![
                    row("A", usd(), 1, 2, None),
                    row("B", usd(), 2, 3, None),
                    row("C", usd(), 3, 4, None),
                ],
            ),
            period(
                "2026-02-01",
                "2026-03-01",
                vec![
                    row("A", usd(), 2, 3, None),
                    row("B", usd(), 3, 4, None),
                    row("C", usd(), 4, 5, None),
                ],
            ),
        ]))
        .unwrap();

        let total = &snapshot.currency_totals[0];
        let sum = snapshot
            .rows
            .iter()
            .map(|row| row.linked_contribution_centibps)
            .sum::<i64>();
        assert_eq!(
            sum + total.contribution_rounding_residual_centibps,
            total.linked_return_centibps
        );
        assert_ne!(total.contribution_rounding_residual_centibps, 0);
    }

    #[test]
    fn rejects_gaps_and_value_discontinuities() {
        let mut gap = linked_fixture();
        gap.periods[1].period_start = "2026-02-02".to_owned();
        assert!(calculate_multi_period_attribution(gap)
            .unwrap_err()
            .to_string()
            .contains("ordered and contiguous"));

        let mut discontinuity = linked_fixture();
        discontinuity.periods[1].rows[0].beginning_value = Money::from_minor_units(66_001, usd());
        assert!(calculate_multi_period_attribution(discontinuity)
            .unwrap_err()
            .to_string()
            .contains("does not reconcile"));
    }

    #[test]
    fn rejects_currency_or_benchmark_coverage_changes() {
        let mut currency_change = linked_fixture();
        currency_change.periods[1]
            .rows
            .push(row("EURO", eur(), 100, 100, Some((100, 100))));
        assert!(calculate_multi_period_attribution(currency_change)
            .unwrap_err()
            .to_string()
            .contains("changes the currency set"));

        let mut benchmark_change = linked_fixture();
        for row in &mut benchmark_change.periods[1].rows {
            row.benchmark_beginning_value = None;
            row.benchmark_ending_value = None;
        }
        assert!(calculate_multi_period_attribution(benchmark_change)
            .unwrap_err()
            .to_string()
            .contains("changes benchmark coverage"));
    }

    #[test]
    fn rejects_single_period_and_returns_at_or_below_negative_one_hundred_percent() {
        let one_period = input(vec![period(
            "2026-01-01",
            "2026-02-01",
            vec![row("ALPHA", usd(), 100, 110, None)],
        )]);
        assert!(calculate_multi_period_attribution(one_period)
            .unwrap_err()
            .to_string()
            .contains("at least two periods"));

        let mut loss_row = row("ALPHA", usd(), 100, 100, None);
        loss_row.external_flow = Money::from_minor_units(200, usd());
        let total_loss = input(vec![
            period("2026-01-01", "2026-02-01", vec![loss_row]),
            period(
                "2026-02-01",
                "2026-03-01",
                vec![row("ALPHA", usd(), 100, 100, None)],
            ),
        ]);
        assert!(calculate_multi_period_attribution(total_loss)
            .unwrap_err()
            .to_string()
            .contains("greater than -100%"));
    }
}
