use std::{path::PathBuf, sync::Arc, time::Duration};

use crate::{
    app::{App, DeskWorkspace, Keymap, RuntimeSettingsSummary, WorkspaceRegistry},
    features::{
        alerts::{AlertStateStore, AlertsQuery, AlertsWorkspace},
        assistant::{AssistantGateway, AssistantWorkspace},
        charting::{ChartHistoryQuery, ChartInstrument, ChartingWorkspace},
        chat::{ChatGateway, ChatWorkspace},
        instrument::{InstrumentSearch, InstrumentSearchWorkspace},
        market_data::MarketDataQuery,
        markets::{MarketsQuery, MarketsWorkspace},
        news::{NewsArticleOpener, NewsFeed, NewsWorkspace},
        overview::{OverviewQuery, OverviewWorkspace, ID as OVERVIEW},
        portfolio::{PortfolioRepository, PortfolioWorkspace},
        risk::{RiskQuery, RiskWorkspace},
        security::{SecurityDocumentOpener, SecurityQuery, SecurityWorkspace},
        spreadsheet::{SpreadsheetMarketData, SpreadsheetWorkbookStore, SpreadsheetWorkspace},
        watchlist::{WatchlistCatalog, WatchlistWorkspace},
    },
    infrastructure::{
        AiCommandInference, AlpacaMarketData, AlphaVantageMarketData, CodexAppServerConfig,
        CodexAppServerGateway, ConfiguredWatchlistCatalog, CsvPortfolioRepository,
        DemoAlertsReplay, DemoChartHistory, DemoChatGateway, DemoData, DemoInstrumentSearch,
        DemoMarketDataReplay, DemoSpreadsheetMarketData, DemoWatchlistCatalog, FinnhubMarketData,
        IrcChatGateway, LiveAlertsQuery, LiveMarketsQuery, LiveNewsFeed, LiveOverviewQuery,
        LiveSecurityQuery, LiveSpreadsheetMarketData, LocalPersistence, LocalSpreadsheetFiles,
        OpenRouterConfig, OpenRouterGateway, PortfolioRiskQuery, SecInstrumentSearch,
        SystemNewsArticleOpener, YahooMarketData,
    },
};

pub fn demo_app() -> App {
    crate::ui::theme::set_theme("default").expect("built-in default theme");
    let data = Arc::new(DemoData);
    let portfolio_query: Arc<dyn PortfolioRepository> = data.clone();
    let risk_query: Arc<dyn RiskQuery> = Arc::new(PortfolioRiskQuery::new(portfolio_query.clone()));
    let news_query: Arc<dyn NewsFeed> = data;
    build_app(AppProviders {
        overview_query: Arc::new(DemoData),
        markets_query: Arc::new(DemoData),
        chat: Arc::new(DemoChatGateway::new()),
        portfolio_query,
        risk_query,
        news_query,
        article_opener: None,
        spreadsheet_market_data: Arc::new(DemoSpreadsheetMarketData),
        spreadsheet_workbook_store: None,
        market_data: Arc::new(DemoMarketDataReplay::new()),
        watchlist_catalog: Arc::new(DemoWatchlistCatalog),
        instrument_search: Arc::new(DemoInstrumentSearch),
        chart_history: Arc::new(DemoChartHistory),
        chart_primary: ChartInstrument::from_terminal_subject("AAPL"),
        security_query: Arc::new(DemoData),
        security_symbol: "AAPL US".to_owned(),
        security_document_opener: None,
        alerts_query: Arc::new(DemoAlertsReplay::new()),
        alert_state_store: None,
        snapshot_refresh_interval: Duration::from_secs(60),
        runtime_settings: RuntimeSettingsSummary::demo(),
    })
}

