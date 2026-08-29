use std::sync::Arc;

use crate::features::{
    alerts::{AlertStateStore, AlertStatus},
    launchpad::LaunchpadStateStore,
    markets::{MarketsQuery, MarketsSnapshot},
    news::NewsFeed,
    overview::{
        LiveOverviewSnapshot, OverviewEvent, OverviewHeadline, OverviewHealthState,
        OverviewHolding, OverviewMarketPulse, OverviewPriority, OverviewQuery, OverviewSavedWork,
        OverviewSnapshot, OverviewSourceHealth,
    },
    portfolio::{PortfolioRepository, PortfolioSnapshot},
};

#[derive(Debug, Clone)]
struct LocalMissionSnapshot {
    saved_work: Vec<OverviewSavedWork>,
    launchpad_health: OverviewSourceHealth,
    alert_health: OverviewSourceHealth,
    triggered_alerts: usize,
}

/// Composes Mission Control from feature-owned snapshots translated into the
/// Overview context's read model.
///
/// Every render-time dependency is an in-memory snapshot. Durable Launchpad and
/// alert documents are sampled once during composition-root startup, so neither
/// rendering nor input handling performs filesystem or network I/O.
pub struct LiveOverviewQuery {
    portfolio: Arc<dyn PortfolioRepository>,
    news: Arc<dyn NewsFeed>,
    markets: Option<Arc<dyn MarketsQuery>>,
    local: LocalMissionSnapshot,
}

impl LiveOverviewQuery {
    #[cfg(test)]
    pub fn new(portfolio: Arc<dyn PortfolioRepository>, news: Arc<dyn NewsFeed>) -> Self {
        Self {
            portfolio,
            news,
            markets: None,
            local: local_mission_snapshot(None, None),
        }
    }

    pub fn mission_control(
        portfolio: Arc<dyn PortfolioRepository>,
        news: Arc<dyn NewsFeed>,
        markets: Arc<dyn MarketsQuery>,
        launchpad: Arc<dyn LaunchpadStateStore>,
        alerts: Arc<dyn AlertStateStore>,
    ) -> Self {
        Self {
            portfolio,
            news,
            markets: Some(markets),
            local: local_mission_snapshot(Some(&*launchpad), Some(&*alerts)),
        }
    }
}

impl OverviewQuery for LiveOverviewQuery {
    fn load_overview(&self) -> OverviewSnapshot {
        let portfolio = self.portfolio.load_portfolio();
        let news_status = self.news.status();
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
        let events = self
            .news
            .load_events()
            .into_iter()
            .take(8)
            .map(|event| OverviewEvent {
                time: event.time,
                region: event.region,
                importance: event.importance.label().to_owned(),
                title: event.event,
                period: event.period,
            })
            .collect::<Vec<_>>();
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
        let (market_pulse, market_status) = market_snapshot(self.markets.as_deref());
        let source_health = source_health(
            &portfolio,
            &market_pulse,
            &market_status,
            &news_status,
            &self.local,
        );
        let priorities = priorities(
            &portfolio,
            &market_status,
            &news_status,
            events.len(),
            &self.local,
        );

        OverviewSnapshot::Live(Box::new(LiveOverviewSnapshot {
            net_asset_value: portfolio.net_asset_value_label(),
            ytd_return: portfolio.ytd_return_label(),
            available_cash: portfolio.available_cash_label(),
            sharpe: portfolio.sharpe_label(),
            portfolio_source: portfolio.source,
            portfolio_as_of: portfolio.as_of,
            holdings,
            headlines,
            news_status,
            market_pulse,
            events,
            source_health,
            saved_work: self.local.saved_work.clone(),
            priorities,
        }))
    }

    fn request_refresh(&self) {
        self.news.request_refresh();
        if let Some(markets) = &self.markets {
            markets.request_refresh();
        }
    }
}

