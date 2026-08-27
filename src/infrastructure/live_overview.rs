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
            .into_iter()
            .take(12)
            .map(|position| OverviewHolding {
                symbol: position.symbol,
                quantity: position.quantity,
                market_value: position.market_value,
                pnl: position.pnl,
                weight: position.weight,
            })
            .collect();

        OverviewSnapshot::Live(LiveOverviewSnapshot {
            net_asset_value: portfolio.net_asset_value,
            ytd_return: portfolio.ytd_return,
            available_cash: portfolio.available_cash,
            sharpe: portfolio.sharpe,
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
            PortfolioSnapshot {
                positions: vec![Position {
                    symbol: "USER".to_owned(),
                    quantity: "4".to_owned(),
                    average_cost: "10.00".to_owned(),
                    market_value: "$48.00".to_owned(),
                    pnl: "+20.00%".to_owned(),
                    weight: "100.00%".to_owned(),
                }],
                net_asset_value: "$48.00".to_owned(),
                ytd_return: "N/A".to_owned(),
                available_cash: "$0.00".to_owned(),
                sharpe: "N/A".to_owned(),
                source: "CSV · positions.csv".to_owned(),
                as_of: "2026-08-26 12:00 UTC".to_owned(),
            }
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
