use std::{fmt, str::FromStr};

/// ISO-4217-style currency code used at domain boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Currency([u8; 3]);

impl Currency {
    pub fn new(value: &str) -> Result<Self, ValueError> {
        let bytes = value.as_bytes();
        if bytes.len() != 3 || !bytes.iter().all(u8::is_ascii_alphabetic) {
            return Err(ValueError::InvalidCurrency(value.to_owned()));
        }
        Ok(Self([
            bytes[0].to_ascii_uppercase(),
            bytes[1].to_ascii_uppercase(),
            bytes[2].to_ascii_uppercase(),
        ]))
    }

    pub fn as_str(&self) -> &str {
        // Construction guarantees three ASCII bytes, which are valid UTF-8.
        std::str::from_utf8(&self.0).expect("currency is validated ASCII")
    }

    /// ISO 4217 minor-unit exponent for monetary display and decimal import.
    ///
    /// Codes not listed as an ISO exception use the common two-digit exponent.
    pub const fn minor_unit_digits(self) -> u32 {
        match self.0 {
            [b'B', b'I', b'F']
            | [b'C', b'L', b'P']
            | [b'D', b'J', b'F']
            | [b'G', b'N', b'F']
            | [b'I', b'S', b'K']
            | [b'J', b'P', b'Y']
            | [b'K', b'M', b'F']
            | [b'K', b'R', b'W']
            | [b'P', b'Y', b'G']
            | [b'R', b'W', b'F']
            | [b'U', b'G', b'X']
            | [b'U', b'Y', b'I']
            | [b'V', b'N', b'D']
            | [b'V', b'U', b'V']
            | [b'X', b'A', b'F']
            | [b'X', b'O', b'F']
            | [b'X', b'P', b'F'] => 0,
            [b'B', b'H', b'D']
            | [b'I', b'Q', b'D']
            | [b'J', b'O', b'D']
            | [b'K', b'W', b'D']
            | [b'L', b'Y', b'D']
            | [b'O', b'M', b'R']
            | [b'T', b'N', b'D'] => 3,
            [b'C', b'L', b'F'] | [b'U', b'Y', b'W'] => 4,
            _ => 2,
        }
    }
}

impl FromStr for Currency {
    type Err = ValueError;

    fn from_str(value: &str) -> Result<Self, Self::Err> { Self::new(value) }
}

impl fmt::Display for Currency {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Exact monetary value represented in the currency's minor units.
///
/// Adapters perform decimal conversion; domain arithmetic never relies on a
/// binary floating-point representation for cash balances or ledger entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Money {
    minor_units: i128,
    currency: Currency,
}

impl Money {
    pub const fn from_minor_units(minor_units: i128, currency: Currency) -> Self {
        Self { minor_units, currency }
    }

    pub const fn minor_units(self) -> i128 { self.minor_units }
    pub const fn currency(self) -> Currency { self.currency }

    pub fn checked_add(self, other: Self) -> Result<Self, ValueError> {
        self.require_same_currency(other)?;
        self.minor_units
            .checked_add(other.minor_units)
            .map(|minor_units| Self::from_minor_units(minor_units, self.currency))
            .ok_or(ValueError::ArithmeticOverflow)
    }

    pub fn checked_sub(self, other: Self) -> Result<Self, ValueError> {
        self.require_same_currency(other)?;
        self.minor_units
            .checked_sub(other.minor_units)
            .map(|minor_units| Self::from_minor_units(minor_units, self.currency))
            .ok_or(ValueError::ArithmeticOverflow)
    }

    fn require_same_currency(self, other: Self) -> Result<(), ValueError> {
        if self.currency == other.currency {
            Ok(())
        } else {
            Err(ValueError::CurrencyMismatch { left: self.currency, right: other.currency })
        }
    }
}

/// Finite observed or calculated price.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Price(f64);

impl Price {
    pub fn new(value: f64) -> Result<Self, ValueError> {
        value.is_finite().then_some(Self(value)).ok_or(ValueError::NonFiniteNumber)
    }

    pub const fn value(self) -> f64 { self.0 }
}