struct AppProviders {
    overview_query: Arc<dyn OverviewQuery>,
    markets_query: Arc<dyn MarketsQuery>,
    chat: Arc<dyn ChatGateway>,
    portfolio_query: Arc<dyn PortfolioRepository>,
    risk_query: Arc<dyn RiskQuery>,
    news_query: Arc<dyn NewsFeed>,
    article_opener: Option<Arc<dyn NewsArticleOpener>>,
    spreadsheet_market_data: Arc<dyn SpreadsheetMarketData>,
    spreadsheet_workbook_store: Option<Arc<dyn SpreadsheetWorkbookStore>>,
    market_data: Arc<dyn MarketDataQuery>,
    watchlist_catalog: Arc<dyn WatchlistCatalog>,
    instrument_search: Arc<dyn InstrumentSearch>,
    chart_history: Arc<dyn ChartHistoryQuery>,
    chart_primary: ChartInstrument,
    security_query: Arc<dyn SecurityQuery>,
    security_symbol: String,
    security_document_opener: Option<Arc<dyn SecurityDocumentOpener>>,
    alerts_query: Arc<dyn AlertsQuery>,
    alert_state_store: Option<Arc<dyn AlertStateStore>>,
    snapshot_refresh_interval: Duration,
    runtime_settings: RuntimeSettingsSummary,
}

