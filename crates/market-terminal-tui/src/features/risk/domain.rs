use std::{collections::BTreeMap, fmt};

use crate::foundation::{Currency, InstrumentId, Money};

use super::HistoricalRiskSnapshot;

pub const SCENARIO_SHOCK_BPS: i32 = -1_000;
const MAX_RISK_POSITIONS: usize = 25_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RiskPositionInput {
    pub instrument_id: InstrumentId,
    pub account: String,
    pub symbol: String,
    pub currency: Currency,
    pub market_value: Option<Money>,
    pub cash: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RiskCurrencyInput {
    pub currency: Currency,
    pub priced_nav: Money,
    pub available_cash: Money,
    pub priced_positions: usize,
    pub unpriced_positions: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RiskInput {
    pub positions: Vec<RiskPositionInput>,
    pub currencies: Vec<RiskCurrencyInput>,
    pub source: String,
    pub as_of: String,
    pub input_version: String,
    pub disclosures: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RiskPositionExposure {
    pub instrument_id: InstrumentId,
    pub account: String,
    pub symbol: String,
    pub currency: Currency,
    pub market_value: Option<Money>,
    pub currency_weight_bps: Option<i32>,
    pub scenario_change: Option<Money>,
    pub cash: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RiskCurrencySummary {
    pub currency: Currency,
    pub priced_nav: Money,
    pub available_cash: Money,
    pub scenario_change: Money,
    pub largest_non_cash_weight_bps: Option<i32>,
    pub priced_positions: usize,
    pub unpriced_positions: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RiskSnapshot {
    pub positions: Vec<RiskPositionExposure>,
    pub currencies: Vec<RiskCurrencySummary>,
    pub source: String,
    pub as_of: String,
    pub input_version: String,
    pub methodology: String,
    pub disclosures: Vec<String>,
    pub historical: Option<HistoricalRiskSnapshot>,
}

impl RiskSnapshot {
    pub fn scenario_label(&self) -> String {
        match self.currencies.as_slice() {
            [] => "—".to_owned(),
            [currency] => format_money(currency.scenario_change),
            currencies => format!("{} CCY · SEE RISK", currencies.len()),
        }
    }

    pub fn largest_position_label(&self) -> String {
        match self.currencies.as_slice() {
            [] => "—".to_owned(),
            [currency] => currency
                .largest_non_cash_weight_bps
                .map(format_bps)
                .unwrap_or_else(|| "—".to_owned()),
            currencies => format!("{} CCY · PER-CCY", currencies.len()),
        }
    }

    pub fn priced_position_count(&self) -> usize {
        self.currencies
            .iter()
            .map(|currency| currency.priced_positions)
            .sum()
    }

    pub fn unpriced_position_count(&self) -> usize {
        self.currencies
            .iter()
            .map(|currency| currency.unpriced_positions)
            .sum()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RiskCalculationError(String);

impl fmt::Display for RiskCalculationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for RiskCalculationError {}

pub fn calculate_risk(input: RiskInput) -> Result<RiskSnapshot, RiskCalculationError> {
    if input.positions.len() > MAX_RISK_POSITIONS {
        return Err(calculation_error(format!(
            "position count exceeds {MAX_RISK_POSITIONS}"
        )));
    }
    validate_metadata(&input)?;

    let mut currency_inputs = BTreeMap::new();
    for currency in &input.currencies {
        validate_currency_input(currency)?;
        if currency_inputs
            .insert(currency.currency, *currency)
            .is_some()
        {
            return Err(calculation_error(format!(
                "duplicate currency total for {}",
                currency.currency
            )));
        }
    }

    let mut reconciliations = BTreeMap::<Currency, Reconciliation>::new();
    let mut positions = Vec::with_capacity(input.positions.len());
    for position in input.positions {
        validate_position(&position)?;
        let currency_input = currency_inputs.get(&position.currency).ok_or_else(|| {
            calculation_error(format!(
                "{} has no {} currency total",
                position.symbol, position.currency
            ))
        })?;
        let reconciliation = reconciliations.entry(position.currency).or_default();
        let (currency_weight_bps, scenario_change) = if let Some(market_value) =
            position.market_value
        {
            reconciliation.priced_positions += 1;
            reconciliation.priced_nav_minor = reconciliation
                .priced_nav_minor
                .checked_add(market_value.minor_units())
                .ok_or_else(|| calculation_error("priced NAV overflow"))?;
            if position.cash {
                reconciliation.cash_minor = reconciliation
                    .cash_minor
                    .checked_add(market_value.minor_units())
                    .ok_or_else(|| calculation_error("cash total overflow"))?;
            }
            let weight_bps = if currency_input.priced_nav.minor_units() == 0 {
                None
            } else {
                let numerator = market_value
                    .minor_units()
                    .checked_mul(10_000)
                    .ok_or_else(|| calculation_error("position weight overflow"))?;
                Some(
                    i32::try_from(rounded_division(
                        numerator,
                        currency_input.priced_nav.minor_units(),
                    ))
                    .map_err(|_| calculation_error("position weight exceeds its typed range"))?,
                )
            };
            let scenario_minor = if position.cash {
                0
            } else {
                let numerator = market_value
                    .minor_units()
                    .checked_mul(i128::from(SCENARIO_SHOCK_BPS))
                    .ok_or_else(|| calculation_error("scenario change overflow"))?;
                rounded_division(numerator, 10_000)
            };
            reconciliation.scenario_minor = reconciliation
                .scenario_minor
                .checked_add(scenario_minor)
                .ok_or_else(|| calculation_error("currency scenario overflow"))?;
            if !position.cash {
                if let Some(weight) = weight_bps {
                    reconciliation.largest_non_cash_weight_bps = Some(
                        reconciliation
                            .largest_non_cash_weight_bps
                            .unwrap_or_default()
                            .max(weight.saturating_abs()),
                    );
                }
            }
            (
                weight_bps,
                Some(Money::from_minor_units(scenario_minor, position.currency)),
            )
        } else {
            reconciliation.unpriced_positions += 1;
            (None, None)
        };
        positions.push(RiskPositionExposure {
            instrument_id: position.instrument_id,
            account: position.account,
            symbol: position.symbol,
            currency: position.currency,
            market_value: position.market_value,
            currency_weight_bps,
            scenario_change,
            cash: position.cash,
        });
    }

    let mut currencies = Vec::with_capacity(currency_inputs.len());
    for (currency, currency_input) in currency_inputs {
        let reconciliation = reconciliations.remove(&currency).unwrap_or_default();
        if reconciliation.priced_nav_minor != currency_input.priced_nav.minor_units()
            || reconciliation.cash_minor != currency_input.available_cash.minor_units()
            || reconciliation.priced_positions != currency_input.priced_positions
            || reconciliation.unpriced_positions != currency_input.unpriced_positions
        {
            return Err(calculation_error(format!(
                "{currency} positions do not reconcile to the versioned portfolio totals"
            )));
        }
        currencies.push(RiskCurrencySummary {
            currency,
            priced_nav: currency_input.priced_nav,
            available_cash: currency_input.available_cash,
            scenario_change: Money::from_minor_units(reconciliation.scenario_minor, currency),
            largest_non_cash_weight_bps: reconciliation.largest_non_cash_weight_bps,
            priced_positions: reconciliation.priced_positions,
            unpriced_positions: reconciliation.unpriced_positions,
        });
    }

    positions.sort_by(|left, right| {
        left.currency
            .cmp(&right.currency)
            .then_with(|| {
                left.market_value
                    .is_none()
                    .cmp(&right.market_value.is_none())
            })
            .then_with(|| {
                right
                    .market_value
                    .map(|money| money.minor_units().saturating_abs())
                    .unwrap_or_default()
                    .cmp(
                        &left
                            .market_value
                            .map(|money| money.minor_units().saturating_abs())
                            .unwrap_or_default(),
                    )
            })
            .then_with(|| left.symbol.cmp(&right.symbol))
            .then_with(|| left.account.cmp(&right.account))
    });

    let mut disclosures = input.disclosures;
    disclosures.push("POINT-IN-TIME CONCENTRATION · NOT A RETURN OR VOLATILITY MODEL".to_owned());
    disclosures
        .push("SCENARIO IS A PARALLEL -10% SHOCK TO PRICED NON-CASH MARKET VALUE".to_owned());
    if currencies.len() > 1 {
        disclosures.push("NO FX CONVERSION · RISK RESULTS REMAIN PER CURRENCY".to_owned());
    }
    if currencies
        .iter()
        .any(|currency| currency.unpriced_positions > 0)
    {
        disclosures.push("UNPRICED POSITIONS EXCLUDED FROM WEIGHTS AND SCENARIO".to_owned());
    }

    Ok(RiskSnapshot {
        positions,
        currencies,
        source: input.source,
        as_of: input.as_of,
        input_version: input.input_version,
        methodology: "MARKET-VALUE CONCENTRATION · CASH HELD FLAT · NON-CASH PARALLEL -10% SHOCK"
            .to_owned(),
        disclosures,
        historical: None,
    })
}

#[derive(Debug, Default)]
struct Reconciliation {
    priced_nav_minor: i128,
    cash_minor: i128,
    scenario_minor: i128,
    priced_positions: usize,
    unpriced_positions: usize,
    largest_non_cash_weight_bps: Option<i32>,
}

fn validate_metadata(input: &RiskInput) -> Result<(), RiskCalculationError> {
    for (field, value) in [
        ("source", input.source.as_str()),
        ("valuation time", input.as_of.as_str()),
        ("input version", input.input_version.as_str()),
    ] {
        if value.trim().is_empty() || value.len() > 1_024 {
            return Err(calculation_error(format!("{field} is empty or too long")));
        }
    }
    Ok(())
}

fn validate_currency_input(input: &RiskCurrencyInput) -> Result<(), RiskCalculationError> {
    if input.priced_nav.currency() != input.currency
        || input.available_cash.currency() != input.currency
    {
        return Err(calculation_error(format!(
            "{} total contains a currency mismatch",
            input.currency
        )));
    }
    Ok(())
}

fn validate_position(input: &RiskPositionInput) -> Result<(), RiskCalculationError> {
    if input.account.trim().is_empty()
        || input.symbol.trim().is_empty()
        || input.account.len() > 128
        || input.symbol.len() > 64
    {
        return Err(calculation_error("position identity is empty or too long"));
    }
    if input
        .market_value
        .is_some_and(|market_value| market_value.currency() != input.currency)
    {
        return Err(calculation_error(format!(
            "{} market value contains a currency mismatch",
            input.symbol
        )));
    }
    Ok(())
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

pub fn format_money(value: Money) -> String {
    let digits = value.currency().minor_unit_digits();
    let scale = 10_u128.pow(digits);
    let absolute = value.minor_units().unsigned_abs();
    let whole = group_digits(&(absolute / scale).to_string());
    let amount = if digits == 0 {
        whole
    } else {
        format!(
            "{whole}.{:0digits$}",
            absolute % scale,
            digits = digits as usize
        )
    };
    let sign = if value.minor_units() < 0 { "-" } else { "" };
    if value.currency().as_str() == "USD" {
        format!("{sign}${amount}")
    } else {
        format!("{sign}{} {amount}", value.currency())
    }
}

pub fn format_bps(value: i32) -> String {
    let sign = if value < 0 { "-" } else { "" };
    let absolute = value.unsigned_abs();
    format!("{sign}{}.{:02}%", absolute / 100, absolute % 100)
}

fn group_digits(value: &str) -> String {
    let mut grouped = String::new();
    for (index, character) in value.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(character);
    }
    grouped.chars().rev().collect()
}

fn calculation_error(message: impl Into<String>) -> RiskCalculationError {
    RiskCalculationError(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usd() -> Currency {
        Currency::new("USD").unwrap()
    }

    #[test]
    fn concentration_and_shock_reconcile_from_exact_money() {
        let usd = usd();
        let snapshot = calculate_risk(RiskInput {
            positions: vec![
                RiskPositionInput {
                    instrument_id: InstrumentId::new("us:xnas:aapl"),
                    account: "ACCOUNT 1".to_owned(),
                    symbol: "AAPL".to_owned(),
                    currency: usd,
                    market_value: Some(Money::from_minor_units(80_000, usd)),
                    cash: false,
                },
                RiskPositionInput {
                    instrument_id: InstrumentId::new("cash:usd"),
                    account: "ACCOUNT 1".to_owned(),
                    symbol: "CASH".to_owned(),
                    currency: usd,
                    market_value: Some(Money::from_minor_units(20_000, usd)),
                    cash: true,
                },
            ],
            currencies: vec![RiskCurrencyInput {
                currency: usd,
                priced_nav: Money::from_minor_units(100_000, usd),
                available_cash: Money::from_minor_units(20_000, usd),
                priced_positions: 2,
                unpriced_positions: 0,
            }],
            source: "TEST".to_owned(),
            as_of: "2026-08-27T20:00:00Z".to_owned(),
            input_version: "INPUT-1".to_owned(),
            disclosures: Vec::new(),
        })
        .unwrap();

        assert_eq!(
            snapshot.currencies[0].largest_non_cash_weight_bps,
            Some(8_000)
        );
        assert_eq!(snapshot.currencies[0].scenario_change.minor_units(), -8_000);
        assert_eq!(snapshot.scenario_label(), "-$80.00");
        assert_eq!(snapshot.positions[0].currency_weight_bps, Some(8_000));
    }

    #[test]
    fn multiple_currencies_and_missing_prices_remain_explicit() {
        let usd = usd();
        let eur = Currency::new("EUR").unwrap();
        let snapshot = calculate_risk(RiskInput {
            positions: vec![
                RiskPositionInput {
                    instrument_id: InstrumentId::new("us:xnas:aapl"),
                    account: "ACCOUNT 1".to_owned(),
                    symbol: "AAPL".to_owned(),
                    currency: usd,
                    market_value: Some(Money::from_minor_units(100_000, usd)),
                    cash: false,
                },
                RiskPositionInput {
                    instrument_id: InstrumentId::new("unresolved:portfolio:sap"),
                    account: "ACCOUNT 2".to_owned(),
                    symbol: "SAP".to_owned(),
                    currency: eur,
                    market_value: None,
                    cash: false,
                },
            ],
            currencies: vec![
                RiskCurrencyInput {
                    currency: usd,
                    priced_nav: Money::from_minor_units(100_000, usd),
                    available_cash: Money::from_minor_units(0, usd),
                    priced_positions: 1,
                    unpriced_positions: 0,
                },
                RiskCurrencyInput {
                    currency: eur,
                    priced_nav: Money::from_minor_units(0, eur),
                    available_cash: Money::from_minor_units(0, eur),
                    priced_positions: 0,
                    unpriced_positions: 1,
                },
            ],
            source: "TEST".to_owned(),
            as_of: "2026-08-27T20:00:00Z".to_owned(),
            input_version: "INPUT-2".to_owned(),
            disclosures: Vec::new(),
        })
        .unwrap();

        assert_eq!(snapshot.scenario_label(), "2 CCY · SEE RISK");
        assert_eq!(snapshot.unpriced_position_count(), 1);
        assert!(snapshot
            .disclosures
            .iter()
            .any(|disclosure| disclosure.contains("NO FX CONVERSION")));
        assert!(snapshot
            .disclosures
            .iter()
            .any(|disclosure| disclosure.contains("UNPRICED")));
    }

    #[test]
    fn mismatched_portfolio_totals_are_rejected() {
        let usd = usd();
        let error = calculate_risk(RiskInput {
            positions: vec![RiskPositionInput {
                instrument_id: InstrumentId::new("us:xnas:aapl"),
                account: "ACCOUNT 1".to_owned(),
                symbol: "AAPL".to_owned(),
                currency: usd,
                market_value: Some(Money::from_minor_units(100, usd)),
                cash: false,
            }],
            currencies: vec![RiskCurrencyInput {
                currency: usd,
                priced_nav: Money::from_minor_units(101, usd),
                available_cash: Money::from_minor_units(0, usd),
                priced_positions: 1,
                unpriced_positions: 0,
            }],
            source: "TEST".to_owned(),
            as_of: "NOW".to_owned(),
            input_version: "INPUT".to_owned(),
            disclosures: Vec::new(),
        })
        .unwrap_err();

        assert!(error.to_string().contains("do not reconcile"));
    }
}
