use std::{cmp::Ordering, collections::BTreeSet, fmt};

use chrono::NaiveDate;

use crate::foundation::Currency;

const MAX_HISTORICAL_SERIES: usize = 32;
const MAX_HISTORICAL_POINTS: usize = 100_000;
const RETURN_BPS_SCALE: f64 = 10_000.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoricalRiskPointInput {
    pub date: String,
    pub ending_value_minor: i128,
    pub external_flow_minor: i128,
    pub benchmark_value_minor: Option<i128>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoricalRiskSeriesInput {
    pub currency: Currency,
    pub points: Vec<HistoricalRiskPointInput>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoricalRiskInput {
    pub series: Vec<HistoricalRiskSeriesInput>,
    pub source: String,
    pub period: String,
    pub input_version: String,
    pub confidence_bps: u16,
    pub ewma_lambda_millionths: u32,
    pub annual_risk_free_rate_bps: i32,
    pub disclosures: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoricalRiskSummary {
    pub currency: Currency,
    pub period_start: String,
    pub period_end: String,
    pub observations: usize,
    pub median_interval_days: u32,
    pub annualization_periods_hundredths: u32,
    pub annualized_volatility_bps: i32,
    pub ewma_volatility_bps: i32,
    pub max_drawdown_bps: i32,
    pub drawdown_peak_date: String,
    pub drawdown_trough_date: String,
    pub recovery_date: Option<String>,
    pub historical_var_bps: i32,
    pub historical_cvar_bps: i32,
    pub parametric_var_bps: i32,
    pub parametric_cvar_bps: i32,
    pub sharpe_hundredths: Option<i32>,
    pub sortino_hundredths: Option<i32>,
    pub beta_hundredths: Option<i32>,
    pub correlation_hundredths: Option<i32>,
    pub tracking_error_bps: Option<i32>,
    pub information_ratio_hundredths: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoricalRiskSnapshot {
    pub series: Vec<HistoricalRiskSummary>,
    pub source: String,
    pub period: String,
    pub input_version: String,
    pub confidence_bps: u16,
    pub ewma_lambda_millionths: u32,
    pub annual_risk_free_rate_bps: i32,
    pub methodology: String,
    pub disclosures: Vec<String>,
}

impl HistoricalRiskSnapshot {
    pub fn annualized_volatility_label(&self) -> String {
        single_metric(&self.series, |series| series.annualized_volatility_bps)
    }

    pub fn max_drawdown_label(&self) -> String {
        single_metric(&self.series, |series| series.max_drawdown_bps)
    }

    pub fn historical_var_label(&self) -> String {
        single_metric(&self.series, |series| series.historical_var_bps)
    }

    pub fn sharpe_label(&self) -> String {
        match self.series.as_slice() {
            [series] => series
                .sharpe_hundredths
                .map(format_hundredths)
                .unwrap_or_else(|| "N/A".to_owned()),
            [] => "N/A".to_owned(),
            series => format!("{} CCY · PER-CCY", series.len()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoricalRiskError(String);

impl fmt::Display for HistoricalRiskError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for HistoricalRiskError {}

pub fn calculate_historical_risk(
    input: HistoricalRiskInput,
) -> Result<HistoricalRiskSnapshot, HistoricalRiskError> {
    validate_metadata(&input)?;
    if input.series.is_empty() || input.series.len() > MAX_HISTORICAL_SERIES {
        return Err(error(format!(
            "historical risk needs 1–{MAX_HISTORICAL_SERIES} currency series"
        )));
    }
    if !matches!(input.confidence_bps, 9_000 | 9_500 | 9_750 | 9_900) {
        return Err(error("confidence must be 90%, 95%, 97.5%, or 99%"));
    }
    if !(1..1_000_000).contains(&input.ewma_lambda_millionths) {
        return Err(error("EWMA lambda must be between zero and one"));
    }

    let mut currencies = BTreeSet::new();
    let mut summaries = Vec::with_capacity(input.series.len());
    for series in &input.series {
        if !currencies.insert(series.currency) {
            return Err(error(format!(
                "duplicate {} historical risk series",
                series.currency
            )));
        }
        summaries.push(calculate_series(series, &input)?);
    }
    summaries.sort_by_key(|summary| summary.currency);

    let mut disclosures = input.disclosures;
    disclosures
        .push("RETURNS USE END-OF-PERIOD FLOW-ADJUSTED VALUES AND REMAIN PER CURRENCY".to_owned());
    disclosures.push(format!(
        "VAR/CVAR ARE {}% ONE-OBSERVATION LOSS ESTIMATES · POSITIVE VALUES DENOTE LOSS",
        format_confidence(input.confidence_bps)
    ));
    disclosures.push(format!(
        "EWMA USES ZERO-MEAN RETURNS AND LAMBDA {:.6}",
        input.ewma_lambda_millionths as f64 / 1_000_000.0
    ));
    disclosures.push(
        "ANNUALIZATION USES 365.25 DIVIDED BY EACH SERIES' MEDIAN CALENDAR-DAY GAP".to_owned(),
    );
    disclosures.push(
        "PER-OBSERVATION RISK-FREE RETURN IS THE ANNUAL INPUT DIVIDED BY PERIODS PER YEAR"
            .to_owned(),
    );
    if summaries.iter().any(|summary| summary.observations < 20) {
        disclosures.push(
            "LOW SAMPLE COUNT · TAIL, CORRELATION, AND RATIO ESTIMATES ARE UNSTABLE".to_owned(),
        );
    }
    if summaries
        .iter()
        .any(|summary| summary.beta_hundredths.is_none())
    {
        disclosures.push(
            "BENCHMARK-RELATIVE METRICS REQUIRE A COMPLETE BENCHMARK VALUE SERIES".to_owned(),
        );
    }
    if summaries.len() > 1 {
        disclosures.push("NO FX CONVERSION · NO CROSS-CURRENCY AGGREGATION".to_owned());
    }

    Ok(HistoricalRiskSnapshot {
        series: summaries,
        source: input.source,
        period: input.period,
        input_version: input.input_version,
        confidence_bps: input.confidence_bps,
        ewma_lambda_millionths: input.ewma_lambda_millionths,
        annual_risk_free_rate_bps: input.annual_risk_free_rate_bps,
        methodology: "SAMPLE VOLATILITY · ZERO-MEAN EWMA · WEALTH-INDEX DRAWDOWN · HISTORICAL AND GAUSSIAN VAR/CVAR · SAMPLE BENCHMARK MOMENTS".to_owned(),
        disclosures,
    })
}

fn calculate_series(
    series: &HistoricalRiskSeriesInput,
    input: &HistoricalRiskInput,
) -> Result<HistoricalRiskSummary, HistoricalRiskError> {
    if series.points.len() < 4 || series.points.len() > MAX_HISTORICAL_POINTS {
        return Err(error(format!(
            "{} historical risk needs 4–{MAX_HISTORICAL_POINTS} valuations",
            series.currency
        )));
    }
    let mut dates = Vec::with_capacity(series.points.len());
    let benchmark_present = series.points[0].benchmark_value_minor.is_some();
    for (index, point) in series.points.iter().enumerate() {
        let date = NaiveDate::parse_from_str(&point.date, "%Y-%m-%d").map_err(|_| {
            error(format!(
                "{} has invalid date {}",
                series.currency, point.date
            ))
        })?;
        if index > 0 && date <= dates[index - 1] {
            return Err(error(format!(
                "{} valuation dates must be strictly increasing",
                series.currency
            )));
        }
        if point.ending_value_minor <= 0 {
            return Err(error(format!(
                "{} ending values must be positive",
                series.currency
            )));
        }
        if index == 0 && point.external_flow_minor != 0 {
            return Err(error(format!(
                "first {} valuation cannot contain an external flow",
                series.currency
            )));
        }
        if point.benchmark_value_minor.is_some() != benchmark_present {
            return Err(error(format!(
                "{} benchmark coverage must be complete or absent",
                series.currency
            )));
        }
        if point.benchmark_value_minor.is_some_and(|value| value <= 0) {
            return Err(error(format!(
                "{} benchmark values must be positive",
                series.currency
            )));
        }
        dates.push(date);
    }

    let mut portfolio_returns = Vec::with_capacity(series.points.len() - 1);
    let mut benchmark_returns = benchmark_present.then(Vec::new);
    let mut intervals = Vec::with_capacity(series.points.len() - 1);
    for index in 1..series.points.len() {
        let previous = &series.points[index - 1];
        let current = &series.points[index];
        let adjusted_end = current
            .ending_value_minor
            .checked_sub(current.external_flow_minor)
            .ok_or_else(|| error("flow-adjusted value overflow"))?;
        if adjusted_end <= 0 {
            return Err(error(format!(
                "{} flow-adjusted values must remain positive",
                series.currency
            )));
        }
        portfolio_returns.push(exact_return_bps(adjusted_end, previous.ending_value_minor)?);
        if let Some(returns) = benchmark_returns.as_mut() {
            returns.push(exact_return_bps(
                current.benchmark_value_minor.expect("complete benchmark"),
                previous.benchmark_value_minor.expect("complete benchmark"),
            )?);
        }
        intervals.push((dates[index] - dates[index - 1]).num_days() as u32);
    }
    intervals.sort_unstable();
    let median_interval = if intervals.len() % 2 == 0 {
        (f64::from(intervals[intervals.len() / 2 - 1]) + f64::from(intervals[intervals.len() / 2]))
            / 2.0
    } else {
        f64::from(intervals[intervals.len() / 2])
    };
    if median_interval == 0.0 {
        return Err(error("historical risk intervals must be positive"));
    }
    let periods_per_year = 365.25 / median_interval;
    let annualization = periods_per_year.sqrt();
    let portfolio_mean = mean(&portfolio_returns);
    let volatility = sample_stddev(&portfolio_returns)?;
    let annualized_volatility = volatility * annualization;
    let ewma_lambda = input.ewma_lambda_millionths as f64 / 1_000_000.0;
    let ewma_volatility = ewma_stddev(&portfolio_returns, ewma_lambda) * annualization;
    let risk_free_period = f64::from(input.annual_risk_free_rate_bps) / periods_per_year;
    let sharpe = ratio(portfolio_mean - risk_free_period, volatility, annualization);
    let downside = downside_deviation(&portfolio_returns, risk_free_period);
    let sortino = ratio(portfolio_mean - risk_free_period, downside, annualization);
    let drawdown = max_drawdown(&portfolio_returns, &series.points);
    let confidence = f64::from(input.confidence_bps) / 10_000.0;
    let historical = historical_tail(&portfolio_returns, confidence);
    let z = confidence_z(input.confidence_bps);
    let parametric_var = (-portfolio_mean + z * volatility).max(0.0);
    let parametric_cvar =
        (-portfolio_mean + volatility * normal_pdf(z) / (1.0 - confidence)).max(0.0);

    let (beta, correlation, tracking_error, information_ratio) =
        if let Some(benchmark) = benchmark_returns {
            let benchmark_variance = sample_variance(&benchmark)?;
            let covariance = sample_covariance(&portfolio_returns, &benchmark)?;
            let beta = (benchmark_variance > 0.0).then_some(covariance / benchmark_variance);
            let benchmark_stddev = benchmark_variance.sqrt();
            let correlation = (volatility > 0.0 && benchmark_stddev > 0.0)
                .then_some(covariance / (volatility * benchmark_stddev));
            let active = portfolio_returns
                .iter()
                .zip(&benchmark)
                .map(|(portfolio, benchmark)| portfolio - benchmark)
                .collect::<Vec<_>>();
            let active_stddev = sample_stddev(&active)?;
            (
                beta,
                correlation,
                Some(active_stddev * annualization),
                ratio(mean(&active), active_stddev, annualization),
            )
        } else {
            (None, None, None, None)
        };

    Ok(HistoricalRiskSummary {
        currency: series.currency,
        period_start: series.points.first().expect("four points").date.clone(),
        period_end: series.points.last().expect("four points").date.clone(),
        observations: portfolio_returns.len(),
        median_interval_days: round_u32(median_interval)?,
        annualization_periods_hundredths: round_u32(periods_per_year * 100.0)?,
        annualized_volatility_bps: round_i32(annualized_volatility)?,
        ewma_volatility_bps: round_i32(ewma_volatility)?,
        max_drawdown_bps: round_i32(drawdown.depth_bps)?,
        drawdown_peak_date: drawdown.peak_date,
        drawdown_trough_date: drawdown.trough_date,
        recovery_date: drawdown.recovery_date,
        historical_var_bps: round_i32(historical.0)?,
        historical_cvar_bps: round_i32(historical.1)?,
        parametric_var_bps: round_i32(parametric_var)?,
        parametric_cvar_bps: round_i32(parametric_cvar)?,
        sharpe_hundredths: round_optional_hundredths(sharpe)?,
        sortino_hundredths: round_optional_hundredths(sortino)?,
        beta_hundredths: round_optional_hundredths(beta)?,
        correlation_hundredths: round_optional_hundredths(correlation)?,
        tracking_error_bps: tracking_error.map(round_i32).transpose()?,
        information_ratio_hundredths: round_optional_hundredths(information_ratio)?,
    })
}

fn exact_return_bps(end: i128, begin: i128) -> Result<f64, HistoricalRiskError> {
    let change = end
        .checked_sub(begin)
        .ok_or_else(|| error("return change overflow"))?;
    let scaled = change
        .checked_mul(10_000)
        .ok_or_else(|| error("return scaling overflow"))?;
    Ok((scaled as f64) / (begin as f64))
}

fn mean(values: &[f64]) -> f64 {
    values.iter().sum::<f64>() / values.len() as f64
}

fn sample_variance(values: &[f64]) -> Result<f64, HistoricalRiskError> {
    if values.len() < 2 {
        return Err(error("sample variance needs at least two observations"));
    }
    let average = mean(values);
    Ok(values
        .iter()
        .map(|value| (value - average).powi(2))
        .sum::<f64>()
        / (values.len() - 1) as f64)
}

fn sample_stddev(values: &[f64]) -> Result<f64, HistoricalRiskError> {
    Ok(sample_variance(values)?.sqrt())
}

fn sample_covariance(left: &[f64], right: &[f64]) -> Result<f64, HistoricalRiskError> {
    if left.len() != right.len() || left.len() < 2 {
        return Err(error("covariance inputs must have equal sample counts"));
    }
    let left_mean = mean(left);
    let right_mean = mean(right);
    Ok(left
        .iter()
        .zip(right)
        .map(|(left, right)| (left - left_mean) * (right - right_mean))
        .sum::<f64>()
        / (left.len() - 1) as f64)
}

fn ewma_stddev(values: &[f64], lambda: f64) -> f64 {
    let mut variance = values[0].powi(2);
    for value in &values[1..] {
        variance = lambda * variance + (1.0 - lambda) * value.powi(2);
    }
    variance.sqrt()
}

fn downside_deviation(values: &[f64], target: f64) -> f64 {
    (values
        .iter()
        .map(|value| (value - target).min(0.0).powi(2))
        .sum::<f64>()
        / values.len() as f64)
        .sqrt()
}

fn ratio(numerator: f64, denominator: f64, annualization: f64) -> Option<f64> {
    (denominator > 0.0).then_some(numerator / denominator * annualization)
}

struct Drawdown {
    depth_bps: f64,
    peak_date: String,
    trough_date: String,
    recovery_date: Option<String>,
}

fn max_drawdown(returns: &[f64], points: &[HistoricalRiskPointInput]) -> Drawdown {
    let mut wealth = 1.0;
    let mut running_peak = 1.0;
    let mut running_peak_date = points[0].date.clone();
    let mut max_depth = 0.0;
    let mut max_peak = 1.0;
    let mut peak_date = points[0].date.clone();
    let mut trough_date = points[0].date.clone();
    let mut recovery_date = None;
    for (index, value) in returns.iter().enumerate() {
        wealth *= 1.0 + value / RETURN_BPS_SCALE;
        let date = points[index + 1].date.clone();
        if wealth >= running_peak {
            running_peak = wealth;
            running_peak_date.clone_from(&date);
        }
        let depth = (wealth / running_peak - 1.0) * RETURN_BPS_SCALE;
        if depth < max_depth {
            max_depth = depth;
            max_peak = running_peak;
            peak_date.clone_from(&running_peak_date);
            trough_date.clone_from(&date);
            recovery_date = None;
        } else if max_depth < 0.0 && recovery_date.is_none() && wealth >= max_peak {
            recovery_date = Some(date);
        }
    }
    Drawdown {
        depth_bps: max_depth,
        peak_date,
        trough_date,
        recovery_date,
    }
}

fn historical_tail(returns: &[f64], confidence: f64) -> (f64, f64) {
    let mut losses = returns.iter().map(|value| -*value).collect::<Vec<_>>();
    losses.sort_by(|left, right| left.partial_cmp(right).unwrap_or(Ordering::Equal));
    let index = ((confidence * losses.len() as f64).ceil() as usize)
        .saturating_sub(1)
        .min(losses.len() - 1);
    let raw_var = losses[index];
    let tail = losses
        .iter()
        .copied()
        .filter(|loss| *loss >= raw_var)
        .collect::<Vec<_>>();
    (raw_var.max(0.0), mean(&tail).max(0.0))
}

fn confidence_z(confidence_bps: u16) -> f64 {
    match confidence_bps {
        9_000 => 1.281_551_565_544_600_4,
        9_500 => 1.644_853_626_951_472_2,
        9_750 => 1.959_963_984_540_054,
        9_900 => 2.326_347_874_040_840_8,
        _ => unreachable!("validated confidence"),
    }
}

fn normal_pdf(value: f64) -> f64 {
    (-0.5 * value.powi(2)).exp() / (2.0 * std::f64::consts::PI).sqrt()
}

fn round_i32(value: f64) -> Result<i32, HistoricalRiskError> {
    if !value.is_finite() || value < f64::from(i32::MIN) || value > f64::from(i32::MAX) {
        return Err(error("historical risk metric exceeds its typed range"));
    }
    Ok(value.round() as i32)
}

fn round_u32(value: f64) -> Result<u32, HistoricalRiskError> {
    if !value.is_finite() || value < 0.0 || value > f64::from(u32::MAX) {
        return Err(error("annualization factor exceeds its typed range"));
    }
    Ok(value.round() as u32)
}

fn round_optional_hundredths(value: Option<f64>) -> Result<Option<i32>, HistoricalRiskError> {
    value.map(|value| round_i32(value * 100.0)).transpose()
}

fn validate_metadata(input: &HistoricalRiskInput) -> Result<(), HistoricalRiskError> {
    for (label, value) in [
        ("source", input.source.as_str()),
        ("period", input.period.as_str()),
        ("input version", input.input_version.as_str()),
    ] {
        if value.trim().is_empty() || value.len() > 1_024 {
            return Err(error(format!("{label} is empty or too long")));
        }
    }
    Ok(())
}

fn single_metric(
    series: &[HistoricalRiskSummary],
    value: impl Fn(&HistoricalRiskSummary) -> i32,
) -> String {
    match series {
        [series] => format_bps(value(series)),
        [] => "N/A".to_owned(),
        series => format!("{} CCY · PER-CCY", series.len()),
    }
}

pub fn format_bps(value: i32) -> String {
    let sign = if value < 0 { "-" } else { "" };
    let absolute = value.unsigned_abs();
    format!("{sign}{}.{:02}%", absolute / 100, absolute % 100)
}

pub fn format_hundredths(value: i32) -> String {
    let sign = if value < 0 { "-" } else { "" };
    let absolute = value.unsigned_abs();
    format!("{sign}{}.{:02}", absolute / 100, absolute % 100)
}

fn format_confidence(value: u16) -> String {
    if value.is_multiple_of(100) {
        (value / 100).to_string()
    } else {
        format!("{}.{:02}", value / 100, value % 100)
    }
}

fn error(message: impl Into<String>) -> HistoricalRiskError {
    HistoricalRiskError(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(benchmark: bool) -> HistoricalRiskInput {
        let usd = Currency::new("USD").unwrap();
        let values = [10_000, 10_500, 9_450, 9_828, 10_811, 10_270];
        let benchmark_values = [1_000, 1_020, 979, 999, 1_049, 1_028];
        let dates = [
            "2026-01-02",
            "2026-02-02",
            "2026-03-02",
            "2026-04-02",
            "2026-05-04",
            "2026-06-02",
        ];
        HistoricalRiskInput {
            series: vec![HistoricalRiskSeriesInput {
                currency: usd,
                points: dates
                    .into_iter()
                    .zip(values)
                    .zip(benchmark_values)
                    .map(
                        |((date, value), benchmark_value)| HistoricalRiskPointInput {
                            date: date.to_owned(),
                            ending_value_minor: value,
                            external_flow_minor: 0,
                            benchmark_value_minor: benchmark.then_some(benchmark_value),
                        },
                    )
                    .collect(),
            }],
            source: "TEST VALUATIONS".to_owned(),
            period: "2026-01-02 — 2026-06-02".to_owned(),
            input_version: "HISTORY-1".to_owned(),
            confidence_bps: 9_500,
            ewma_lambda_millionths: 940_000,
            annual_risk_free_rate_bps: 0,
            disclosures: Vec::new(),
        }
    }

    #[test]
    fn calculates_absolute_relative_and_tail_risk_with_explicit_metadata() {
        let snapshot = calculate_historical_risk(input(true)).unwrap();
        let summary = &snapshot.series[0];

        assert_eq!(summary.observations, 5);
        assert_eq!(summary.median_interval_days, 31);
        assert!(summary.annualized_volatility_bps > 0);
        assert!(summary.ewma_volatility_bps > 0);
        assert_eq!(summary.max_drawdown_bps, -1_000);
        assert_eq!(summary.drawdown_peak_date, "2026-02-02");
        assert_eq!(summary.drawdown_trough_date, "2026-03-02");
        assert_eq!(summary.recovery_date.as_deref(), Some("2026-05-04"));
        assert_eq!(summary.historical_var_bps, 1_000);
        assert!(summary.historical_cvar_bps >= summary.historical_var_bps);
        assert!(summary.parametric_cvar_bps >= summary.parametric_var_bps);
        assert!(summary.beta_hundredths.is_some());
        assert!(summary.correlation_hundredths.is_some());
        assert!(summary.tracking_error_bps.is_some());
        assert!(summary.information_ratio_hundredths.is_some());
        assert!(snapshot.methodology.contains("HISTORICAL AND GAUSSIAN"));
        assert!(snapshot
            .disclosures
            .iter()
            .any(|disclosure| disclosure.contains("LOW SAMPLE COUNT")));
    }

    #[test]
    fn flow_adjustment_and_missing_benchmark_remain_explicit() {
        let mut input = input(false);
        input.series[0].points[5].ending_value_minor += 1_000;
        input.series[0].points[5].external_flow_minor = 1_000;
        let snapshot = calculate_historical_risk(input).unwrap();
        let summary = &snapshot.series[0];

        assert_eq!(summary.max_drawdown_bps, -1_000);
        assert!(summary.beta_hundredths.is_none());
        assert!(snapshot
            .disclosures
            .iter()
            .any(|disclosure| disclosure.contains("BENCHMARK-RELATIVE")));
    }

    #[test]
    fn rejects_partial_benchmark_bad_chronology_and_short_samples() {
        let mut partial = input(true);
        partial.series[0].points[2].benchmark_value_minor = None;
        assert!(calculate_historical_risk(partial)
            .unwrap_err()
            .to_string()
            .contains("coverage"));

        let mut chronology = input(true);
        chronology.series[0].points[2].date = "2026-01-01".to_owned();
        assert!(calculate_historical_risk(chronology)
            .unwrap_err()
            .to_string()
            .contains("strictly increasing"));

        let mut short = input(true);
        short.series[0].points.truncate(3);
        assert!(calculate_historical_risk(short)
            .unwrap_err()
            .to_string()
            .contains("needs 4"));
    }

    #[test]
    fn identical_benchmark_has_unit_beta_and_zero_active_risk() {
        let mut input = input(true);
        for point in &mut input.series[0].points {
            point.benchmark_value_minor = Some(point.ending_value_minor);
        }
        let summary = &calculate_historical_risk(input).unwrap().series[0];

        assert_eq!(summary.beta_hundredths, Some(100));
        assert_eq!(summary.correlation_hundredths, Some(100));
        assert_eq!(summary.tracking_error_bps, Some(0));
        assert_eq!(summary.information_ratio_hundredths, None);
    }
}
