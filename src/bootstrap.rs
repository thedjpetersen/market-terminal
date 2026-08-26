use std::{path::PathBuf, sync::Arc};

use crate::{
    app::{App, WorkspaceRegistry},
    features::{
        alerts::{AlertsQuery, AlertsWorkspace},
        assistant::{AssistantGateway, AssistantWorkspace},
        charting::{ChartHistoryQuery, ChartingWorkspace},
        chat::{ChatGateway, ChatWorkspace},
        instrument::{InstrumentSearch, InstrumentSearchWorkspace},
        market_data::MarketDataQuery,
        markets::{MarketsQuery, MarketsWorkspace},
        news::{NewsArticleOpener, NewsFeed, NewsWorkspace},
        overview::{ID as OVERVIEW, OverviewQuery, OverviewWorkspace},
        portfolio::{PortfolioRepository, PortfolioWorkspace},
        security::{SecurityQuery, SecurityWorkspace},
        spreadsheet::{SpreadsheetMarketData, SpreadsheetWorkspace},
        watchlist::{WatchlistCatalog, WatchlistWorkspace},
    },
    infrastructure::{
        CodexAppServerConfig, CodexAppServerGateway, CsvPortfolioRepository, DemoAlertsReplay,
        DemoChartHistory, DemoChatGateway, DemoData, DemoInstrumentSearch, DemoMarketDataReplay,
        DemoSpreadsheetMarketData, DemoWatchlistCatalog, IrcChatGateway, LiveNewsFeed,
        LocalPersistence, OpenRouterConfig, OpenRouterGateway, SystemNewsArticleOpener,
    },
};

pub fn demo_app() -> App {
    let data = Arc::new(DemoData);
    let portfolio_query: Arc<dyn PortfolioRepository> = data.clone();
    let news_query: Arc<dyn NewsFeed> = data;
    build_app(
        Arc::new(DemoChatGateway::new()),
        portfolio_query,
        news_query,
        None,
    )
}

fn build_app(
    chat_gateway: Arc<dyn ChatGateway>,
    portfolio_query: Arc<dyn PortfolioRepository>,
    news_query: Arc<dyn NewsFeed>,
    article_opener: Option<Arc<dyn NewsArticleOpener>>,
) -> App {
    let data = Arc::new(DemoData);
    let overview_query: Arc<dyn OverviewQuery> = data.clone();
    let markets_query: Arc<dyn MarketsQuery> = data.clone();
    let security_query: Arc<dyn SecurityQuery> = data.clone();
    let spreadsheet_market_data: Arc<dyn SpreadsheetMarketData> =
        Arc::new(DemoSpreadsheetMarketData);
    let assistant_provider = std::env::var("MARKET_TERMINAL_AI_PROVIDER")
        .unwrap_or_else(|_| "codex".to_owned())
        .to_ascii_lowercase();
    let assistant_gateway: Arc<dyn AssistantGateway> = match assistant_provider.as_str() {
        "codex" => Arc::new(CodexAppServerGateway::new(CodexAppServerConfig::from_env())),
        _ => Arc::new(OpenRouterGateway::new(OpenRouterConfig::from_env())),
    };
    let instrument_search: Arc<dyn InstrumentSearch> = Arc::new(DemoInstrumentSearch);
    let chart_history: Arc<dyn ChartHistoryQuery> = Arc::new(DemoChartHistory);
    let market_data: Arc<dyn MarketDataQuery> = Arc::new(DemoMarketDataReplay::new());
    let watchlist_catalog: Arc<dyn WatchlistCatalog> = Arc::new(DemoWatchlistCatalog);
    let alerts_query: Arc<dyn AlertsQuery> = Arc::new(DemoAlertsReplay::new());

    let workspaces = WorkspaceRegistry::new(vec![
        Box::new(OverviewWorkspace::new(overview_query)),
        Box::new(AssistantWorkspace::new(
            assistant_gateway,
            vec![
                "overview".to_owned(),
                "assistant".to_owned(),
                "instrument_search".to_owned(),
                "watchlist".to_owned(),
                "markets".to_owned(),
                "charting".to_owned(),
                "chat".to_owned(),
                "alerts".to_owned(),
                "security".to_owned(),
                "portfolio".to_owned(),
                "news".to_owned(),
                "spreadsheet".to_owned(),
            ],
        )),
        Box::new(InstrumentSearchWorkspace::new(instrument_search)),
        Box::new(WatchlistWorkspace::new(market_data, watchlist_catalog)),
        Box::new(MarketsWorkspace::new(markets_query)),
        Box::new(ChartingWorkspace::new(chart_history)),
        Box::new(ChatWorkspace::new(chat_gateway)),
        Box::new(AlertsWorkspace::new(alerts_query)),
        Box::new(SecurityWorkspace::new(security_query)),
        Box::new(PortfolioWorkspace::new(portfolio_query)),
        Box::new(match article_opener {
            Some(opener) => NewsWorkspace::with_article_opener(news_query, opener),
            None => NewsWorkspace::new(news_query),
        }),
        Box::new(SpreadsheetWorkspace::new(spreadsheet_market_data)),
    ]);
    App::new(workspaces, OVERVIEW)
}

/// Builds the interactive application with durable shell state enabled.
pub fn persistent_app() -> App {
    let repository = Arc::new(LocalPersistence::new(default_state_directory()));
    let portfolio_query: Arc<dyn PortfolioRepository> =
        Arc::new(CsvPortfolioRepository::from_env());
    let news_query: Arc<dyn NewsFeed> = Arc::new(LiveNewsFeed::from_env());
    build_app(
        Arc::new(IrcChatGateway::from_env()),
        portfolio_query,
        news_query,
        Some(Arc::new(SystemNewsArticleOpener)),
    )
    .with_session_repository(repository)
}

pub fn default_state_directory() -> PathBuf {
    if let Some(path) = std::env::var_os("MARKET_TERMINAL_STATE_DIR") {
        return PathBuf::from(path);
    }
    if let Some(path) = std::env::var_os("XDG_STATE_HOME") {
        return PathBuf::from(path).join("market-terminal");
    }
    if let Some(path) = std::env::var_os("HOME") {
        return PathBuf::from(path).join(".local/state/market-terminal");
    }
    PathBuf::from(".market-terminal")
}
