use super::WatchlistDefinition;

/// Persistence/read-model boundary owned by Watchlists & Monitors.
pub trait WatchlistCatalog: Send + Sync {
    /// `None` resolves the user's default watchlist.
    fn load_watchlist(&self, name: Option<&str>) -> Option<WatchlistDefinition>;
}
