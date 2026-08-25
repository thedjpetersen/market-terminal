use crate::features::instrument::{Instrument, InstrumentId, InstrumentKind, InstrumentSearch};

pub struct DemoInstrumentSearch;

impl InstrumentSearch for DemoInstrumentSearch {
    fn search(&self, query: &str, limit: usize) -> Vec<Instrument> {
        let query = query.trim().to_ascii_uppercase();
        let mut matches = instruments()
            .into_iter()
            .filter_map(|instrument| {
                let symbol = instrument.symbol.to_ascii_uppercase();
                let name = instrument.name.to_ascii_uppercase();
                let id = instrument.id.as_str().to_ascii_uppercase();
                let score = if query.is_empty() || symbol == query {
                    0
                } else if symbol.starts_with(&query) {
                    1
                } else if name.starts_with(&query) {
                    2
                } else if name.contains(&query) {
                    3
                } else if id.contains(&query) {
                    4
                } else {
                    return None;
                };
                Some((score, instrument))
            })
            .collect::<Vec<_>>();
        matches.sort_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then_with(|| left.1.symbol.cmp(&right.1.symbol))
        });
        matches
            .into_iter()
            .take(limit)
            .map(|(_, instrument)| instrument)
            .collect()
    }
}

fn instruments() -> Vec<Instrument> {
    vec![
        instrument("us:xnas:aapl", "AAPL", "Apple Inc.", "US", "USD", InstrumentKind::Equity),
        instrument("us:xnas:msft", "MSFT", "Microsoft Corporation", "US", "USD", InstrumentKind::Equity),
        instrument("us:xnas:nvda", "NVDA", "NVIDIA Corporation", "US", "USD", InstrumentKind::Equity),
        instrument("us:xnas:meta", "META", "Meta Platforms Inc.", "US", "USD", InstrumentKind::Equity),
        instrument("us:arcx:spy", "SPY", "SPDR S&P 500 ETF Trust", "US", "USD", InstrumentKind::Etf),
        instrument("us:arcx:vt", "VT", "Vanguard Total World Stock ETF", "US", "USD", InstrumentKind::Etf),
        instrument("index:spx", "SPX", "S&P 500 Index", "INDEX", "USD", InstrumentKind::Index),
        instrument("index:ndx", "NDX", "NASDAQ 100 Index", "INDEX", "USD", InstrumentKind::Index),
        instrument("fx:eurusd", "EURUSD", "Euro / U.S. Dollar", "FX", "USD", InstrumentKind::Currency),
        instrument("fx:usdjpy", "USDJPY", "U.S. Dollar / Japanese Yen", "FX", "JPY", InstrumentKind::Currency),
        instrument("commodity:xau", "XAU", "Gold Spot", "SPOT", "USD", InstrumentKind::Commodity),
        instrument("commodity:cl", "CL", "WTI Crude Oil", "NYMEX", "USD", InstrumentKind::Commodity),
    ]
}

fn instrument(
    id: &str,
    symbol: &str,
    name: &str,
    venue: &str,
    currency: &str,
    kind: InstrumentKind,
) -> Instrument {
    Instrument {
        id: InstrumentId::new(id),
        symbol: symbol.to_owned(),
        name: name.to_owned(),
        venue: venue.to_owned(),
        currency: currency.to_owned(),
        kind,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_symbols_rank_before_name_matches() {
        let results = DemoInstrumentSearch.search("SPY", 10);
        assert_eq!(results.first().map(|instrument| instrument.symbol.as_str()), Some("SPY"));
    }

    #[test]
    fn search_matches_company_names_case_insensitively() {
        let results = DemoInstrumentSearch.search("microsoft", 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id.as_str(), "us:xnas:msft");
    }

    #[test]
    fn result_limit_is_respected() {
        assert_eq!(DemoInstrumentSearch.search("", 3).len(), 3);
    }
}
