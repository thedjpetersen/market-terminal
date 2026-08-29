use std::fmt;

use serde::{Deserialize, Serialize};

pub const MODEL_VERSION: &str = "FIXED-RATE-BULLET-PERIODIC-V1";
const MONEY_SCALE: f64 = 1_000_000.0;
const METRIC_SCALE: f64 = 1_000_000.0;
const BPS: f64 = 10_000.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CouponFrequency {
    Annual,
    SemiAnnual,
    Quarterly,
}

impl CouponFrequency {
    pub const fn periods_per_year(self) -> u32 {
        match self {
            Self::Annual => 1,
            Self::SemiAnnual => 2,
            Self::Quarterly => 4,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Annual => "ANNUAL",
            Self::SemiAnnual => "SEMIANNUAL",
            Self::Quarterly => "QUARTERLY",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BondModelInput {
    pub instrument_id: String,
    pub currency: String,
    pub face_micros: i64,
    /// Nominal annual coupon in basis points.
    pub coupon_bps: i32,
    /// Nominal annual yield compounded at the coupon frequency, in basis points.
    pub yield_bps: i32,
    pub years_to_maturity: u32,
    pub frequency: CouponFrequency,
    /// Fraction of the current coupon period elapsed, in basis points.
    pub accrued_period_bps: u32,
}

impl Default for BondModelInput {
    fn default() -> Self {
        Self {
            instrument_id: "UST-5Y-REFERENCE".to_owned(),
            currency: "USD".to_owned(),
            face_micros: 100_000_000,
            coupon_bps: 450,
            yield_bps: 425,
            years_to_maturity: 5,
            frequency: CouponFrequency::SemiAnnual,
            accrued_period_bps: 0,
        }
    }
}

impl BondModelInput {
    pub fn validate(&self) -> Result<(), BondModelError> {
        if self.instrument_id.is_empty()
            || self.instrument_id.len() > 64
            || !self.instrument_id.chars().all(|value| {
                value.is_ascii_alphanumeric() || matches!(value, '.' | '-' | '_' | ':')
            })
        {
            return Err(BondModelError::InvalidInput(
                "instrument ID must be 1-64 terminal identity characters".to_owned(),
            ));
        }
        if self.currency.len() != 3
            || !self
                .currency
                .chars()
                .all(|value| value.is_ascii_uppercase())
        {
            return Err(BondModelError::InvalidInput(
                "currency must be a three-letter uppercase code".to_owned(),
            ));
        }
        if !(1..=1_000_000_000_000_000).contains(&self.face_micros) {
            return Err(BondModelError::InvalidInput(
                "face value must be positive and at most 1 billion".to_owned(),
            ));
        }
        if !(0..=10_000).contains(&self.coupon_bps) {
            return Err(BondModelError::InvalidInput(
                "coupon must be between 0% and 100%".to_owned(),
            ));
        }
        if !(-4_800..=19_800).contains(&self.yield_bps) {
            return Err(BondModelError::InvalidInput(
                "yield must be between -48% and 198% so the full shock grid remains valid"
                    .to_owned(),
            ));
        }
        let frequency = self.frequency.periods_per_year();
        if 1.0 + f64::from(self.yield_bps) / BPS / f64::from(frequency) <= 0.0 {
            return Err(BondModelError::InvalidInput(
                "yield makes the periodic discount factor non-positive".to_owned(),
            ));
        }
        if !(1..=100).contains(&self.years_to_maturity) {
            return Err(BondModelError::InvalidInput(
                "maturity must be between 1 and 100 whole years".to_owned(),
            ));
        }
        if self.accrued_period_bps >= 10_000 {
            return Err(BondModelError::InvalidInput(
                "accrued period must be at least 0% and less than 100%".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BondCashFlow {
    pub ordinal: u32,
    pub time_years_millionths: i64,
    pub coupon_micros: i64,
    pub principal_micros: i64,
    pub total_micros: i64,
    pub present_value_micros: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct YieldScenario {
    pub shock_bps: i32,
    pub yield_bps: i32,
    pub clean_price_micros: i64,
    pub dirty_price_micros: i64,
    pub clean_change_micros: i64,
    pub clean_change_bps: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BondAnalytics {
    pub input: BondModelInput,
    pub clean_price_micros: i64,
    pub dirty_price_micros: i64,
    pub accrued_interest_micros: i64,
    pub coupon_payment_micros: i64,
    pub current_yield_bps: i32,
    pub macaulay_duration_years_millionths: i64,
    pub modified_duration_years_millionths: i64,
    pub convexity_years2_millionths: i64,
    /// Central clean-price sensitivity: half the difference between one-basis-point
    /// yield-down and yield-up valuations.
    pub dv01_micros: i64,
    pub cash_flows: Vec<BondCashFlow>,
    pub scenarios: Vec<YieldScenario>,
    pub model_version: &'static str,
    pub input_digest: String,
    pub methodology: &'static str,
    pub disclosures: [&'static str; 5],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BondModelError {
    InvalidInput(String),
    InvalidTargetPrice,
    YieldNotBracketed,
    NonFiniteOutput,
}

impl fmt::Display for BondModelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(message) => write!(formatter, "invalid bond model input: {message}"),
            Self::InvalidTargetPrice => write!(formatter, "target clean price must be positive"),
            Self::YieldNotBracketed => write!(
                formatter,
                "target price has no yield in the supported range"
            ),
            Self::NonFiniteOutput => write!(formatter, "bond model produced a non-finite output"),
        }
    }
}

impl std::error::Error for BondModelError {}

pub fn analyze_bond(input: &BondModelInput) -> Result<BondAnalytics, BondModelError> {
    input.validate()?;
    let valuation = value_at_yield(input, input.yield_bps)?;
    let clean_price_micros = scale_money(valuation.clean)?;
    let dirty_price_micros = scale_money(valuation.dirty)?;
    let accrued_interest_micros = scale_money(valuation.accrued)?;
    let coupon_payment_micros = scale_money(valuation.coupon_payment)?;
    let current_yield_bps = ratio_bps(
        valuation.coupon_payment * f64::from(input.frequency.periods_per_year()),
        valuation.clean,
    )?;
    let down = value_at_yield(input, input.yield_bps.saturating_sub(1))?.clean;
    let up = value_at_yield(input, input.yield_bps.saturating_add(1))?.clean;
    let dv01_micros = scale_money((down - up) / 2.0)?;
    let scenarios = scenario_grid(input, valuation.clean, valuation.accrued)?;
    let cash_flows = valuation
        .cash_flows
        .iter()
        .map(|flow| {
            Ok(BondCashFlow {
                ordinal: flow.ordinal,
                time_years_millionths: scale_metric(flow.time_years)?,
                coupon_micros: scale_money(flow.coupon)?,
                principal_micros: scale_money(flow.principal)?,
                total_micros: scale_money(flow.coupon + flow.principal)?,
                present_value_micros: scale_money(flow.present_value)?,
            })
        })
        .collect::<Result<Vec<_>, BondModelError>>()?;
    Ok(BondAnalytics {
        input: input.clone(),
        clean_price_micros,
        dirty_price_micros,
        accrued_interest_micros,
        coupon_payment_micros,
        current_yield_bps,
        macaulay_duration_years_millionths: scale_metric(valuation.macaulay_duration)?,
        modified_duration_years_millionths: scale_metric(valuation.modified_duration)?,
        convexity_years2_millionths: scale_metric(valuation.convexity)?,
        dv01_micros,
        cash_flows,
        scenarios,
        model_version: MODEL_VERSION,
        input_digest: input_digest(input),
        methodology: "FIXED-RATE BULLET · NOMINAL PERIODIC YIELD · EXPLICIT ACCRUAL FRACTION",
        disclosures: [
            "REFERENCE MODEL ONLY · NOT A LIVE PRICE, CURVE, OR RECOMMENDATION",
            "CASH FLOWS USE WHOLE-YEAR MATURITY AND A USER-SUPPLIED ACCRUAL FRACTION",
            "NO HOLIDAY CALENDAR, BUSINESS-DAY ROLL, EX-DATE, TAX, CALL, OR DEFAULT MODEL",
            "DURATION, CONVEXITY, AND DV01 ARE MODEL SENSITIVITIES, NOT PROVIDER FIELDS",
            "PARALLEL YIELD SHOCKS HOLD CASH FLOWS AND CREDIT ASSUMPTIONS CONSTANT",
        ],
    })
}

/// Solves nominal annual yield from an observed clean price using a bounded,
/// deterministic bisection over the domain supported by [`BondModelInput`].
pub fn solve_yield_bps(
    input: &BondModelInput,
    target_clean_price_micros: i64,
) -> Result<i32, BondModelError> {
    input.validate()?;
    if target_clean_price_micros <= 0 {
        return Err(BondModelError::InvalidTargetPrice);
    }
    let target = target_clean_price_micros as f64 / MONEY_SCALE;
    let mut low = -5_000;
    let mut high = 20_000;
    let low_price = value_at_yield(input, low)?.clean;
    let high_price = value_at_yield(input, high)?.clean;
    if target > low_price || target < high_price {
        return Err(BondModelError::YieldNotBracketed);
    }
    while high - low > 1 {
        let middle = low + (high - low) / 2;
        let price = value_at_yield(input, middle)?.clean;
        if price > target {
            low = middle;
        } else {
            high = middle;
        }
    }
    let low_error = (value_at_yield(input, low)?.clean - target).abs();
    let high_error = (value_at_yield(input, high)?.clean - target).abs();
    Ok(if low_error <= high_error { low } else { high })
}

struct RawCashFlow {
    ordinal: u32,
    time_years: f64,
    coupon: f64,
    principal: f64,
    present_value: f64,
}

struct Valuation {
    clean: f64,
    dirty: f64,
    accrued: f64,
    coupon_payment: f64,
    macaulay_duration: f64,
    modified_duration: f64,
    convexity: f64,
    cash_flows: Vec<RawCashFlow>,
}

fn value_at_yield(input: &BondModelInput, yield_bps: i32) -> Result<Valuation, BondModelError> {
    let frequency = input.frequency.periods_per_year();
    let frequency_f = f64::from(frequency);
    let period_rate = f64::from(yield_bps) / BPS / frequency_f;
    let discount_base = 1.0 + period_rate;
    if discount_base <= 0.0 {
        return Err(BondModelError::InvalidInput(
            "yield makes the periodic discount factor non-positive".to_owned(),
        ));
    }
    let face = input.face_micros as f64 / MONEY_SCALE;
    let coupon_payment = face * f64::from(input.coupon_bps) / BPS / frequency_f;
    let elapsed = f64::from(input.accrued_period_bps) / BPS;
    let accrued = coupon_payment * elapsed;
    let periods = input
        .years_to_maturity
        .checked_mul(frequency)
        .ok_or(BondModelError::NonFiniteOutput)?;
    let mut cash_flows = Vec::with_capacity(periods as usize);
    let mut dirty = 0.0;
    let mut weighted_time = 0.0;
    let mut convexity_numerator = 0.0;
    for index in 0..periods {
        let time_periods = (1.0 - elapsed) + f64::from(index);
        let time_years = time_periods / frequency_f;
        let principal = if index + 1 == periods { face } else { 0.0 };
        let payment = coupon_payment + principal;
        let present_value = payment / discount_base.powf(time_periods);
        if !present_value.is_finite() {
            return Err(BondModelError::NonFiniteOutput);
        }
        dirty += present_value;
        weighted_time += time_years * present_value;
        convexity_numerator += time_periods * (time_periods + 1.0) * present_value;
        cash_flows.push(RawCashFlow {
            ordinal: index + 1,
            time_years,
            coupon: coupon_payment,
            principal,
            present_value,
        });
    }
    let clean = dirty - accrued;
    if !dirty.is_finite() || dirty <= 0.0 || clean <= 0.0 {
        return Err(BondModelError::NonFiniteOutput);
    }
    let macaulay_duration = weighted_time / dirty;
    let modified_duration = macaulay_duration / discount_base;
    let convexity =
        convexity_numerator / (dirty * frequency_f * frequency_f * discount_base * discount_base);
    Ok(Valuation {
        clean,
        dirty,
        accrued,
        coupon_payment,
        macaulay_duration,
        modified_duration,
        convexity,
        cash_flows,
    })
}

fn scenario_grid(
    input: &BondModelInput,
    base_clean: f64,
    accrued: f64,
) -> Result<Vec<YieldScenario>, BondModelError> {
    [-200, -100, -50, 0, 50, 100, 200]
        .into_iter()
        .map(|shock_bps| {
            let yield_bps = input.yield_bps.saturating_add(shock_bps);
            if !(-5_000..=20_000).contains(&yield_bps) {
                return Err(BondModelError::InvalidInput(
                    "scenario yield leaves supported range".to_owned(),
                ));
            }
            let valuation = value_at_yield(input, yield_bps)?;
            Ok(YieldScenario {
                shock_bps,
                yield_bps,
                clean_price_micros: scale_money(valuation.clean)?,
                dirty_price_micros: scale_money(valuation.clean + accrued)?,
                clean_change_micros: scale_money(valuation.clean - base_clean)?,
                clean_change_bps: ratio_bps(valuation.clean - base_clean, base_clean)?,
            })
        })
        .collect()
}

fn ratio_bps(numerator: f64, denominator: f64) -> Result<i32, BondModelError> {
    let value = (numerator / denominator * BPS).round();
    if !value.is_finite() || value < i32::MIN as f64 || value > i32::MAX as f64 {
        Err(BondModelError::NonFiniteOutput)
    } else {
        Ok(value as i32)
    }
}

fn scale_money(value: f64) -> Result<i64, BondModelError> {
    scale(value, MONEY_SCALE)
}

fn scale_metric(value: f64) -> Result<i64, BondModelError> {
    scale(value, METRIC_SCALE)
}

fn scale(value: f64, multiplier: f64) -> Result<i64, BondModelError> {
    let scaled = (value * multiplier).round();
    if !scaled.is_finite() || scaled < i64::MIN as f64 || scaled > i64::MAX as f64 {
        Err(BondModelError::NonFiniteOutput)
    } else {
        Ok(scaled as i64)
    }
}

fn input_digest(input: &BondModelInput) -> String {
    let mut hash = Fnv64::new();
    hash.text(MODEL_VERSION);
    hash.text(&input.instrument_id);
    hash.text(&input.currency);
    hash.i64(input.face_micros);
    hash.i64(i64::from(input.coupon_bps));
    hash.i64(i64::from(input.yield_bps));
    hash.u64(u64::from(input.years_to_maturity));
    hash.u64(u64::from(input.frequency.periods_per_year()));
    hash.u64(u64::from(input.accrued_period_bps));
    format!("BOND-{:016X}", hash.0)
}

struct Fnv64(u64);
impl Fnv64 {
    const fn new() -> Self {
        Self(14_695_981_039_346_656_037)
    }
    fn bytes(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = (self.0 ^ u64::from(*byte)).wrapping_mul(1_099_511_628_211);
        }
    }
    fn text(&mut self, value: &str) {
        self.bytes(value.as_bytes());
        self.bytes(&[0xff]);
    }
    fn i64(&mut self, value: i64) {
        self.bytes(&value.to_le_bytes());
    }
    fn u64(&mut self, value: u64) {
        self.bytes(&value.to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference() -> BondModelInput {
        BondModelInput {
            instrument_id: "REFERENCE".to_owned(),
            currency: "USD".to_owned(),
            face_micros: 100_000_000,
            coupon_bps: 500,
            yield_bps: 400,
            years_to_maturity: 5,
            frequency: CouponFrequency::SemiAnnual,
            accrued_period_bps: 0,
        }
    }

    #[test]
    fn matches_independent_standard_bond_reference_price() {
        let analytics = analyze_bond(&reference()).unwrap();
        assert!((analytics.clean_price_micros - 104_491_293).abs() <= 2);
        assert_eq!(analytics.clean_price_micros, analytics.dirty_price_micros);
        assert_eq!(analytics.cash_flows.len(), 10);
        assert_eq!(
            analytics.cash_flows.last().unwrap().principal_micros,
            100_000_000
        );
    }

    #[test]
    fn par_coupon_and_yield_reconcile_exactly_at_coupon_date() {
        let mut input = reference();
        input.yield_bps = input.coupon_bps;
        let analytics = analyze_bond(&input).unwrap();
        assert!((analytics.clean_price_micros - input.face_micros).abs() <= 1);
    }

    #[test]
    fn accrued_interest_reconciles_clean_and_dirty_price() {
        let mut input = reference();
        input.accrued_period_bps = 4_000;
        let analytics = analyze_bond(&input).unwrap();
        assert_eq!(analytics.accrued_interest_micros, 1_000_000);
        assert_eq!(
            analytics.dirty_price_micros - analytics.clean_price_micros,
            analytics.accrued_interest_micros
        );
    }

    #[test]
    fn yield_solver_round_trips_model_price() {
        let input = reference();
        let analytics = analyze_bond(&input).unwrap();
        assert_eq!(
            solve_yield_bps(&input, analytics.clean_price_micros).unwrap(),
            400
        );
    }

    #[test]
    fn duration_dv01_and_parallel_shocks_have_expected_direction() {
        let analytics = analyze_bond(&reference()).unwrap();
        assert!(analytics.macaulay_duration_years_millionths > 4_000_000);
        assert!(analytics.modified_duration_years_millionths > 4_000_000);
        assert!(analytics.convexity_years2_millionths > 0);
        assert!(analytics.dv01_micros > 0);
        assert!(analytics.scenarios.windows(2).all(|pair| {
            pair[0].yield_bps < pair[1].yield_bps
                && pair[0].clean_price_micros > pair[1].clean_price_micros
        }));
    }

    #[test]
    fn invalid_conventions_and_unbracketed_prices_fail_closed() {
        let mut input = reference();
        input.accrued_period_bps = 10_000;
        assert!(analyze_bond(&input).is_err());
        input = reference();
        assert_eq!(
            solve_yield_bps(&input, 0),
            Err(BondModelError::InvalidTargetPrice)
        );
        assert_eq!(
            solve_yield_bps(&input, i64::MAX),
            Err(BondModelError::YieldNotBracketed)
        );
    }
}
