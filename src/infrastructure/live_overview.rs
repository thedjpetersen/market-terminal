use std::sync::Arc;

use crate::features::{
    news::NewsFeed,
    overview::{
        LiveOverviewSnapshot, OverviewHeadline, OverviewHolding, OverviewQuery, OverviewSnapshot,
    },
    portfolio::PortfolioRepository,
};

/// Composes the Overview from already-loaded feature snapshots.
///
/// Both dependencies expose in-memory snapshots, so rendering never performs
/// filesystem or network I/O. Returns and risk statistics remain unavailable
/// until a real performance-history adapter exists; they are never inferred
/// from a point-in-time CSV import.
pub struct LiveOverviewQuery {
    portfolio: Arc<dyn PortfolioRepository>,
    news: Arc<dyn NewsFeed>,
}

impl LiveOverviewQuery {
    pub fn new(portfolio: Arc<dyn PortfolioRepository>, news: Arc<dyn NewsFeed>) -> Self {
        Self { portfolio, news }
    }
}

impl OverviewQuery for LiveOverviewQuery {
    fn load_overview(&self) -> OverviewSnapshot {
        let portfolio = self.portfolio.load_portfolio();
        let headlines = self
            .news
            .load_news()
            .headlines
            .into_iter()
            .take(12)
            .map(|headline| OverviewHeadline {
                time: headline.time,
                topic: headline.topic,
                title: headline.title,
                region: headline.region,
            })
            .collect();
        let holdings = portfolio
            .positions
            .iter()
            .take(12)
            .map(|position| OverviewHolding {
                symbol: position.symbol.clone(),
                quantity: position.quantity_label(),
                market_value: position.market_value_label(),
                pnl: position.pnl_label(),
                weight: position.weight_label(),
            })
            .collect();

        OverviewSnapshot::Live(LiveOverviewSnapshot {
            net_asset_value: portfolio.net_asset_value_label(),
            ytd_return: portfolio.ytd_return_label(),
            available_cash: portfolio.available_cash_label(),
            sharpe: portfolio.sharpe_label(),
            portfolio_source: portfolio.source,
            portfolio_as_of: portfolio.as_of,
            holdings,
            headlines,
            news_status: self.news.status(),
        })
    }

    fn request_refresh(&self) {
        self.news.request_refresh();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::{
        news::{Headline, NewsSnapshot},
        portfolio::{PortfolioSnapshot, Position},
    };

    struct PortfolioFixture;

    impl PortfolioRepository for PortfolioFixture {
        fn load_portfolio(&self) -> PortfolioSnapshot {
            let usd = crate::foundation::Currency::new("USD").unwrap();
            let mut snapshot = PortfolioSnapshot::empty("CSV · positions.csv");
            snapshot.positions = vec![Position {
                instrument_id: crate::foundation::InstrumentId::new("us:xnys:user"),
                account_id: crate::features::portfolio::PortfolioAccountId::new("ACCOUNT 1"),
                symbol: "USER".to_owned(),
                currency: usd,
                quantity: crate::features::portfolio::PositionQuantity::from_scaled_units(
                    4_000_000,
                ),
                average_cost: Some(crate::foundation::Money::from_minor_units(1_000, usd)),
                market_value: Some(crate::foundation::Money::from_minor_units(4_800, usd)),
                unrealized_return_bps: Some(2_000),
                weight_bps: Some(10_000),
                cash: false,
            }];
            snapshot.currency_totals = vec![crate::features::portfolio::PortfolioCurrencyTotal {
                currency: usd,
                net_asset_value: crate::foundation::Money::from_minor_units(4_800, usd),
                available_cash: crate::foundation::Money::from_minor_units(0, usd),
                priced_positions: 1,
                unpriced_positions: 0,
            }];
            snapshot.as_of = "2026-08-26 12:00 UTC".to_owned();
            snapshot
        }
    }

    struct NewsFixture;

    impl NewsFeed for NewsFixture {
        fn load_news(&self) -> NewsSnapshot {
            NewsSnapshot {
                headlines: vec![Headline {
                    time: "12:01".to_owned(),
                    topic: "TOP".to_owned(),
                    title: "A real cached headline".to_owned(),
                    region: "US".to_owned(),
                }],
            }
        }

        fn status(&self) -> String {
            "LIVE · 1 STORY".to_owned()
        }
    }

    #[test]
    fn composes_only_the_imported_portfolio_and_cached_news() {
        let query = LiveOverviewQuery::new(Arc::new(PortfolioFixture), Arc::new(NewsFixture));

        let OverviewSnapshot::Live(snapshot) = query.load_overview() else {
            panic!("live query must return a live overview")
        };

        assert_eq!(snapshot.holdings[0].symbol, "USER");
        assert_eq!(snapshot.headlines[0].title, "A real cached headline");
        assert_eq!(snapshot.ytd_return, "N/A");
        assert_eq!(snapshot.news_status, "LIVE · 1 STORY");
    }
}
