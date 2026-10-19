use std::collections::HashSet;

pub use crate::foundation::InstrumentId as CanonicalInstrumentId;

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Price(f64);

impl Price {
    pub const fn new(value: f64) -> Self {
        Self(value)
    }
    pub const fn value(self) -> f64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Quantity(u64);

impl Quantity {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }
    pub const fn value(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Percent(f64);

impl Percent {
    pub const fn new(value: f64) -> Self {
        Self(value)
    }
    pub const fn value(self) -> f64 {
        self.0
    }
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

    pub fn as_str(&self) -> &str {
        &self.0
    }
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

    pub fn as_str(&self) -> &str {
        &self.0
    }
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
    /// Provider-reported low for the current trading day, when available.
    pub day_low: Option<Price>,
    /// Provider-reported high for the current trading day, when available.
    pub day_high: Option<Price>,
    pub volume: Option<Quantity>,
    pub as_of: UtcTimestamp,
    pub quality: DataQuality,
    pub provenance: DataProvenance,
}

impl QuoteSnapshot {
    pub fn spread(&self) -> Option<Price> {
        Some(Price::new(self.ask?.value() - self.bid?.value()))
    }

    pub fn day_range(&self) -> Option<(Price, Price)> {
        let low = self.day_low?;
        let high = self.day_high?;
        (low.value() <= high.value()).then_some((low, high))
    }
}

#[derive(Debug, Clone)]
pub enum MarketDataError {
    InvalidRequest(String),
    Unavailable(String),
    Unsupported(String),
    Cancelled,
}
impl std::fmt::Display for MarketDataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRequest(s) | Self::Unavailable(s) | Self::Unsupported(s) => f.write_str(s),
            Self::Cancelled => f.write_str("subscription cancelled"),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuoteSubscriptionRequest {
    instruments: Vec<CanonicalInstrumentId>,
    capacity: usize,
}

impl QuoteSubscriptionRequest {
    pub fn new(
        instruments: Vec<CanonicalInstrumentId>,
        capacity: usize,
    ) -> Result<Self, MarketDataError> {
        if instruments.is_empty() {
            return Err(MarketDataError::InvalidRequest(
                "quote subscription requires at least one instrument".to_owned(),
            ));
        }
        if capacity == 0 {
            return Err(MarketDataError::InvalidRequest(
                "quote subscription capacity must be non-zero".to_owned(),
            ));
        }

        let mut seen = HashSet::new();
        let instruments = instruments
            .into_iter()
            .filter(|instrument| seen.insert(instrument.clone()))
            .collect();
        Ok(Self {
            instruments,
            capacity,
        })
    }

    pub fn instruments(&self) -> &[CanonicalInstrumentId] {
        &self.instruments
    }
    pub const fn capacity(&self) -> usize {
        self.capacity
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SubscriptionId(u64);

impl SubscriptionId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }
    pub const fn value(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct QuoteUpdate {
    pub snapshot: QuoteSnapshot,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SubscriptionMetrics {
    pub received: u64,
    pub coalesced: u64,
    pub dropped: u64,
    pub pending: usize,
}
