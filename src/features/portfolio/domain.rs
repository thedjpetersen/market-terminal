#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Position {
    pub symbol: String,
    pub quantity: String,
    pub average_cost: String,
    pub market_value: String,
    pub pnl: String,
    pub weight: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortfolioSnapshot {
    pub positions: Vec<Position>,
    pub net_asset_value: String,
    pub ytd_return: String,
    pub available_cash: String,
    pub sharpe: String,
    pub source: String,
    pub as_of: String,
}

impl PortfolioSnapshot {
    pub fn empty(source: impl Into<String>) -> Self {
        Self {
            positions: Vec::new(),
            net_asset_value: "—".to_owned(),
            ytd_return: "N/A".to_owned(),
            available_cash: "—".to_owned(),
            sharpe: "N/A".to_owned(),
            source: source.into(),
            as_of: "—".to_owned(),
        }
    }
}
