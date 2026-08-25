use std::fmt;

/// Provider-neutral identity shared by instrument-centered bounded contexts.
///
/// Symbols and provider keys are display/mapping concerns; this value remains
/// stable across venues, adapters, watchlists, charts, and saved documents.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct InstrumentId(String);

impl InstrumentId {
    pub fn new(value: impl Into<String>) -> Self {
        let value = value.into();
        assert!(!value.trim().is_empty(), "instrument ID cannot be empty");
        Self(value)
    }

    pub fn as_str(&self) -> &str { &self.0 }
}

impl fmt::Display for InstrumentId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_is_stable_and_provider_neutral() {
        let id = InstrumentId::new("us:xnas:aapl");
        assert_eq!(id.as_str(), "us:xnas:aapl");
        assert_eq!(id.to_string(), "us:xnas:aapl");
    }

    #[test]
    #[should_panic(expected = "instrument ID cannot be empty")]
    fn empty_identity_is_rejected() {
        let _ = InstrumentId::new("  ");
    }
}
