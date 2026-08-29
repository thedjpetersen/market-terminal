use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use crate::features::{
    market_data::{MarketDataError, MarketDataQuery, QuoteSnapshot},
    screening::{ScreeningError, ScreeningUniverseQuery, UniverseMember, UniverseSnapshot},
    watchlist::WatchlistCatalog,
};

/// Composition-root translator from existing market-data and watchlist ports
/// into Screening's point-in-time universe vocabulary.
pub struct MarketScreeningUniverseQuery {
    market_data: Arc<dyn MarketDataQuery>,
    watchlists: Arc<dyn WatchlistCatalog>,
}

impl MarketScreeningUniverseQuery {
    pub fn new(
        market_data: Arc<dyn MarketDataQuery>,
        watchlists: Arc<dyn WatchlistCatalog>,
    ) -> Self {
        Self {
            market_data,
            watchlists,
        }
    }
}

impl ScreeningUniverseQuery for MarketScreeningUniverseQuery {
    fn load_universe(&self, id: &str) -> Result<UniverseSnapshot, ScreeningError> {
        let definition = self
            .watchlists
            .load_watchlist(Some(id))
            .or_else(|| {
                matches!(id.to_ascii_lowercase().as_str(), "core" | "default")
                    .then(|| self.watchlists.load_watchlist(None))
                    .flatten()
            })
            .ok_or_else(|| ScreeningError::UniverseNotFound(id.to_owned()))?;
        let identities = definition
            .items
            .iter()
            .map(|item| item.instrument_id.clone())
            .collect::<Vec<_>>();
        let quotes = self
            .market_data
            .quote_snapshots(&identities)
            .map_err(map_market_data_error)?;
        let mut quotes_by_id = quotes
            .into_iter()
            .map(|quote| (quote.instrument_id.as_str().to_owned(), quote))
            .collect::<BTreeMap<_, _>>();
        let mut providers = BTreeSet::new();
        let mut as_of = String::new();
        let members = definition
            .items
            .into_iter()
            .map(|item| {
                let quote = quotes_by_id.remove(item.instrument_id.as_str());
                if let Some(quote) = &quote {
                    providers.insert(quote.provenance.provider.as_str().to_owned());
                    if quote.as_of.as_str() > as_of.as_str() {
                        as_of = quote.as_of.as_str().to_owned();
                    }
                }
                translate_member(item.instrument_id, item.symbol, item.description, quote)
            })
            .collect::<Vec<_>>();
        if as_of.is_empty() {
            as_of = "UNOBSERVED".to_owned();
        }
        if providers.is_empty() {
            providers.insert("NO PROVIDER OBSERVATIONS".to_owned());
        }
        let source = providers.into_iter().collect::<Vec<_>>().join(" + ");
        let snapshot_id = id.to_ascii_lowercase();
        let version = universe_version(&snapshot_id, &as_of, &source, &members);
        UniverseSnapshot::new(
            snapshot_id,
            definition.name,
            version,
            as_of,
            source,
            members,
        )
        .map_err(|error| ScreeningError::InvalidSnapshot(error.to_string()))
    }
}

fn translate_member(
    instrument_id: crate::foundation::InstrumentId,
    symbol: String,
    description: String,
    quote: Option<QuoteSnapshot>,
) -> UniverseMember {
    let Some(quote) = quote else {
        return UniverseMember {
            instrument_id,
            symbol,
            description,
            currency: "UNKNOWN".to_owned(),
            last: None,
            change_percent: None,
            volume: None,
            spread_bps: None,
            day_range_percent: None,
            quality: "UNAVAILABLE".to_owned(),
            provider: "NO OBSERVATION".to_owned(),
        };
    };
    let last = quote.last.map(|price| price.value());
    let spread_bps = quote.bid.zip(quote.ask).and_then(|(bid, ask)| {
        let midpoint = (bid.value() + ask.value()) / 2.0;
        (midpoint > 0.0 && ask.value() >= bid.value())
            .then_some((ask.value() - bid.value()) / midpoint * 10_000.0)
    });
    let day_range_percent = quote.day_range().and_then(|(low, high)| {
        let denominator = last?;
        (denominator > 0.0).then_some((high.value() - low.value()) / denominator * 100.0)
    });
    UniverseMember {
        instrument_id,
        symbol,
        description,
        currency: quote.currency,
        last,
        change_percent: quote.change.map(|change| change.percent.value()),
        volume: quote.volume.map(|volume| volume.value() as f64),
        spread_bps,
        day_range_percent,
        quality: quote.quality.label(),
        provider: quote.provenance.provider.as_str().to_owned(),
    }
}

