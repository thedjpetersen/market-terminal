use std::{path::PathBuf, sync::Arc};

use crate::{
    app::{App, WorkspaceRegistry},
    features::{
        alerts::{AlertsQuery, AlertsWorkspace},
        assistant::{AssistantGateway, AssistantWorkspace},
        charting::{ChartHistoryQuery, ChartInstrument, ChartingWorkspace},
        chat::{ChatGateway, ChatWorkspace},
        instrument::{InstrumentSearch, InstrumentSearchWorkspace},
        market_data::MarketDataQuery,
        markets::{MarketsQuery, MarketsWorkspace},
        news::{NewsArticleOpener, NewsFeed, NewsWorkspace},
        overview::{OverviewQuery, OverviewWorkspace, ID as OVERVIEW},
        portfolio::{PortfolioRepository, PortfolioWorkspace},
        security::{SecurityQuery, SecurityWorkspace},
        spreadsheet::{SpreadsheetMarketData, SpreadsheetWorkspace},
        watchlist::{WatchlistCatalog, WatchlistWorkspace},
    },
    infrastructure::{
        AlphaVantageMarketData, CodexAppServerConfig, CodexAppServerGateway,
        ConfiguredWatchlistCatalog, CsvPortfolioRepository, DemoAlertsReplay, DemoChartHistory,
        DemoChatGateway, DemoData, DemoInstrumentSearch, DemoMarketDataReplay,
        DemoSpreadsheetMarketData, DemoWatchlistCatalog, IrcChatGateway, LiveNewsFeed,
        LocalPersistence, OpenRouterConfig, OpenRouterGateway, SecInstrumentSearch,
        SystemNewsArticleOpener,
    },
};

pub fn demo_app() -> App {
    let data = Arc::new(DemoData);
    let portfolio_query: Arc<dyn PortfolioRepository> = data.clone();
    let news_query: Arc<dyn NewsFeed> = data;
    build_app(AppProviders {
        chat: Arc::new(DemoChatGateway::new()),
        portfolio_query,
        news_query,
        article_opener: None,
        spreadsheet_market_data: Arc::new(DemoSpreadsheetMarketData),
        market_data: Arc::new(DemoMarketDataReplay::new()),
        watchlist_catalog: Arc::new(DemoWatchlistCatalog),
        instrument_search: Arc::new(DemoInstrumentSearch),
        chart_history: Arc::new(DemoChartHistory),
        chart_primary: ChartInstrument::from_terminal_subject("AAPL"),
    })
}

struct AppProviders {
    chat: Arc<dyn ChatGateway>,
    portfolio_query: Arc<dyn PortfolioRepository>,
    news_query: Arc<dyn NewsFeed>,
    article_opener: Option<Arc<dyn NewsArticleOpener>>,
    spreadsheet_market_data: Arc<dyn SpreadsheetMarketData>,
    market_data: Arc<dyn MarketDataQuery>,
    watchlist_catalog: Arc<dyn WatchlistCatalog>,
    instrument_search: Arc<dyn InstrumentSearch>,
    chart_history: Arc<dyn ChartHistoryQuery>,
    chart_primary: ChartInstrument,
}

fn build_app(providers: AppProviders) -> App {
    let AppProviders {
        chat,
        portfolio_query,
        news_query,
        article_opener,
        spreadsheet_market_data,
        market_data,
        watchlist_catalog,
        instrument_search,
        chart_history,
        chart_primary,
    } = providers;
    let data = Arc::new(DemoData);
    let overview_query: Arc<dyn OverviewQuery> = data.clone();
    let markets_query: Arc<dyn MarketsQuery> = data.clone();
    let security_query: Arc<dyn SecurityQuery> = data.clone();
    let assistant_provider = std::env::var("MARKET_TERMINAL_AI_PROVIDER")
        .unwrap_or_else(|_| "codex".to_owned())
        .to_ascii_lowercase();
    let assistant_gateway: Arc<dyn AssistantGateway> = match assistant_provider.as_str() {
        "codex" => Arc::new(CodexAppServerGateway::new(CodexAppServerConfig::from_env())),
        _ => Arc::new(OpenRouterGateway::new(OpenRouterConfig::from_env())),
    };
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
        Box::new(ChartingWorkspace::with_primary(
            chart_history,
            chart_primary,
        )),
        Box::new(ChatWorkspace::new(chat)),
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
    let alpha_vantage = Arc::new(AlphaVantageMarketData::from_env());
    let spreadsheet_market_data: Arc<dyn SpreadsheetMarketData> = alpha_vantage.clone();
    let market_data: Arc<dyn MarketDataQuery> = alpha_vantage.clone();
    let chart_history: Arc<dyn ChartHistoryQuery> = alpha_vantage;
    let watchlist_catalog: Arc<dyn WatchlistCatalog> =
        Arc::new(ConfiguredWatchlistCatalog::from_env());
    build_app(AppProviders {
        chat: Arc::new(IrcChatGateway::from_env()),
        portfolio_query,
        news_query,
        article_opener: Some(Arc::new(SystemNewsArticleOpener)),
        spreadsheet_market_data,
        market_data,
        watchlist_catalog,
        instrument_search: Arc::new(SecInstrumentSearch::from_env()),
        chart_history,
        chart_primary: ChartInstrument::from_terminal_subject(&initial_chart_symbol()),
    })
    .with_session_repository(repository)
}

fn initial_chart_symbol() -> String {
    std::env::var("MARKET_TERMINAL_CHART_SYMBOL")
        .ok()
        .map(|symbol| symbol.trim().to_ascii_uppercase())
        .filter(|symbol| {
            !symbol.is_empty()
                && symbol.len() <= 32
                && symbol.chars().all(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '.' | '-')
                })
        })
        .unwrap_or_else(|| "IBM".to_owned())
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