fn local_mission_snapshot(
    launchpad: Option<&dyn LaunchpadStateStore>,
    alerts: Option<&dyn AlertStateStore>,
) -> LocalMissionSnapshot {
    let (saved_work, launchpad_health) = match launchpad {
        Some(store) => match store.load_launchpad() {
            Ok(Some(state)) => {
                let revision = state.revision;
                let work = state
                    .tiles
                    .into_iter()
                    .take(8)
                    .map(|tile| {
                        let command = tile.command();
                        let kind = format!("{} TILE", tile.target.kind());
                        OverviewSavedWork {
                            id: tile.id,
                            label: tile.label,
                            command,
                            kind,
                        }
                    })
                    .collect::<Vec<_>>();
                let detail = format!("{} SAVED TILE(S) · REV {revision}", work.len());
                (
                    work,
                    health(
                        "LAUNCHPAD",
                        OverviewHealthState::Ready,
                        detail,
                        "—",
                        "LAUNCH",
                    ),
                )
            }
            Ok(None) => (
                Vec::new(),
                health(
                    "LAUNCHPAD",
                    OverviewHealthState::NotConfigured,
                    "VERSIONED SEEDS WILL BE CREATED ON FIRST OPEN",
                    "—",
                    "LAUNCH",
                ),
            ),
            Err(error) => (
                Vec::new(),
                health(
                    "LAUNCHPAD",
                    OverviewHealthState::Unavailable,
                    error.to_string(),
                    "—",
                    "LAUNCH",
                ),
            ),
        },
        None => (
            Vec::new(),
            health(
                "LAUNCHPAD",
                OverviewHealthState::NotConfigured,
                "LOCAL SAVED-WORK SOURCE NOT ATTACHED",
                "—",
                "LAUNCH",
            ),
        ),
    };

    let (alert_health, triggered_alerts) = match alerts {
        Some(store) => match store.load_alert_rules() {
            Ok(Some(state)) => {
                let triggered = state
                    .rules
                    .iter()
                    .filter(|rule| matches!(rule.status, AlertStatus::Triggered { .. }))
                    .count();
                let state_label = if triggered > 0 {
                    OverviewHealthState::Partial
                } else {
                    OverviewHealthState::Ready
                };
                (
                    health(
                        "ALERTS",
                        state_label,
                        format!("{} RULE(S) · {triggered} TRIGGERED", state.rules.len()),
                        "—",
                        "ALERTS",
                    ),
                    triggered,
                )
            }
            Ok(None) => (
                health(
                    "ALERTS",
                    OverviewHealthState::NotConfigured,
                    "NO DURABLE RULES",
                    "—",
                    "ALERTS",
                ),
                0,
            ),
            Err(error) => (
                health(
                    "ALERTS",
                    OverviewHealthState::Unavailable,
                    error.to_string(),
                    "—",
                    "ALERTS",
                ),
                0,
            ),
        },
        None => (
            health(
                "ALERTS",
                OverviewHealthState::NotConfigured,
                "LOCAL ALERT SOURCE NOT ATTACHED",
                "—",
                "ALERTS",
            ),
            0,
        ),
    };

    LocalMissionSnapshot {
        saved_work,
        launchpad_health,
        alert_health,
        triggered_alerts,
    }
}

fn market_snapshot(markets: Option<&dyn MarketsQuery>) -> (Vec<OverviewMarketPulse>, String) {
    match markets.map(MarketsQuery::load_markets) {
        Some(MarketsSnapshot::Live(snapshot)) => (
            snapshot
                .rows
                .into_iter()
                .take(8)
                .map(|row| OverviewMarketPulse {
                    symbol: row.symbol,
                    last: row.last,
                    percent_change: row.percent_change,
                    quality: row.quality,
                    as_of: row.as_of,
                    provider: row.provider,
                })
                .collect(),
            snapshot.status,
        ),
        Some(MarketsSnapshot::Gallery { .. }) => (
            Vec::new(),
            "LIVE MARKET PULSE UNAVAILABLE · GALLERY SOURCE REJECTED".to_owned(),
        ),
        None => (Vec::new(), "LIVE MARKET PULSE NOT CONFIGURED".to_owned()),
    }
}

fn source_health(
    portfolio: &PortfolioSnapshot,
    market_pulse: &[OverviewMarketPulse],
    market_status: &str,
    news_status: &str,
    local: &LocalMissionSnapshot,
) -> Vec<OverviewSourceHealth> {
    let unpriced = unpriced_positions(portfolio);
    let portfolio_state = if portfolio.positions.is_empty() {
        OverviewHealthState::NotConfigured
    } else if unpriced > 0 {
        OverviewHealthState::Partial
    } else {
        OverviewHealthState::Ready
    };
    let market_state = status_state(market_status, !market_pulse.is_empty());
    let news_state = status_state(news_status, !news_status.contains("NO STORIES"));
    let market_as_of = market_pulse
        .iter()
        .map(|row| row.as_of.as_str())
        .max()
        .unwrap_or("—");
    vec![
        health(
            "PORTFOLIO",
            portfolio_state,
            if portfolio.positions.is_empty() {
                "NO POSITION SNAPSHOT".to_owned()
            } else {
                format!(
                    "{} POSITION(S) · {unpriced} UNPRICED",
                    portfolio.positions.len()
                )
            },
            &portfolio.as_of,
            "PORT",
        ),
        health(
            "MARKETS",
            market_state,
            market_status,
            market_as_of,
            "MARKETS",
        ),
        health("NEWS", news_state, news_status, "CURRENT CACHE", "NEWS"),
        local.alert_health.clone(),
        local.launchpad_health.clone(),
    ]
}