fn map_market_data_error(error: MarketDataError) -> ScreeningError {
    match error {
        MarketDataError::PermissionDenied(message) => ScreeningError::PermissionDenied(message),
        MarketDataError::InvalidRequest(message) => ScreeningError::InvalidSnapshot(message),
        other => ScreeningError::TemporarilyUnavailable(other.to_string()),
    }
}

fn universe_version(id: &str, as_of: &str, source: &str, members: &[UniverseMember]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    hash_text(&mut hash, id);
    hash_text(&mut hash, as_of);
    hash_text(&mut hash, source);
    hash_u64(&mut hash, members.len() as u64);
    for member in members {
        hash_text(&mut hash, member.instrument_id.as_str());
        hash_text(&mut hash, &member.symbol);
        hash_text(&mut hash, &member.description);
        hash_text(&mut hash, &member.currency);
        hash_optional_f64(&mut hash, member.last);
        hash_optional_f64(&mut hash, member.change_percent);
        hash_optional_f64(&mut hash, member.volume);
        hash_optional_f64(&mut hash, member.spread_bps);
        hash_optional_f64(&mut hash, member.day_range_percent);
        hash_text(&mut hash, &member.quality);
        hash_text(&mut hash, &member.provider);
    }
    hash
}

fn hash_text(hash: &mut u64, value: &str) {
    hash_u64(hash, value.len() as u64);
    for byte in value.bytes() {
        *hash ^= u64::from(byte);
        *hash = (*hash).wrapping_mul(0x100000001b3);
    }
}

fn hash_optional_f64(hash: &mut u64, value: Option<f64>) {
    match value {
        Some(value) => {
            hash_byte(hash, 1);
            hash_u64(hash, value.to_bits());
        }
        None => hash_byte(hash, 0),
    }
}

fn hash_u64(hash: &mut u64, value: u64) {
    for byte in value.to_le_bytes() {
        hash_byte(hash, byte);
    }
}

fn hash_byte(hash: &mut u64, byte: u8) {
    *hash ^= u64::from(byte);
    *hash = (*hash).wrapping_mul(0x100000001b3);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::{DemoMarketDataReplay, DemoWatchlistCatalog};

    #[test]
    fn translator_builds_a_versioned_provider_disclosed_universe() {
        let query = MarketScreeningUniverseQuery::new(
            Arc::new(DemoMarketDataReplay::new()),
            Arc::new(DemoWatchlistCatalog),
        );
        let snapshot = query.load_universe("core").unwrap();
        let next = query.load_universe("core").unwrap();
        let replayed = MarketScreeningUniverseQuery::new(
            Arc::new(DemoMarketDataReplay::new()),
            Arc::new(DemoWatchlistCatalog),
        )
        .load_universe("core")
        .unwrap();

        assert_eq!(snapshot.id, "core");
        assert_eq!(snapshot.members.len(), 5);
        assert!(snapshot.version > 0);
        assert_ne!(snapshot.version, next.version);
        assert_eq!(snapshot.version, replayed.version);
        assert_eq!(snapshot.source, "demo-replay");
        assert!(snapshot
            .members
            .iter()
            .all(|member| !member.provider.is_empty()));
    }

    #[test]
    fn missing_universe_is_explicit() {
        let query = MarketScreeningUniverseQuery::new(
            Arc::new(DemoMarketDataReplay::new()),
            Arc::new(DemoWatchlistCatalog),
        );
        assert_eq!(
            query.load_universe("unknown").unwrap_err(),
            ScreeningError::UniverseNotFound("unknown".to_owned())
        );
    }
}