fn build_app(providers: AppProviders) -> App {
    let AppProviders {
        overview_query,
        markets_query,
        chat,
        portfolio_query,
        risk_query,
        news_query,
        article_opener,
        spreadsheet_market_data,
        spreadsheet_workbook_store,
        market_data,
        watchlist_catalog,
        instrument_search,
        chart_history,
        chart_primary,
        security_query,
        security_symbol,
        security_document_opener,
        alerts_query,
        alert_state_store,
        snapshot_refresh_interval,
        runtime_settings,
    } = providers;
    let assistant_provider = std::env::var("MARKET_TERMINAL_AI_PROVIDER")
        .unwrap_or_else(|_| "codex".to_owned())
        .to_ascii_lowercase();
    let assistant_gateway: Arc<dyn AssistantGateway> = match assistant_provider.as_str() {
        "codex" => Arc::new(CodexAppServerGateway::new(CodexAppServerConfig::from_env())),
        _ => Arc::new(OpenRouterGateway::new(OpenRouterConfig::from_env())),
    };
    let command_inference = Arc::new(AiCommandInference::new(assistant_gateway.clone()));
    let spreadsheet_workspace = if runtime_settings.gallery_replay {
        SpreadsheetWorkspace::new(spreadsheet_market_data)
    } else if let Some(store) = spreadsheet_workbook_store {
        SpreadsheetWorkspace::persistent(
            spreadsheet_market_data,
            Arc::new(LocalSpreadsheetFiles),
            store,
        )
    } else {
        SpreadsheetWorkspace::empty(spreadsheet_market_data, Arc::new(LocalSpreadsheetFiles))
    };
    let desk_news = match article_opener.clone() {
        Some(opener) => NewsWorkspace::with_article_opener(news_query.clone(), opener),
        None => NewsWorkspace::new(news_query.clone()),
    };
    let desk_workspace = DeskWorkspace::new(
        Box::new(WatchlistWorkspace::with_snapshot_refresh_interval(
            market_data.clone(),
            watchlist_catalog.clone(),
            snapshot_refresh_interval,
        )),
        Box::new(ChartingWorkspace::with_primary(
            chart_history.clone(),
            chart_primary.clone(),
        )),
        Box::new(desk_news),
    );

    let workspaces = WorkspaceRegistry::new(vec![
        Box::new(OverviewWorkspace::new(overview_query)),
        Box::new(desk_workspace),
        Box::new(AssistantWorkspace::new(
            assistant_gateway,
            portfolio_query.clone(),
            vec![
                "overview".to_owned(),
                "desk".to_owned(),
                "assistant".to_owned(),
                "instrument_search".to_owned(),
                "watchlist".to_owned(),
                "markets".to_owned(),
                "charting".to_owned(),
                "chat".to_owned(),
                "alerts".to_owned(),
                "security".to_owned(),
                "portfolio".to_owned(),
                "risk".to_owned(),
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
        Box::new(match alert_state_store {
            Some(store) => AlertsWorkspace::persistent(alerts_query, store),
            None => AlertsWorkspace::new(alerts_query),
        }),
        Box::new(match security_document_opener {
            Some(opener) => SecurityWorkspace::with_symbol_and_document_opener(
                security_query,
                security_symbol,
                opener,
            ),
            None => SecurityWorkspace::with_symbol(security_query, security_symbol),
        }),
        Box::new(PortfolioWorkspace::new(portfolio_query)),
        Box::new(RiskWorkspace::new(risk_query)),
        Box::new(match article_opener {
            Some(opener) => NewsWorkspace::with_article_opener(news_query, opener),
            None => NewsWorkspace::new(news_query),
        }),
        Box::new(spreadsheet_workspace),
    ]);
    App::new(workspaces, OVERVIEW)
        .with_command_inference(command_inference)
        .with_runtime_settings(runtime_settings)
}

/// Builds the interactive application with durable shell state enabled.
pub fn persistent_app() -> App {
    let configured_theme =
        std::env::var("MARKET_TERMINAL_THEME").unwrap_or_else(|_| "default".to_owned());
    if crate::ui::theme::set_theme(&configured_theme).is_none() {
        crate::ui::theme::set_theme("default").expect("built-in default theme");
    }
    let repository = Arc::new(LocalPersistence::new(default_state_directory()));
    let (keymap, keymap_warnings) = Keymap::from_env();
    let portfolio_query: Arc<dyn PortfolioRepository> =
        Arc::new(CsvPortfolioRepository::persistent(repository.clone()));
    let risk_query: Arc<dyn RiskQuery> = Arc::new(PortfolioRiskQuery::new(portfolio_query.clone()));
    let news_query: Arc<dyn NewsFeed> = Arc::new(LiveNewsFeed::from_env());
    let live_market_data = configured_market_data();
    let live_security = Arc::new(LiveSecurityQuery::from_env(
        live_market_data.market_data.clone(),
        live_market_data.chart_history.clone(),
    ));
    let security_query: Arc<dyn SecurityQuery> = live_security.clone();
    let spreadsheet_market_data: Arc<dyn SpreadsheetMarketData> = Arc::new(
        LiveSpreadsheetMarketData::new(live_market_data.spreadsheet.clone(), live_security),
    );
    let alerts_query: Arc<dyn AlertsQuery> =
        Arc::new(LiveAlertsQuery::new(live_market_data.market_data.clone()));
    let watchlist_catalog: Arc<dyn WatchlistCatalog> =
        Arc::new(ConfiguredWatchlistCatalog::from_env());
    let initial_symbol = initial_chart_symbol();
    let snapshot_refresh_interval = quote_refresh_interval();
    let mut runtime_settings = runtime_settings_summary(snapshot_refresh_interval, &initial_symbol);
    runtime_settings.keybindings = keymap.status(keymap_warnings.len());
    let overview_query: Arc<dyn OverviewQuery> = Arc::new(LiveOverviewQuery::new(
        portfolio_query.clone(),
        news_query.clone(),
    ));
    let markets_query: Arc<dyn MarketsQuery> = Arc::new(LiveMarketsQuery::new(
        live_market_data.market_data.clone(),
        configured_market_symbols(),
        snapshot_refresh_interval,
    ));
    build_app(AppProviders {
        overview_query,
        markets_query,
        chat: Arc::new(IrcChatGateway::from_env()),
        portfolio_query,
        risk_query,
        news_query,
        article_opener: Some(Arc::new(SystemNewsArticleOpener)),
        spreadsheet_market_data,
        spreadsheet_workbook_store: Some(repository.clone()),
        market_data: live_market_data.market_data,
        watchlist_catalog,
        instrument_search: Arc::new(SecInstrumentSearch::from_env()),
        chart_history: live_market_data.chart_history,
        chart_primary: ChartInstrument::from_terminal_subject(&initial_symbol),
        security_query,
        security_symbol: format!("{initial_symbol} US"),
        security_document_opener: Some(Arc::new(SystemNewsArticleOpener)),
        alerts_query,
        alert_state_store: Some(repository.clone()),
        snapshot_refresh_interval,
        runtime_settings,
    })
    .with_keymap(keymap)
    .with_session_repository(repository)
}

struct LiveMarketDataProviders {
    spreadsheet: Arc<dyn SpreadsheetMarketData>,
    market_data: Arc<dyn MarketDataQuery>,
    chart_history: Arc<dyn ChartHistoryQuery>,
}

fn configured_market_data() -> LiveMarketDataProviders {
    let provider = std::env::var("MARKET_TERMINAL_MARKET_DATA_PROVIDER")
        .unwrap_or_else(|_| "yahoo".to_owned())
        .trim()
        .to_ascii_lowercase();
    match provider.as_str() {
        "alpaca" => {
            let adapter = Arc::new(AlpacaMarketData::from_env());
            LiveMarketDataProviders {
                spreadsheet: adapter.clone(),
                market_data: adapter.clone(),
                chart_history: adapter,
            }
        }
        "alpha-vantage" | "alphavantage" => {
            let adapter = Arc::new(AlphaVantageMarketData::from_env());
            LiveMarketDataProviders {
                spreadsheet: adapter.clone(),
                market_data: adapter.clone(),
                chart_history: adapter,
            }
        }
        "finnhub" => {
            let adapter = Arc::new(FinnhubMarketData::from_env());
            LiveMarketDataProviders {
                spreadsheet: adapter.clone(),
                market_data: adapter.clone(),
                chart_history: adapter,
            }
        }
        _ => {
            let adapter = Arc::new(YahooMarketData::from_env());
            LiveMarketDataProviders {
                spreadsheet: adapter.clone(),
                market_data: adapter.clone(),
                chart_history: adapter,
            }
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

fn runtime_settings_summary(
    quote_refresh_interval: Duration,
    chart_symbol: &str,
) -> RuntimeSettingsSummary {
    let provider = std::env::var("MARKET_TERMINAL_MARKET_DATA_PROVIDER")
        .unwrap_or_else(|_| "yahoo".to_owned())
        .trim()
        .to_ascii_lowercase();
    let (market_provider, market_credentials) = match provider.as_str() {
        "alpaca" => {
            let feed = std::env::var("ALPACA_FEED")
                .unwrap_or_else(|_| "iex".to_owned())
                .trim()
                .to_ascii_uppercase();
            let configured = env_present("APCA_API_KEY_ID") && env_present("APCA_API_SECRET_KEY");
            (
                format!("ALPACA · {}", if feed == "SIP" { "SIP" } else { "IEX" }),
                if configured { "CONFIGURED" } else { "MISSING" }.to_owned(),
            )
        }
        "alpha-vantage" | "alphavantage" => {
            let personal_key = std::env::var("ALPHA_VANTAGE_API_KEY").is_ok_and(|value| {
                let value = value.trim();
                !value.is_empty() && value != "demo"
            });
            (
                if personal_key {
                    "ALPHA VANTAGE · PERSONAL KEY"
                } else {
                    "ALPHA VANTAGE · DEMO (IBM ONLY)"
                }
                .to_owned(),
                if personal_key {
                    "CONFIGURED"
                } else {
                    "DEMO KEY"
                }
                .to_owned(),
            )
        }
        "finnhub" => (
            "FINNHUB · REALTIME US QUOTE · SESSION CHART".to_owned(),
            if env_present("FINNHUB_API_KEY") {
                "CONFIGURED"
            } else {
                "MISSING"
            }
            .to_owned(),
        ),
        _ => (
            "YAHOO FINANCE CHART · DELAYED · UNOFFICIAL".to_owned(),
            "NOT REQUIRED".to_owned(),
        ),
    };
    let assistant_provider = std::env::var("MARKET_TERMINAL_AI_PROVIDER")
        .unwrap_or_else(|_| "codex".to_owned())
        .trim()
        .to_ascii_lowercase();
    let ai_provider = if assistant_provider == "codex" {
        "CODEX · CHATGPT LOGIN".to_owned()
    } else if env_present("OPENROUTER_API_KEY") {
        "OPENROUTER · KEY CONFIGURED".to_owned()
    } else {
        "OPENROUTER · KEY MISSING".to_owned()
    };
    let news_sources = std::env::var("MARKET_TERMINAL_NEWS_FEEDS")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(|value| {
            format!(
                "{} CUSTOM RSS/ATOM FEED(S)",
                value
                    .split(',')
                    .filter(|feed| !feed.trim().is_empty())
                    .count()
            )
        })
        .unwrap_or_else(|| "3 DEFAULT LIVE RSS/ATOM FEEDS".to_owned());
    let portfolio_paths = [
        ("POSITIONS", "MARKET_TERMINAL_PORTFOLIO_CSV"),
        ("ACTIVITY", "MARKET_TERMINAL_PORTFOLIO_ACTIVITY_CSV"),
        ("PERFORMANCE", "MARKET_TERMINAL_PORTFOLIO_PERFORMANCE_CSV"),
        ("LOTS", "MARKET_TERMINAL_PORTFOLIO_TAX_LOTS_CSV"),
        ("REALIZED", "MARKET_TERMINAL_PORTFOLIO_REALIZED_GAINS_CSV"),
        ("TRADES", "MARKET_TERMINAL_PORTFOLIO_TRADES_CSV"),
        ("CONTRIBUTION", "MARKET_TERMINAL_PORTFOLIO_CONTRIBUTION_CSV"),
    ]
    .into_iter()
    .filter_map(|(label, variable)| env_present(variable).then_some(label))
    .collect::<Vec<_>>();

    RuntimeSettingsSummary {
        gallery_replay: false,
        market_provider,
        market_credentials,
        quote_refresh_seconds: quote_refresh_interval.as_secs(),
        watchlist: bounded_env("MARKET_TERMINAL_WATCHLIST", "AAPL,MSFT,NVDA", 96),
        market_symbols: bounded_env(
            "MARKET_TERMINAL_MARKETS_SYMBOLS",
            &bounded_env("MARKET_TERMINAL_WATCHLIST", "AAPL,MSFT,NVDA", 96),
            96,
        ),
        chart_symbol: chart_symbol.to_owned(),
        ai_provider,
        keybindings: "DEFAULT · VIM + TMUX FIXED".to_owned(),
        portfolio_import: if portfolio_paths.is_empty() {
            "NOT CONFIGURED".to_owned()
        } else {
            format!("{} PATH(S) CONFIGURED", portfolio_paths.join(" + "))
        },
        news_sources,
        irc: if env_present("IRC_SERVER") {
            "SERVER CONFIGURED"
        } else {
            "OFFLINE"
        }
        .to_owned(),
    }
}

fn env_present(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| !value.trim().is_empty())
}

fn bounded_env(name: &str, default: &str, maximum_chars: usize) -> String {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| default.to_owned())
        .chars()
        .take(maximum_chars)
        .collect()
}

fn configured_market_symbols() -> Vec<String> {
    std::env::var("MARKET_TERMINAL_MARKETS_SYMBOLS")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| std::env::var("MARKET_TERMINAL_WATCHLIST").ok())
        .unwrap_or_else(|| "AAPL,MSFT,NVDA".to_owned())
        .split(',')
        .map(str::trim)
        .filter(|symbol| !symbol.is_empty())
        .take(12)
        .map(ToOwned::to_owned)
        .collect()
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
        .unwrap_or_else(|| "AAPL".to_owned())
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