/// Non-negative whole-unit quantity for quote volume and count-like positions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Quantity(u64);

impl Quantity {
    pub const fn new(value: u64) -> Self { Self(value) }
    pub const fn value(self) -> u64 { self.0 }
}

/// A normalized UTC observation timestamp.
///
/// This lightweight value validates the transport shape without coupling every
/// bounded context to a clock/date crate. Clock-aware adapters remain
/// responsible for calendar arithmetic.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct UtcTimestamp(String);

impl UtcTimestamp {
    pub fn new(value: impl Into<String>) -> Result<Self, ValueError> {
        let value = value.into();
        let bytes = value.as_bytes();
        let valid_shape = bytes.len() >= 20
            && bytes.get(4) == Some(&b'-')
            && bytes.get(7) == Some(&b'-')
            && bytes.get(10) == Some(&b'T')
            && bytes.get(13) == Some(&b':')
            && bytes.get(16) == Some(&b':')
            && value.ends_with('Z');
        if !valid_shape {
            return Err(ValueError::InvalidUtcTimestamp(value));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str { &self.0 }
}

impl fmt::Display for UtcTimestamp {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Quality and entitlement state carried with external observations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DataQuality {
    RealTime,
    Delayed { minutes: u16 },
    Stale { age_seconds: u64 },
    Derived,
    Unavailable,
    PermissionDenied,
}

impl DataQuality {
    pub const fn is_usable(self) -> bool {
        !matches!(self, Self::Unavailable | Self::PermissionDenied)
    }

    pub fn label(self) -> String {
        match self {
            Self::RealTime => "REALTIME".to_owned(),
            Self::Delayed { minutes } => format!("DELAYED {minutes}M"),
            Self::Stale { age_seconds } => format!("STALE {age_seconds}S"),
            Self::Derived => "DERIVED".to_owned(),
            Self::Unavailable => "UNAVAILABLE".to_owned(),
            Self::PermissionDenied => "NO ENTITLEMENT".to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueError {
    InvalidCurrency(String),
    CurrencyMismatch { left: Currency, right: Currency },
    InvalidUtcTimestamp(String),
    NonFiniteNumber,
    ArithmeticOverflow,
}

impl fmt::Display for ValueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCurrency(value) => write!(formatter, "invalid currency code: {value}"),
            Self::CurrencyMismatch { left, right } => {
                write!(formatter, "currency mismatch: {left} and {right}")
            }
            Self::InvalidUtcTimestamp(value) => write!(formatter, "invalid UTC timestamp: {value}"),
            Self::NonFiniteNumber => formatter.write_str("number must be finite"),
            Self::ArithmeticOverflow => formatter.write_str("arithmetic overflow"),
        }
    }
}

impl std::error::Error for ValueError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn currency_is_normalized_and_money_requires_matching_units() {
        let usd = Currency::new("usd").expect("USD");
        let eur = Currency::new("EUR").expect("EUR");
        assert_eq!(usd.as_str(), "USD");
        assert_eq!(
            Money::from_minor_units(125, usd)
                .checked_add(Money::from_minor_units(75, usd))
                .expect("matching currencies")
                .minor_units(),
            200
        );
        assert!(matches!(
            Money::from_minor_units(1, usd).checked_add(Money::from_minor_units(1, eur)),
            Err(ValueError::CurrencyMismatch { .. })
        ));
    }

    #[test]
    fn external_values_reject_ambiguous_or_non_finite_inputs() {
        assert!(Currency::new("US").is_err());
        assert!(Price::new(f64::NAN).is_err());
        assert!(UtcTimestamp::new("2026-08-25 20:00:00").is_err());
        assert_eq!(
            UtcTimestamp::new("2026-08-25T20:00:00Z").expect("UTC").as_str(),
            "2026-08-25T20:00:00Z"
        );
    }

    #[test]
    fn quality_never_hides_entitlement_failure() {
        assert!(!DataQuality::PermissionDenied.is_usable());
        assert_eq!(DataQuality::Delayed { minutes: 15 }.label(), "DELAYED 15M");
    }
}
