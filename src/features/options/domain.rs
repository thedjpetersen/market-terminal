use std::fmt;

pub const MODEL_VERSION: &str = "BLACK-SCHOLES-EUROPEAN-V1";
const YEAR_DAYS: f64 = 365.0;
const SCALE: f64 = 1_000_000.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionRight {
    Call,
    Put,
}

impl OptionRight {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Call => "CALL",
            Self::Put => "PUT",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionModelInput {
    pub symbol: String,
    pub right: OptionRight,
    pub spot_micros: i64,
    pub strike_micros: i64,
    pub days_to_expiry: u32,
    /// Annualized volatility in basis points; 2_500 means 25%.
    pub volatility_bps: u32,
    /// Continuously compounded annual risk-free rate in basis points.
    pub risk_free_rate_bps: i32,
    /// Continuously compounded annual dividend yield in basis points.
    pub dividend_yield_bps: i32,
    pub contract_multiplier: u32,
}

impl Default for OptionModelInput {
    fn default() -> Self {
        Self {
            symbol: "AAPL".to_owned(),
            right: OptionRight::Call,
            spot_micros: 190_000_000,
            strike_micros: 200_000_000,
            days_to_expiry: 30,
            volatility_bps: 2_500,
            risk_free_rate_bps: 500,
            dividend_yield_bps: 0,
            contract_multiplier: 100,
        }
    }
}

impl OptionModelInput {
    pub fn validate(&self) -> Result<(), OptionModelError> {
        if self.symbol.is_empty()
            || self.symbol.len() > 32
            || !self
                .symbol
                .chars()
                .all(|value| value.is_ascii_alphanumeric() || matches!(value, '.' | '-'))
        {
            return Err(OptionModelError::InvalidInput(
                "symbol must be 1-32 terminal identity characters".to_owned(),
            ));
        }
        if self.spot_micros <= 0
            || self.strike_micros <= 0
            || self.spot_micros > 1_000_000_000_000_000
            || self.strike_micros > 1_000_000_000_000_000
        {
            return Err(OptionModelError::InvalidInput(
                "spot and strike must be positive and at most 1 billion".to_owned(),
            ));
        }
        if self.days_to_expiry > 3_650 {
            return Err(OptionModelError::InvalidInput(
                "expiry must be between 0 and 3650 days".to_owned(),
            ));
        }
        if self.days_to_expiry > 0 && !(1..=20_000).contains(&self.volatility_bps) {
            return Err(OptionModelError::InvalidInput(
                "volatility must be between 0.01% and 200%".to_owned(),
            ));
        }
        if !(-5_000..=10_000).contains(&self.risk_free_rate_bps)
            || !(-5_000..=10_000).contains(&self.dividend_yield_bps)
        {
            return Err(OptionModelError::InvalidInput(
                "rates must be between -50% and 100%".to_owned(),
            ));
        }
        if !(1..=10_000).contains(&self.contract_multiplier) {
            return Err(OptionModelError::InvalidInput(
                "contract multiplier must be between 1 and 10000".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionScenario {
    pub spot_shock_bps: i32,
    pub volatility_shift_bps: i32,
    pub spot_micros: i64,
    pub volatility_bps: u32,
    pub price_micros: i64,
    pub contract_value_micros: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionAnalytics {
    pub input: OptionModelInput,
    pub price_micros: i64,
    pub intrinsic_micros: i64,
    pub time_value_micros: i64,
    pub delta_millionths: i64,
    pub gamma_billionths: i64,
    /// Price change for one volatility percentage point.
    pub vega_micros_per_point: i64,
    /// Price change for one calendar day passing, all else equal.
    pub theta_micros_per_day: i64,
    /// Price change for one interest-rate percentage point.
    pub rho_micros_per_point: i64,
    pub scenarios: Vec<OptionScenario>,
    pub model_version: &'static str,
    pub input_digest: String,
    pub methodology: &'static str,
    pub disclosures: [&'static str; 4],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OptionModelError {
    InvalidInput(String),
    NonFiniteOutput,
}

impl fmt::Display for OptionModelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(message) => {
                write!(formatter, "invalid option model input: {message}")
            }
            Self::NonFiniteOutput => write!(formatter, "option model produced a non-finite output"),
        }
    }
}

impl std::error::Error for OptionModelError {}

pub fn price_option(input: &OptionModelInput) -> Result<OptionAnalytics, OptionModelError> {
    input.validate()?;
    let values = black_scholes(input)?;
    let intrinsic_micros = scale(intrinsic(input))?;
    let price_micros = scale(values.price)?;
    let scenarios = scenario_grid(input)?;
    Ok(OptionAnalytics {
        input: input.clone(),
        price_micros,
        intrinsic_micros,
        time_value_micros: price_micros.saturating_sub(intrinsic_micros),
        delta_millionths: scale(values.delta)?,
        gamma_billionths: scale_by(values.gamma, 1_000_000_000.0)?,
        vega_micros_per_point: scale(values.vega * 0.01)?,
        theta_micros_per_day: scale(values.theta / YEAR_DAYS)?,
        rho_micros_per_point: scale(values.rho * 0.01)?,
        scenarios,
        model_version: MODEL_VERSION,
        input_digest: input_digest(input),
        methodology: "EUROPEAN BLACK-SCHOLES · CONTINUOUS RATES/DIVIDENDS · ACT/365",
        disclosures: [
            "MODEL OUTPUT ONLY · NOT A LIVE QUOTE OR INVESTMENT RECOMMENDATION",
            "MODEL GREEKS ARE SEPARATE FROM PROVIDER GREEKS; NONE ARE LOADED",
            "EUROPEAN EXERCISE · NO EARLY-EXERCISE OR DISCRETE-DIVIDEND MODEL",
            "SCENARIOS HOLD TIME, RATES, AND DIVIDENDS CONSTANT",
        ],
    })
}

#[derive(Clone, Copy)]
struct Values {
    price: f64,
    delta: f64,
    gamma: f64,
    vega: f64,
    theta: f64,
    rho: f64,
}

fn black_scholes(input: &OptionModelInput) -> Result<Values, OptionModelError> {
    let spot = input.spot_micros as f64 / SCALE;
    let strike = input.strike_micros as f64 / SCALE;
    if input.days_to_expiry == 0 {
        let delta = match input.right {
            OptionRight::Call if spot > strike => 1.0,
            OptionRight::Put if spot < strike => -1.0,
            _ => 0.0,
        };
        return Ok(Values {
            price: intrinsic(input),
            delta,
            gamma: 0.0,
            vega: 0.0,
            theta: 0.0,
            rho: 0.0,
        });
    }
    let years = f64::from(input.days_to_expiry) / YEAR_DAYS;
    let volatility = f64::from(input.volatility_bps) / 10_000.0;
    let rate = f64::from(input.risk_free_rate_bps) / 10_000.0;
    let dividend = f64::from(input.dividend_yield_bps) / 10_000.0;
    let root_time = years.sqrt();
    let d1 = ((spot / strike).ln() + (rate - dividend + volatility * volatility / 2.0) * years)
        / (volatility * root_time);
    let d2 = d1 - volatility * root_time;
    let discounted_spot = spot * (-dividend * years).exp();
    let discounted_strike = strike * (-rate * years).exp();
    let density = normal_pdf(d1);
    let gamma = (-dividend * years).exp() * density / (spot * volatility * root_time);
    let vega = discounted_spot * density * root_time;
    let (price, delta, theta, rho) = match input.right {
        OptionRight::Call => (
            discounted_spot * normal_cdf(d1) - discounted_strike * normal_cdf(d2),
            (-dividend * years).exp() * normal_cdf(d1),
            -(discounted_spot * density * volatility) / (2.0 * root_time)
                - rate * discounted_strike * normal_cdf(d2)
                + dividend * discounted_spot * normal_cdf(d1),
            years * discounted_strike * normal_cdf(d2),
        ),
        OptionRight::Put => (
            discounted_strike * normal_cdf(-d2) - discounted_spot * normal_cdf(-d1),
            (-dividend * years).exp() * (normal_cdf(d1) - 1.0),
            -(discounted_spot * density * volatility) / (2.0 * root_time)
                + rate * discounted_strike * normal_cdf(-d2)
                - dividend * discounted_spot * normal_cdf(-d1),
            -years * discounted_strike * normal_cdf(-d2),
        ),
    };
    let values = Values {
        price,
        delta,
        gamma,
        vega,
        theta,
        rho,
    };
    if [price, delta, gamma, vega, theta, rho]
        .into_iter()
        .all(f64::is_finite)
    {
        Ok(values)
    } else {
        Err(OptionModelError::NonFiniteOutput)
    }
}

fn intrinsic(input: &OptionModelInput) -> f64 {
    let spot = input.spot_micros as f64 / SCALE;
    let strike = input.strike_micros as f64 / SCALE;
    match input.right {
        OptionRight::Call => (spot - strike).max(0.0),
        OptionRight::Put => (strike - spot).max(0.0),
    }
}

fn scenario_grid(input: &OptionModelInput) -> Result<Vec<OptionScenario>, OptionModelError> {
    let mut scenarios = Vec::with_capacity(15);
    for spot_shock_bps in [-2_000, -1_000, 0, 1_000, 2_000] {
        for volatility_shift_bps in [-500, 0, 500] {
            let mut scenario = input.clone();
            scenario.spot_micros = i64::try_from(
                (i128::from(input.spot_micros) * i128::from(10_000 + spot_shock_bps)) / 10_000,
            )
            .map_err(|_| OptionModelError::NonFiniteOutput)?;
            scenario.volatility_bps = i64::from(input.volatility_bps)
                .saturating_add(i64::from(volatility_shift_bps))
                .clamp(1, 20_000) as u32;
            let price_micros = scale(black_scholes(&scenario)?.price)?;
            let contract_value_micros = price_micros
                .checked_mul(i64::from(input.contract_multiplier))
                .ok_or(OptionModelError::NonFiniteOutput)?;
            scenarios.push(OptionScenario {
                spot_shock_bps,
                volatility_shift_bps,
                spot_micros: scenario.spot_micros,
                volatility_bps: scenario.volatility_bps,
                price_micros,
                contract_value_micros,
            });
        }
    }
    Ok(scenarios)
}

fn normal_pdf(value: f64) -> f64 {
    (-value * value / 2.0).exp() / (2.0 * std::f64::consts::PI).sqrt()
}

/// Abramowitz-Stegun 7.1.26; maximum absolute error is approximately 7.5e-8.
fn normal_cdf(value: f64) -> f64 {
    let absolute = value.abs();
    let t = 1.0 / (1.0 + 0.231_641_9 * absolute);
    let polynomial = t
        * (0.319_381_530
            + t * (-0.356_563_782
                + t * (1.781_477_937 + t * (-1.821_255_978 + t * 1.330_274_429))));
    let positive = 1.0 - normal_pdf(absolute) * polynomial;
    if value >= 0.0 {
        positive
    } else {
        1.0 - positive
    }
}

fn scale(value: f64) -> Result<i64, OptionModelError> {
    scale_by(value, SCALE)
}

fn scale_by(value: f64, multiplier: f64) -> Result<i64, OptionModelError> {
    let value = (value * multiplier).round();
    if !value.is_finite() || value < i64::MIN as f64 || value > i64::MAX as f64 {
        Err(OptionModelError::NonFiniteOutput)
    } else {
        Ok(value as i64)
    }
}

fn input_digest(input: &OptionModelInput) -> String {
    let mut hash = Fnv64::new();
    hash.text(MODEL_VERSION);
    hash.text(&input.symbol);
    hash.u64(match input.right {
        OptionRight::Call => 1,
        OptionRight::Put => 2,
    });
    hash.i64(input.spot_micros);
    hash.i64(input.strike_micros);
    hash.u64(u64::from(input.days_to_expiry));
    hash.u64(u64::from(input.volatility_bps));
    hash.i64(i64::from(input.risk_free_rate_bps));
    hash.i64(i64::from(input.dividend_yield_bps));
    hash.u64(u64::from(input.contract_multiplier));
    format!("OPT-{:016X}", hash.0)
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
    fn u64(&mut self, value: u64) {
        self.bytes(&value.to_le_bytes());
    }
    fn i64(&mut self, value: i64) {
        self.bytes(&value.to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at_money(right: OptionRight) -> OptionModelInput {
        OptionModelInput {
            symbol: "TEST".to_owned(),
            right,
            spot_micros: 100_000_000,
            strike_micros: 100_000_000,
            days_to_expiry: 365,
            volatility_bps: 2_000,
            risk_free_rate_bps: 500,
            dividend_yield_bps: 0,
            contract_multiplier: 100,
        }
    }

    #[test]
    fn matches_independent_black_scholes_reference_case() {
        let call = price_option(&at_money(OptionRight::Call)).unwrap();
        let put = price_option(&at_money(OptionRight::Put)).unwrap();
        assert!((call.price_micros - 10_450_584).abs() < 100);
        assert!((put.price_micros - 5_573_526).abs() < 100);
        assert!((call.delta_millionths - 636_831).abs() < 20);
        assert!((call.gamma_billionths - 18_762_000).abs() < 2_000);
    }

    #[test]
    fn put_call_parity_reconciles_to_rounding_tolerance() {
        let input = at_money(OptionRight::Call);
        let call = price_option(&input).unwrap();
        let mut put_input = input.clone();
        put_input.right = OptionRight::Put;
        let put = price_option(&put_input).unwrap();
        let discounted_strike = 100.0 * (-0.05_f64).exp();
        let parity = call.price_micros as f64 / SCALE - put.price_micros as f64 / SCALE;
        assert!((parity - (100.0 - discounted_strike)).abs() < 0.0002);
    }

    #[test]
    fn expiry_is_intrinsic_with_zero_greeks_except_directional_delta() {
        let mut input = at_money(OptionRight::Put);
        input.days_to_expiry = 0;
        input.spot_micros = 90_000_000;
        input.volatility_bps = 0;
        let result = price_option(&input).unwrap();
        assert_eq!(result.price_micros, 10_000_000);
        assert_eq!(result.delta_millionths, -1_000_000);
        assert_eq!(result.gamma_billionths, 0);
        assert_eq!(result.scenarios.len(), 15);
    }

    #[test]
    fn scenarios_preserve_multiplier_and_are_reproducible() {
        let input = at_money(OptionRight::Call);
        let first = price_option(&input).unwrap();
        let second = price_option(&input).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.scenarios.len(), 15);
        assert!(first.scenarios.iter().all(|scenario| {
            scenario.contract_value_micros
                == scenario.price_micros * i64::from(input.contract_multiplier)
        }));
    }

    #[test]
    fn malformed_inputs_fail_closed() {
        let mut input = at_money(OptionRight::Call);
        input.spot_micros = 0;
        assert!(price_option(&input).is_err());
        input.spot_micros = 100_000_000;
        input.volatility_bps = 20_001;
        assert!(price_option(&input).is_err());
    }
}
