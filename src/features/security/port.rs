use super::{SecurityResearch, SecuritySnapshot};

pub trait SecurityQuery: Send + Sync {
    fn load_security(&self, symbol: &str) -> SecuritySnapshot;

    /// Optional research depth for adapters that have fundamentals, ownership,
    /// and filings. Existing adapters receive deterministic offline data.
    fn load_research(&self, symbol: &str) -> SecurityResearch {
        SecurityResearch::deterministic(symbol)
    }
}
