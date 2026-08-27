#[derive(Debug, Clone)]
pub enum OverviewSnapshot {
    Gallery {
        periods: &'static [&'static str],
        primary_returns: &'static [(f64, f64)],
        comparison_returns: &'static [(f64, f64)],
    },
    Live(LiveOverviewSnapshot),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveOverviewSnapshot {
    pub net_asset_value: String,
    pub ytd_return: String,
    pub available_cash: String,
    pub sharpe: String,
    pub portfolio_source: String,
    pub portfolio_as_of: String,
    pub holdings: Vec<OverviewHolding>,
    pub headlines: Vec<OverviewHeadline>,
    pub news_status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverviewHolding {
    pub symbol: String,
    pub quantity: String,
    pub market_value: String,
    pub pnl: String,
    pub weight: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverviewHeadline {
    pub time: String,
    pub topic: String,
    pub title: String,
    pub region: String,
}
