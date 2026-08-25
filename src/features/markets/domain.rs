#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarketIndex {
    pub name: &'static str,
    pub symbol: &'static str,
    pub last: &'static str,
    pub net_change: &'static str,
    pub percent_change: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct MarketsSnapshot {
    pub indices: &'static [MarketIndex],
    pub treasury_curve: &'static [(f64, f64)],
}
