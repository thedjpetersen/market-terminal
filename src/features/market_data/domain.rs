use std::fmt;

pub use crate::foundation::InstrumentId as CanonicalInstrumentId;

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Price(f64);

impl Price {
    pub const fn new(value: f64) -> Self { Self(value) }
    pub const fn value(self) -> f64 { self.0 }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Quantity(u64);

impl Quantity {
    pub const fn new(value: u64) -> Self { Self(value) }
    pub const fn value(self) -> u64 { self.0 }
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Percent(f64);

impl Percent {
    pub const fn new(value: f64) -> Self { Self(value) }
    pub const fn value(self) -> f64 { self.0 }
}

/// An ISO-8601 UTC timestamp supplied by an adapter.
///
/// Keeping the serialized representation at the port boundary avoids coupling
/// the domain to a particular clock crate while preserving observation time.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct UtcTimestamp(String);

impl UtcTimestamp {
    pub fn new(value: impl Into<String>) -> Self {
        let value = value.into();
        assert!(!value.trim().is_empty(), "UTC timestamp cannot be empty");
        Self(value)
    }

    pub fn as_str(&self) -> &str { &self.0 }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataQuality {
    RealTime,
    Delayed { minutes: u16 },
    Stale { age_seconds: u64 },
    Derived,
    Unavailable,
    PermissionDenied,
}

impl DataQuality {
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

    pub const fn is_usable(self) -> bool {
        !matches!(self, Self::Unavailable | Self::PermissionDenied)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PriceChange {
    pub absolute: Price,
    pub percent: Percent,
}

#[derive(Debug, Clone, PartialEq)]
pub struct QuoteSnapshot {
    pub instrument_id: CanonicalInstrumentId,
    pub symbol: String,
    pub currency: String,
    pub last: Option<Price>,
    pub change: Option<PriceChange>,
    pub bid: Option<Price>,
    pub ask: Option<Price>,
    pub volume: Option<Quantity>,
    pub as_of: UtcTimestamp,
    pub quality: DataQuality,
}

impl QuoteSnapshot {
    pub fn spread(&self) -> Option<Price> {
        Some(Price::new(self.ask?.value() - self.bid?.value()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarInterval {
    OneMinute,
    FiveMinutes,
    OneHour,
    OneDay,
    OneWeek,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryRequest {
    pub instrument_id: CanonicalInstrumentId,
    pub interval: BarInterval,
    pub start: UtcTimestamp,
    pub end: UtcTimestamp,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PriceBar {
    pub instrument_id: CanonicalInstrumentId,
    pub interval: BarInterval,
    pub opened_at: UtcTimestamp,
    pub open: Price,
    pub high: Price,
    pub low: Price,
    pub close: Price,
    pub volume: Quantity,
    pub quality: DataQuality,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarketDataError {
    InvalidRequest(String),
    TemporarilyUnavailable(String),
}

impl fmt::Display for MarketDataError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest(message) => write!(formatter, "invalid market data request: {message}"),
            Self::TemporarilyUnavailable(message) => {
                write!(formatter, "market data temporarily unavailable: {message}")
            }
        }
    }
}

impl std::error::Error for MarketDataError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_spread_is_derived_from_typed_prices() {
        let quote = QuoteSnapshot {
            instrument_id: CanonicalInstrumentId::new("us:xnas:aapl"),
            symbol: "AAPL".to_owned(),
            currency: "USD".to_owned(),
            last: Some(Price::new(205.30)),
            change: Some(PriceChange {
                absolute: Price::new(1.72),
                percent: Percent::new(0.84),
            }),
            bid: Some(Price::new(205.28)),
            ask: Some(Price::new(205.32)),
            volume: Some(Quantity::new(41_820_000)),
            as_of: UtcTimestamp::new("2026-08-25T20:00:00Z"),
            quality: DataQuality::RealTime,
        };

        let spread = quote.spread().expect("two-sided quote").value();
        assert!((spread - 0.04).abs() < 1e-10);
        assert!(quote.quality.is_usable());
    }

    #[test]
    fn entitlement_failure_is_explicitly_unusable() {
        assert!(!DataQuality::PermissionDenied.is_usable());
        assert_eq!(DataQuality::Delayed { minutes: 15 }.label(), "DELAYED 15M");
    }
}
