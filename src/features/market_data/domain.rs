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

/// Stable adapter identity carried with every observation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProviderId(String);

impl ProviderId {
    pub fn new(value: impl Into<String>) -> Self {
        let value = value.into();
        assert!(!value.trim().is_empty(), "provider id cannot be empty");
        Self(value)
    }

    pub fn as_str(&self) -> &str { &self.0 }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheStatus {
    /// Observation came directly from the provider.
    Live,
    /// Observation was served from a cache inside its freshness window.
    Fresh,
    /// Provider failed and this usable, explicitly stale observation was retained.
    LastKnownGood,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataProvenance {
    pub provider: ProviderId,
    pub source_timestamp: UtcTimestamp,
    pub received_at: UtcTimestamp,
    pub sequence: Option<u64>,
    pub cache_status: CacheStatus,
}

impl DataProvenance {
    pub fn live(
        provider: ProviderId,
        source_timestamp: UtcTimestamp,
        received_at: UtcTimestamp,
        sequence: Option<u64>,
    ) -> Self {
        Self {
            provider,
            source_timestamp,
            received_at,
            sequence,
            cache_status: CacheStatus::Live,
        }
    }
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
    pub provenance: DataProvenance,
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
    RateLimited { retry_after_ms: u64 },
    PermissionDenied(String),
    Cancelled,
    Unsupported(String),
    Provider {
        provider: ProviderId,
        message: String,
        retriable: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarketDataErrorKind {
    InvalidRequest,
    TemporarilyUnavailable,
    RateLimited,
    PermissionDenied,
    Cancelled,
    Unsupported,
    Provider,
}

impl MarketDataError {
    pub const fn kind(&self) -> MarketDataErrorKind {
        match self {
            Self::InvalidRequest(_) => MarketDataErrorKind::InvalidRequest,
            Self::TemporarilyUnavailable(_) => MarketDataErrorKind::TemporarilyUnavailable,
            Self::RateLimited { .. } => MarketDataErrorKind::RateLimited,
            Self::PermissionDenied(_) => MarketDataErrorKind::PermissionDenied,
            Self::Cancelled => MarketDataErrorKind::Cancelled,
            Self::Unsupported(_) => MarketDataErrorKind::Unsupported,
            Self::Provider { .. } => MarketDataErrorKind::Provider,
        }
    }

    pub const fn is_retriable(&self) -> bool {
        match self {
            Self::TemporarilyUnavailable(_) | Self::RateLimited { .. } => true,
            Self::Provider { retriable, .. } => *retriable,
            _ => false,
        }
    }
}

impl fmt::Display for MarketDataError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest(message) => write!(formatter, "invalid market data request: {message}"),
            Self::TemporarilyUnavailable(message) => {
                write!(formatter, "market data temporarily unavailable: {message}")
            }
            Self::RateLimited { retry_after_ms } => {
                write!(formatter, "market data rate limited; retry after {retry_after_ms}ms")
            }
            Self::PermissionDenied(message) => {
                write!(formatter, "market data permission denied: {message}")
            }
            Self::Cancelled => formatter.write_str("market data subscription cancelled"),
            Self::Unsupported(message) => write!(formatter, "unsupported market data operation: {message}"),
            Self::Provider { provider, message, .. } => {
                write!(formatter, "market data provider {} failed: {message}", provider.as_str())
            }
        }
    }
}

/// Deterministic exponential retry configuration. Adapters own sleeping and I/O.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    pub max_attempts: u8,
    pub initial_delay_ms: u64,
    pub max_delay_ms: u64,
    pub multiplier: u16,
}

impl RetryPolicy {
    pub fn new(
        max_attempts: u8,
        initial_delay_ms: u64,
        max_delay_ms: u64,
        multiplier: u16,
    ) -> Result<Self, MarketDataError> {
        if max_attempts == 0 {
            return Err(MarketDataError::InvalidRequest(
                "retry policy requires at least one attempt".to_owned(),
            ));
        }
        if initial_delay_ms > max_delay_ms || multiplier == 0 {
            return Err(MarketDataError::InvalidRequest(
                "retry delay bounds and multiplier are invalid".to_owned(),
            ));
        }
        Ok(Self { max_attempts, initial_delay_ms, max_delay_ms, multiplier })
    }

    /// `failed_attempt` is one-based: the delay after the first failed attempt is the initial delay.
    pub fn delay_after(&self, failed_attempt: u8) -> Option<u64> {
        if failed_attempt == 0 || failed_attempt >= self.max_attempts {
            return None;
        }
        let exponent = u32::from(failed_attempt - 1);
        let factor = u64::from(self.multiplier).saturating_pow(exponent);
        Some(self.initial_delay_ms.saturating_mul(factor).min(self.max_delay_ms))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimitPolicy {
    pub requests: u32,
    pub window_ms: u64,
    pub burst: u32,
}

impl RateLimitPolicy {
    pub fn new(requests: u32, window_ms: u64, burst: u32) -> Result<Self, MarketDataError> {
        if requests == 0 || window_ms == 0 || burst == 0 {
            return Err(MarketDataError::InvalidRequest(
                "rate limit values must be non-zero".to_owned(),
            ));
        }
        Ok(Self { requests, window_ms, burst })
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
            provenance: DataProvenance::live(
                ProviderId::new("test"),
                UtcTimestamp::new("2026-08-25T20:00:00Z"),
                UtcTimestamp::new("2026-08-25T20:00:01Z"),
                Some(1),
            ),
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

    #[test]
    fn retry_policy_caps_backoff_and_stops_at_attempt_budget() {
        let policy = RetryPolicy::new(5, 100, 750, 3).expect("valid retry policy");
        assert_eq!(policy.delay_after(1), Some(100));
        assert_eq!(policy.delay_after(2), Some(300));
        assert_eq!(policy.delay_after(3), Some(750));
        assert_eq!(policy.delay_after(5), None);
    }

    #[test]
    fn typed_errors_expose_retry_classification() {
        let error = MarketDataError::RateLimited { retry_after_ms: 250 };
        assert_eq!(error.kind(), MarketDataErrorKind::RateLimited);
        assert!(error.is_retriable());
        assert!(!MarketDataError::PermissionDenied("quotes".to_owned()).is_retriable());
    }
}
