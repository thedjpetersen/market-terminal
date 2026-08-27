use std::{path::PathBuf, sync::Arc, time::Duration};

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
        AlpacaMarketData, AlphaVantageMarketData, CodexAppServerConfig, CodexAppServerGateway,
        ConfiguredWatchlistCatalog, CsvPortfolioRepository, DemoAlertsReplay, DemoChartHistory,
        DemoChatGateway, DemoData, DemoInstrumentSearch, DemoMarketDataReplay,
        DemoSpreadsheetMarketData, DemoWatchlistCatalog, IrcChatGateway, LiveAlertsQuery,
        LiveNewsFeed, LiveSecurityQuery, LocalPersistence, OpenRouterConfig, OpenRouterGateway,
        SecInstrumentSearch, SystemNewsArticleOpener,
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
        security_query: Arc::new(DemoData),
        security_symbol: "AAPL US".to_owned(),
        alerts_query: Arc::new(DemoAlertsReplay::new()),
        snapshot_refresh_interval: Duration::from_secs(60),
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
    security_query: Arc<dyn SecurityQuery>,
    security_symbol: String,
    alerts_query: Arc<dyn AlertsQuery>,
    snapshot_refresh_interval: Duration,
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
        security_query,
        security_symbol,
        alerts_query,
        snapshot_refresh_interval,
    } = providers;
    let data = Arc::new(DemoData);
    let overview_query: Arc<dyn OverviewQuery> = data.clone();
    let markets_query: Arc<dyn MarketsQuery> = data.clone();
    let assistant_provider = std::env::var("MARKET_TERMINAL_AI_PROVIDER")
        .unwrap_or_else(|_| "codex".to_owned())
        .to_ascii_lowercase();
    let assistant_gateway: Arc<dyn AssistantGateway> = match assistant_provider.as_str() {
        "codex" => Arc::new(CodexAppServerGateway::new(CodexAppServerConfig::from_env())),
        _ => Arc::new(OpenRouterGateway::new(OpenRouterConfig::from_env())),
    };

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
        Box::new(WatchlistWorkspace::with_snapshot_refresh_interval(
            market_data,
            watchlist_catalog,
            snapshot_refresh_interval,
        )),
        Box::new(MarketsWorkspace::new(markets_query)),
        Box::new(ChartingWorkspace::with_primary(
            chart_history,
            chart_primary,
        )),
        Box::new(ChatWorkspace::new(chat)),
        Box::new(AlertsWorkspace::new(alerts_query)),
        Box::new(SecurityWorkspace::with_symbol(
            security_query,
            security_symbol,
        )),
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
    let live_market_data = configured_market_data();
    let security_query: Arc<dyn SecurityQuery> = Arc::new(LiveSecurityQuery::from_env(
        live_market_data.market_data.clone(),
        live_market_data.chart_history.clone(),
    ));
    let alerts_query: Arc<dyn AlertsQuery> =
        Arc::new(LiveAlertsQuery::new(live_market_data.market_data.clone()));
    let watchlist_catalog: Arc<dyn WatchlistCatalog> =
        Arc::new(ConfiguredWatchlistCatalog::from_env());
    let initial_symbol = initial_chart_symbol();
    build_app(AppProviders {
        chat: Arc::new(IrcChatGateway::from_env()),
        portfolio_query,
        news_query,
        article_opener: Some(Arc::new(SystemNewsArticleOpener)),
        spreadsheet_market_data: live_market_data.spreadsheet,
        market_data: live_market_data.market_data,
        watchlist_catalog,
        instrument_search: Arc::new(SecInstrumentSearch::from_env()),
        chart_history: live_market_data.chart_history,
        chart_primary: ChartInstrument::from_terminal_subject(&initial_symbol),
        security_query,
        security_symbol: format!("{initial_symbol} US"),
        alerts_query,
        snapshot_refresh_interval: quote_refresh_interval(),
    })
    .with_session_repository(repository)
}

struct LiveMarketDataProviders {
    spreadsheet: Arc<dyn SpreadsheetMarketData>,
    market_data: Arc<dyn MarketDataQuery>,
    chart_history: Arc<dyn ChartHistoryQuery>,
}

fn configured_market_data() -> LiveMarketDataProviders {
    let provider = std::env::var("MARKET_TERMINAL_MARKET_DATA_PROVIDER")
        .unwrap_or_else(|_| "alpha-vantage".to_owned())
        .trim()
        .to_ascii_lowercase();
    if provider == "alpaca" {
        let adapter = Arc::new(AlpacaMarketData::from_env());
        LiveMarketDataProviders {
            spreadsheet: adapter.clone(),
            market_data: adapter.clone(),
            chart_history: adapter,
        }
    } else {
        let adapter = Arc::new(AlphaVantageMarketData::from_env());
        LiveMarketDataProviders {
            spreadsheet: adapter.clone(),
            market_data: adapter.clone(),
            chart_history: adapter,
        }
    }
}

fn quote_refresh_interval() -> Duration {
    let seconds = std::env::var("MARKET_TERMINAL_QUOTE_REFRESH_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(|seconds| seconds.clamp(5, 3_600))
        .unwrap_or(60);
    Duration::from_secs(seconds)
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