fn priorities(
    portfolio: &PortfolioSnapshot,
    market_status: &str,
    news_status: &str,
    event_count: usize,
    local: &LocalMissionSnapshot,
) -> Vec<OverviewPriority> {
    let mut items = Vec::new();
    if local.triggered_alerts > 0 {
        items.push(priority(
            "triggered-alerts",
            100,
            format!("{} triggered alert(s)", local.triggered_alerts),
            "Triggered rules require acknowledgement or review",
            "ALERTS",
            "—",
            "ALERTS",
        ));
    }
    let unpriced = unpriced_positions(portfolio);
    if unpriced > 0 {
        items.push(priority(
            "unpriced-positions",
            90,
            format!("{unpriced} unpriced portfolio position(s)"),
            "Portfolio totals and weights are explicitly partial",
            "PORTFOLIO",
            &portfolio.as_of,
            "PORT",
        ));
    }
    if portfolio.positions.is_empty() {
        items.push(priority(
            "portfolio-missing",
            65,
            "Portfolio snapshot not configured",
            "Import positions to make exposure and priority ranking personal",
            "PORTFOLIO",
            "—",
            "PORT",
        ));
    } else if portfolio.ytd_return_bps.is_none() {
        items.push(priority(
            "performance-missing",
            45,
            "Performance history is unavailable",
            "Point-in-time positions cannot establish returns or risk",
            "PORTFOLIO",
            &portfolio.as_of,
            "PORT PERF",
        ));
    }
    let market_state = status_state(market_status, false);
    if matches!(
        market_state,
        OverviewHealthState::Unavailable | OverviewHealthState::Loading
    ) {
        items.push(priority(
            "market-pulse-unavailable",
            if market_state == OverviewHealthState::Unavailable {
                80
            } else {
                40
            },
            "Market pulse is unavailable",
            market_status,
            "MARKETS",
            "—",
            "MARKETS",
        ));
    }
    let news_state = status_state(news_status, false);
    if matches!(
        news_state,
        OverviewHealthState::Unavailable | OverviewHealthState::Loading
    ) {
        items.push(priority(
            "news-unavailable",
            if news_state == OverviewHealthState::Unavailable {
                70
            } else {
                35
            },
            "News feed is unavailable",
            news_status,
            "NEWS",
            "CURRENT CACHE",
            "NEWS",
        ));
    }
    if event_count == 0 {
        items.push(priority(
            "calendar-unavailable",
            25,
            "Upcoming events are unavailable",
            "No provider-backed calendar events were returned",
            "NEWS",
            "—",
            "NEWS CAL",
        ));
    }
    if local.saved_work.is_empty() {
        items.push(priority(
            "saved-work-empty",
            15,
            "No saved work is available",
            "Create Launchpad tiles for repeatable daily workflows",
            "LAUNCHPAD",
            "—",
            "LAUNCH",
        ));
    }
    items.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.id.cmp(&right.id))
    });
    items.truncate(8);
    items
}

fn status_state(status: &str, has_rows: bool) -> OverviewHealthState {
    let upper = status.to_ascii_uppercase();
    if upper.contains("UNAVAILABLE") || upper.contains("ERROR") || upper.contains("MISSING") {
        OverviewHealthState::Unavailable
    } else if upper.contains("LOADING") {
        OverviewHealthState::Loading
    } else if upper.contains("PARTIAL") {
        OverviewHealthState::Partial
    } else if has_rows {
        OverviewHealthState::Ready
    } else if upper.contains("NOT CONFIGURED") || upper.contains("NO STORIES") {
        OverviewHealthState::NotConfigured
    } else {
        OverviewHealthState::Ready
    }
}

fn unpriced_positions(portfolio: &PortfolioSnapshot) -> usize {
    portfolio
        .currency_totals
        .iter()
        .map(|total| total.unpriced_positions)
        .sum()
}

fn health(
    source: impl Into<String>,
    state: OverviewHealthState,
    detail: impl Into<String>,
    as_of: impl Into<String>,
    command: impl Into<String>,
) -> OverviewSourceHealth {
    OverviewSourceHealth {
        source: source.into(),
        state,
        detail: detail.into(),
        as_of: as_of.into(),
        command: command.into(),
    }
}

