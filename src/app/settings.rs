//! The first-run/effective-settings flow is inspired by `makeev/alphai-tui`
//! commit `9143d2e1176d0a67a9f26960427cf370187fc2e6` (MIT, Copyright (c) 2026
//! Mikhail Makeev). This implementation is specific to Market Terminal's shell;
//! see `THIRD_PARTY_NOTICES.md`.

/// Secret-free snapshot of the effective startup configuration.
///
/// Providers are composed before the event loop starts. The settings overlay
/// therefore reports exactly what this process is using and clearly marks the
/// values that require a restart when changed in `.env` or the environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeSettingsSummary {
    pub gallery_replay: bool,
    pub market_provider: String,
    pub market_credentials: String,
    pub quote_refresh_seconds: u64,
    pub watchlist: String,
    pub market_symbols: String,
    pub chart_symbol: String,
    pub ai_provider: String,
    pub portfolio_import: String,
    pub news_sources: String,
    pub irc: String,
}

impl RuntimeSettingsSummary {
    pub fn demo() -> Self {
        Self {
            gallery_replay: true,
            market_provider: "DETERMINISTIC GALLERY REPLAY".to_owned(),
            market_credentials: "NOT USED".to_owned(),
            quote_refresh_seconds: 60,
            watchlist: "BUILT-IN GALLERY SYMBOLS".to_owned(),
            market_symbols: "BUILT-IN GALLERY MARKETS".to_owned(),
            chart_symbol: "AAPL".to_owned(),
            ai_provider: "LOCAL TEST GATEWAY".to_owned(),
            portfolio_import: "GALLERY SNAPSHOT".to_owned(),
            news_sources: "GALLERY SNAPSHOT".to_owned(),
            irc: "LOCAL TEST GATEWAY".to_owned(),
        }
    }
}
