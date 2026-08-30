#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarketIndex {
    pub name: &'static str,
    pub symbol: &'static str,
    pub last: &'static str,
    pub net_change: &'static str,
    pub percent_change: &'static str,
}

#[derive(Debug, Clone)]
pub enum MarketsSnapshot {
    Gallery {
        indices: &'static [MarketIndex],
        treasury_curve: &'static [(f64, f64)],
    },
    Live(LiveMarketsSnapshot),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LiveMarketsSnapshot {
    pub rows: Vec<LiveMarketRow>,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveMarketRow {
    pub symbol: String,
    pub last: String,
    pub net_change: String,
    pub percent_change: String,
    pub quality: String,
    pub as_of: String,
    pub provider: String,
}
