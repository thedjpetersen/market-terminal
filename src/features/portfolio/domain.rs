use crate::foundation::{Currency, InstrumentId, Money};

const QUANTITY_SCALE: i128 = 1_000_000;

/// Import-local account identity that never retains a broker account number.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PortfolioAccountId(String);

impl PortfolioAccountId {
    pub fn new(value: impl Into<String>) -> Self {
        let value = value.into();
        assert!(
            !value.trim().is_empty(),
            "portfolio account ID cannot be empty"
        );
        Self(value)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Signed position quantity with six deterministic decimal places.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PositionQuantity(i128);

impl PositionQuantity {
    pub const fn from_scaled_units(value: i128) -> Self {
        Self(value)
    }

    pub const fn scaled_units(self) -> i128 {
        self.0
    }

    pub fn label(self) -> String {
        format_scaled(self.0, 6)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Position {
    pub instrument_id: InstrumentId,
    pub account_id: PortfolioAccountId,
    pub symbol: String,
    pub currency: Currency,
    pub quantity: PositionQuantity,
    pub average_cost: Option<Money>,
    pub market_value: Option<Money>,
    pub unrealized_return_bps: Option<i32>,
    pub weight_bps: Option<i32>,
    pub cash: bool,
}

impl Position {
    pub fn quantity_label(&self) -> String {
        self.quantity.label()
    }

    pub fn average_cost_label(&self) -> String {
        self.average_cost
            .map(format_money)
            .unwrap_or_else(|| "—".to_owned())
    }

    pub fn market_value_label(&self) -> String {
        self.market_value
            .map(format_money)
            .unwrap_or_else(|| "UNPRICED".to_owned())
    }

    pub fn pnl_label(&self) -> String {
        self.unrealized_return_bps
            .map(format_signed_bps)
            .unwrap_or_else(|| "—".to_owned())
    }

    pub fn weight_label(&self) -> String {
        self.weight_bps
            .map(format_bps)
            .unwrap_or_else(|| "—".to_owned())
    }

    pub const fn currency(&self) -> Currency {
        self.currency
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortfolioCurrencyTotal {
    pub currency: Currency,
    pub net_asset_value: Money,
    pub available_cash: Money,
    pub priced_positions: usize,
    pub unpriced_positions: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortfolioSnapshot {
    pub positions: Vec<Position>,
    pub currency_totals: Vec<PortfolioCurrencyTotal>,
    pub ytd_return_bps: Option<i32>,
    pub sharpe_hundredths: Option<i32>,
    pub source: String,
    pub as_of: String,
    pub input_version: String,
    pub methodology: String,
    pub disclosures: Vec<String>,
}

impl PortfolioSnapshot {
    pub fn empty(source: impl Into<String>) -> Self {
        Self {
            positions: Vec::new(),
            currency_totals: Vec::new(),
            ytd_return_bps: None,
            sharpe_hundredths: None,
            source: source.into(),
            as_of: "—".to_owned(),
            input_version: "—".to_owned(),
            methodology: "NO PORTFOLIO INPUT".to_owned(),
            disclosures: vec!["IMPORT A POSITION SNAPSHOT TO BEGIN".to_owned()],
        }
    }

    pub fn net_asset_value_label(&self) -> String {
        match self.currency_totals.as_slice() {
            [] => "—".to_owned(),
            [total] if total.unpriced_positions == 0 => format_money(total.net_asset_value),
            [total] => format!(
                "{} PRICED · {} UNPRICED",
                format_money(total.net_asset_value),
                total.unpriced_positions
            ),
            totals => format!("{} CURRENCIES · SEE PORT", totals.len()),
        }
    }

    pub fn available_cash_label(&self) -> String {
        match self.currency_totals.as_slice() {
            [] => "—".to_owned(),
            [total] => format_money(total.available_cash),
            totals => format!("{} CURRENCIES · SEE PORT", totals.len()),
        }
    }

    pub fn ytd_return_label(&self) -> String {
        self.ytd_return_bps
            .map(format_signed_bps)
            .unwrap_or_else(|| "N/A".to_owned())
    }

    pub fn sharpe_label(&self) -> String {
        self.sharpe_hundredths
            .map(|value| {
                let sign = if value < 0 { "-" } else { "" };
                let absolute = value.unsigned_abs();
                format!("{sign}{}.{:02}", absolute / 100, absolute % 100)
            })
            .unwrap_or_else(|| "N/A".to_owned())
    }
}

pub fn format_money(value: Money) -> String {
    let minor_units = value.minor_units();
    let negative = minor_units.is_negative();
    let absolute = minor_units.unsigned_abs();
    let digits = value.currency().minor_unit_digits();
    let scale = 10_u128.pow(digits);
    let whole = absolute / scale;
    let fraction = absolute % scale;
    let grouped = group_digits(&whole.to_string());
    let sign = if negative { "-" } else { "" };
    let amount = if digits == 0 {
        grouped
    } else {
        format!("{grouped}.{fraction:0digits$}", digits = digits as usize)
    };
    if value.currency().as_str() == "USD" {
        format!("{sign}${amount}")
    } else {
        format!("{sign}{} {amount}", value.currency())
    }
}

fn format_signed_bps(value: i32) -> String {
    let sign = if value >= 0 { "+" } else { "-" };
    let absolute = value.unsigned_abs();
    format!("{sign}{}.{:02}%", absolute / 100, absolute % 100)
}

fn format_bps(value: i32) -> String {
    let absolute = value.unsigned_abs();
    let sign = if value < 0 { "-" } else { "" };
    format!("{sign}{}.{:02}%", absolute / 100, absolute % 100)
}

fn format_scaled(value: i128, decimals: u32) -> String {
    let scale = 10_i128.pow(decimals);
    debug_assert_eq!(scale, QUANTITY_SCALE);
    let negative = value.is_negative();
    let absolute = value.unsigned_abs();
    let whole = absolute / scale as u128;
    let fraction = absolute % scale as u128;
    let mut result = group_digits(&whole.to_string());
    if fraction > 0 {
        let fraction = format!("{fraction:0decimals$}", decimals = decimals as usize)
            .trim_end_matches('0')
            .to_owned();
        result.push('.');
        result.push_str(&fraction);
    }
    if negative {
        result.insert(0, '-');
    }
    result
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantities_and_money_render_from_exact_units() {
        let usd = Currency::new("USD").unwrap();
        let eur = Currency::new("EUR").unwrap();

        assert_eq!(
            PositionQuantity::from_scaled_units(1_234_567_000).label(),
            "1,234.567"
        );
        assert_eq!(
            format_money(Money::from_minor_units(123_456, usd)),
            "$1,234.56"
        );
        assert_eq!(
            format_money(Money::from_minor_units(-123_456, eur)),
            "-EUR 1,234.56"
        );
        let jpy = Currency::new("JPY").unwrap();
        let kwd = Currency::new("KWD").unwrap();
        assert_eq!(
            format_money(Money::from_minor_units(123_456, jpy)),
            "JPY 123,456"
        );
        assert_eq!(
            format_money(Money::from_minor_units(123_456, kwd)),
            "KWD 123.456"
        );
    }

    #[test]
    fn multi_currency_snapshot_refuses_to_invent_a_combined_nav() {
        let usd = Currency::new("USD").unwrap();
        let eur = Currency::new("EUR").unwrap();
        let mut snapshot = PortfolioSnapshot::empty("TEST");
        snapshot.currency_totals = vec![
            PortfolioCurrencyTotal {
                currency: usd,
                net_asset_value: Money::from_minor_units(10_000, usd),
                available_cash: Money::from_minor_units(10_00, usd),
                priced_positions: 1,
                unpriced_positions: 0,
            },
            PortfolioCurrencyTotal {
                currency: eur,
                net_asset_value: Money::from_minor_units(20_000, eur),
                available_cash: Money::from_minor_units(0, eur),
                priced_positions: 1,
                unpriced_positions: 0,
            },
        ];

        assert_eq!(snapshot.net_asset_value_label(), "2 CURRENCIES · SEE PORT");
        assert_eq!(snapshot.available_cash_label(), "2 CURRENCIES · SEE PORT");
    }

    #[test]
    fn negative_sub_unit_sharpe_keeps_its_sign() {
        let mut snapshot = PortfolioSnapshot::empty("TEST");
        snapshot.sharpe_hundredths = Some(-50);

        assert_eq!(snapshot.sharpe_label(), "-0.50");
    }
}
