#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    pub symbol: &'static str,
    pub quantity: &'static str,
    pub average_cost: &'static str,
    pub market_value: &'static str,
    pub pnl: &'static str,
    pub weight: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct PortfolioSnapshot {
    pub positions: &'static [Position],
    pub net_asset_value: &'static str,
    pub ytd_return: &'static str,
    pub available_cash: &'static str,
    pub sharpe: &'static str,
}
