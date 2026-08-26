use std::env;

use crate::features::{
    market_data::CanonicalInstrumentId,
    watchlist::{MonitorColumn, WatchlistCatalog, WatchlistDefinition, WatchlistItem},
};

const DEFAULT_SYMBOLS: &str = "IBM";
const MAX_SYMBOLS: usize = 50;

#[derive(Debug, Clone)]
pub struct ConfiguredWatchlistCatalog {
    symbols: Vec<String>,
}

impl ConfiguredWatchlistCatalog {
    pub fn from_env() -> Self {
        let configured = env::var("MARKET_TERMINAL_WATCHLIST")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_SYMBOLS.to_owned());
        let mut symbols = Vec::new();
        for candidate in configured.split(',') {
            let symbol = candidate.trim().to_ascii_uppercase();
            if is_valid_symbol(&symbol) && !symbols.contains(&symbol) {
                symbols.push(symbol);
            }
            if symbols.len() == MAX_SYMBOLS {
                break;
            }
        }
        if symbols.is_empty() {
            symbols.push(DEFAULT_SYMBOLS.to_owned());
        }
        Self { symbols }
    }
}

impl WatchlistCatalog for ConfiguredWatchlistCatalog {
    fn load_watchlist(&self, name: Option<&str>) -> Option<WatchlistDefinition> {
        if name.is_some_and(|name| {
            !matches!(
                name.trim().to_ascii_uppercase().as_str(),
                "DEFAULT" | "CORE"
            )
        }) {
            return None;
        }
        let items = self
            .symbols
            .iter()
            .map(|symbol| {
                WatchlistItem::new(
                    CanonicalInstrumentId::new(format!(
                        "us:listed:{}",
                        symbol.to_ascii_lowercase()
                    )),
                    symbol,
                    format!("{symbol} · CONFIGURED LIVE SYMBOL"),
                )
            })
            .collect();
        Some(
            WatchlistDefinition::new("configured", "LIVE WATCHLIST", items).with_columns(vec![
                MonitorColumn::Symbol,
                MonitorColumn::Last,
                MonitorColumn::Change,
                MonitorColumn::ChangePercent,
                MonitorColumn::Volume,
                MonitorColumn::Quality,
                MonitorColumn::AsOf,
            ]),
        )
    }
}

fn is_valid_symbol(symbol: &str) -> bool {
    !symbol.is_empty()
        && symbol.len() <= 32
        && symbol
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_validation_rejects_query_injection() {
        assert!(is_valid_symbol("IBM"));
        assert!(is_valid_symbol("BRK-B"));
        assert!(!is_valid_symbol("IBM&apikey=secret"));
    }
}