fn priority(
    id: impl Into<String>,
    score: u16,
    title: impl Into<String>,
    reason: impl Into<String>,
    source: impl Into<String>,
    as_of: impl Into<String>,
    command: impl Into<String>,
) -> OverviewPriority {
    OverviewPriority {
        id: id.into(),
        score,
        title: title.into(),
        reason: reason.into(),
        source: source.into(),
        as_of: as_of.into(),
        command: command.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::{
        alerts::{AlertRulesState, AlertStateError},
        launchpad::{LaunchpadState, LaunchpadStateError},
        markets::LiveMarketsSnapshot,
        news::{Headline, NewsSnapshot},
        portfolio::{PortfolioCurrencyTotal, Position},
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
            snapshot.currency_totals = vec![PortfolioCurrencyTotal {
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

    struct EmptyPortfolio;

    impl PortfolioRepository for EmptyPortfolio {
        fn load_portfolio(&self) -> PortfolioSnapshot {
            PortfolioSnapshot::empty("POSITIONS NOT CONFIGURED")
        }
    }

    struct OfflineNews;

    impl NewsFeed for OfflineNews {
        fn load_news(&self) -> NewsSnapshot {
            NewsSnapshot::default()
        }

        fn status(&self) -> String {
            "LIVE FEED UNAVAILABLE · OFFLINE".to_owned()
        }
    }

    struct OfflineMarkets;

    impl MarketsQuery for OfflineMarkets {
        fn load_markets(&self) -> MarketsSnapshot {
            MarketsSnapshot::Live(LiveMarketsSnapshot {
                rows: Vec::new(),
                status: "MARKET SNAPSHOT UNAVAILABLE · OFFLINE".to_owned(),
            })
        }
    }

    struct SavedLaunchpad;

    impl LaunchpadStateStore for SavedLaunchpad {
        fn load_launchpad(&self) -> Result<Option<LaunchpadState>, LaunchpadStateError> {
            Ok(Some(LaunchpadState::seeded()))
        }

        fn save_launchpad(&self, _state: &LaunchpadState) -> Result<(), LaunchpadStateError> {
            Ok(())
        }
    }

    struct EmptyAlerts;

    impl AlertStateStore for EmptyAlerts {
        fn load_alert_rules(&self) -> Result<Option<AlertRulesState>, AlertStateError> {
            Ok(None)
        }

        fn save_alert_rules(&self, _state: &AlertRulesState) -> Result<(), AlertStateError> {
            Ok(())
        }
    }

    #[test]
    fn composes_only_imported_cached_and_explicitly_unavailable_sources() {
        let query = LiveOverviewQuery::new(Arc::new(PortfolioFixture), Arc::new(NewsFixture));

        let OverviewSnapshot::Live(snapshot) = query.load_overview() else {
            panic!("live query must return a live overview")
        };

        assert_eq!(snapshot.holdings[0].symbol, "USER");
        assert_eq!(snapshot.headlines[0].title, "A real cached headline");
        assert_eq!(snapshot.ytd_return, "N/A");
        assert!(snapshot.market_pulse.is_empty());
        assert!(snapshot.events.is_empty());
        assert!(snapshot
            .priorities
            .iter()
            .any(|item| item.id == "calendar-unavailable"));
        assert!(snapshot.source_health.iter().any(|health| {
            health.source == "LAUNCHPAD" && health.state == OverviewHealthState::NotConfigured
        }));
    }

    #[test]
    fn ranking_is_descending_and_inspectable() {
        let portfolio = PortfolioSnapshot::empty("NOT CONFIGURED");
        let local = local_mission_snapshot(None, None);
        let ranked = priorities(
            &portfolio,
            "MARKET SNAPSHOT UNAVAILABLE",
            "LIVE FEED UNAVAILABLE",
            0,
            &local,
        );

        assert!(ranked.windows(2).all(|pair| pair[0].score >= pair[1].score));
        assert_eq!(ranked[0].id, "market-pulse-unavailable");
        assert!(ranked.iter().all(|item| {
            !item.reason.is_empty() && !item.source.is_empty() && !item.command.is_empty()
        }));
    }

    #[test]
    fn mission_control_remains_useful_with_every_external_provider_offline() {
        let query = LiveOverviewQuery::mission_control(
            Arc::new(EmptyPortfolio),
            Arc::new(OfflineNews),
            Arc::new(OfflineMarkets),
            Arc::new(SavedLaunchpad),
            Arc::new(EmptyAlerts),
        );

        let OverviewSnapshot::Live(snapshot) = query.load_overview() else {
            panic!("mission control must return a live snapshot")
        };
        assert!(snapshot.market_pulse.is_empty());
        assert!(snapshot.events.is_empty());
        assert!(snapshot.headlines.is_empty());
        assert_eq!(snapshot.saved_work.len(), 8);
        assert_eq!(snapshot.source_health.len(), 5);
        assert!(snapshot.source_health.iter().any(|health| {
            health.source == "MARKETS" && health.state == OverviewHealthState::Unavailable
        }));
        assert!(snapshot.source_health.iter().any(|health| {
            health.source == "NEWS" && health.state == OverviewHealthState::Unavailable
        }));
        assert!(snapshot
            .priorities
            .iter()
            .any(|item| item.id == "portfolio-missing"));
        assert!(snapshot
            .priorities
            .iter()
            .any(|item| item.id == "market-pulse-unavailable"));
        assert!(snapshot
            .priorities
            .iter()
            .all(|item| !item.reason.is_empty() && !item.command.is_empty()));
    }
}
