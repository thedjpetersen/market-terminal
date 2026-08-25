use super::PortfolioSnapshot;

pub trait PortfolioQuery: Send + Sync {
    fn load_portfolio(&self) -> PortfolioSnapshot;
}
