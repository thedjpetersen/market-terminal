use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use market_terminal::{
    app::{CommandInvocation, Workspace, WorkspaceRegistry},
    bootstrap,
    features::overview::{
        LiveOverviewSnapshot, OverviewHealthState, OverviewPriority, OverviewQuery,
        OverviewSavedWork, OverviewSnapshot, OverviewSourceHealth, OverviewWorkspace,
        ID as OVERVIEW,
    },
    features::{
        market_data::{
            CanonicalInstrumentId, HistoryRequest, MarketDataError, MarketDataQuery, PriceBar,
            QuoteSnapshot,
        },
        news::{Headline, NewsFeed, NewsSnapshot, NewsWorkspace},
        watchlist::{WatchlistCatalog, WatchlistDefinition, WatchlistItem, WatchlistWorkspace},
    },
    runtime, App,
};
use ratatui::{backend::TestBackend, Terminal};
use std::sync::Arc;

struct Golden {
    name: &'static str,
    prepare: fn(&mut App),
    hashes: [u64; 3],
}

const SIZES: [(u16, u16); 3] = [(80, 24), (120, 36), (160, 48)];
const GOLDENS: &[Golden] = &[
    Golden {
        name: "overview",
        prepare: prepare_overview,
        hashes: [0x2643050e5451cb41, 0xb4eb9bce4d88db55, 0x000972c989c6f858],
    },
    Golden {
        name: "spreadsheet",
        prepare: prepare_spreadsheet,
        hashes: [0xf9230f8a8fd14c57, 0x0e94acb21950e789, 0x6e86fd7d6962bf6f],
    },
    Golden {
        name: "instrument-find",
        prepare: prepare_find,
        hashes: [0xf2fe9d208fbbf51a, 0x67645010b054a87c, 0x501951749dd66918],
    },
    Golden {
        name: "screening",
        prepare: prepare_screening,
        hashes: [0xa9d705cf202c12d9, 0x492e36dda26eb185, 0x71ec920512da20ab],
    },
    Golden {
        name: "options-model",
        prepare: prepare_options,
        hashes: [0x89e75aebc0b927c3, 0xe128dca45a5b5bbe, 0x2f8cb8de0dc6689b],
    },
    Golden {
        name: "fixed-income-model",
        prepare: prepare_fixed_income,
        hashes: [0x5f7530bf182c7190, 0xcf1747e6a06b6676, 0x9a15225f3a14d7fe],
    },
    Golden {
        name: "help",
        prepare: prepare_help,
        hashes: [0xe4ced9ce670d87b0, 0x5fd336349ff8d7fc, 0x2c9070f90bec679c],
    },
    Golden {
        name: "workspace-preset-preview",
        prepare: prepare_workspace_preset_preview,
        hashes: [0xe5a250d709192288, 0xeda58d065ca99f79, 0x3d6f5ea7507468c5],
    },
    Golden {
        name: "launchpad",
        prepare: prepare_launchpad,
        hashes: [0xa0b8b3900ded6afd, 0x34b7a8a493166916, 0xc50402d338051ee7],
    },
    Golden {
        name: "desk-layout",
        prepare: prepare_desk_layout,
        hashes: [0xbcac2bcd52343537, 0xd5f4496a79a6de96, 0x7861cc979c624014],
    },
    Golden {
        name: "security-filings",
        prepare: prepare_security_filings,
        hashes: [0xe1ef579f6bc34f82, 0xe51ad15cc85ada20, 0x24fa6cff600dc666],
    },
    Golden {
        name: "spreadsheet-error",
        prepare: prepare_spreadsheet_error,
        hashes: [0x76a9644580a265ae, 0x00a7bbd2419b59f0, 0x5a9c39b397b6aacc],
    },
    Golden {
        name: "formula-editor",
        prepare: prepare_formula_editor,
        hashes: [0x2567bfb6c3bd4a20, 0x2264db06aa916a6a, 0x08c62dc4587a6fae],
    },
    Golden {
        name: "risk",
        prepare: prepare_risk,
        hashes: [0x4c5c0bfc0464b49a, 0x209b1dbfccefb00e, 0x9638fa4de3585cad],
    },
    Golden {
        name: "risk-history",
        prepare: prepare_risk_history,
        hashes: [0x5d20bf61782ba5b8, 0x0e8e78bbf288768e, 0xf8a0b18e4a108179],
    },
    Golden {
        name: "portfolio-attribution",
        prepare: prepare_portfolio_attribution,
        hashes: [0x70b0d0a136cdd9d5, 0x0193d7061992cd02, 0xb74c337632467645],
    },
    Golden {
        name: "news",
        prepare: prepare_news,
        hashes: [0xaaf54e7509f037ca, 0x540542c85a6ad144, 0x155ecf03164656ca],
    },
    Golden {
        name: "news-reader",
        prepare: prepare_news_reader,
        hashes: [0xe0fc790837272076, 0x65167d1a9a0d1905, 0x2ce872c93c82a156],
    },
    Golden {
        name: "alerts",
        prepare: prepare_alerts,
        hashes: [0x80aaf89ad4b6c6e6, 0x561c29a243634edd, 0xe5f5c81aa89b8130],
    },
    Golden {
        name: "alerts-long-register",
        prepare: prepare_alerts_long_register,
        hashes: [0xab2e3d9b7dfd915b, 0x6cafbcd9f9af6406, 0xbc8767bb4cff9067],
    },
    Golden {
        name: "panel-focus",
        prepare: prepare_panel_focus,
        hashes: [0xebaece9fbc211136, 0xbd6c922be7814bcc, 0x8b293512b6dc3cb0],
    },
    Golden {
        name: "follow-hints",
        prepare: prepare_follow_hints,
        hashes: [0x556423b50c616d73, 0x1f41b7a2307f771b, 0x08272a1e1c58f387],
    },
];

#[test]
fn key_workspaces_match_semantic_goldens_at_standard_sizes() {
    let mut differences = Vec::new();
    for golden in GOLDENS {
        for (index, (width, height)) in SIZES.into_iter().enumerate() {
            let actual = render_hash(golden.prepare, width, height);
            if actual != golden.hashes[index] {
                differences.push(format!(
                    "{} {width}x{height}: expected 0x{:016x}, actual 0x{actual:016x}",
                    golden.name, golden.hashes[index]
                ));
            }
        }
    }
    assert!(
        differences.is_empty(),
        "semantic frames changed; review them before updating the intentional hashes:\n{}",
        differences.join("\n")
    );
}

#[test]
fn offline_mission_control_matches_semantic_goldens_at_standard_sizes() {
    let expected = [0x95ef8129ead0501d, 0x6d3f1f2325402473, 0x61e0560a59eb00c1];
    let actual = SIZES.map(|(width, height)| {
        let app = offline_mission_app();
        render_app_hash(&app, width, height)
    });
    assert_eq!(
        actual, expected,
        "review Mission Control offline frames before updating hashes"
    );
}

#[test]
fn filtered_news_events_match_semantic_goldens_at_standard_sizes() {
    let expected = [0x366d6cb47e5addfe, 0x66205393d2a971e3, 0x9e1a8d8bb1e245e3];
    let actual = SIZES.map(|(width, height)| {
        let workspace = filtered_news_workspace();
        render_news_hash(&workspace, width, height)
    });
    assert_eq!(
        actual, expected,
        "review the filtered News events frame before updating hashes"
    );
}

#[test]
fn recoverable_monitor_matches_semantic_goldens_at_standard_sizes() {
    let expected = [0x7af12af6e074bac4, 0xf5eab5bf2b9b5b3f, 0x06d148df09f41ad2];
    let actual = SIZES.map(|(width, height)| {
        let workspace = recoverable_monitor_workspace();
        render_monitor_hash(&workspace, width, height)
    });
    assert_eq!(
        actual, expected,
        "review the recoverable Monitor frame before updating hashes"
    );
}

fn render_hash(prepare: fn(&mut App), width: u16, height: u16) -> u64 {
    let mut app = bootstrap::demo_app();
    prepare(&mut app);
    render_app_hash(&app, width, height)
}

fn render_app_hash(app: &App, width: u16, height: u16) -> u64 {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| runtime::render(frame, app))
        .expect("render golden frame");

    semantic_hash(terminal.backend().buffer())
}

fn render_news_hash(workspace: &NewsWorkspace, width: u16, height: u16) -> u64 {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| workspace.render(frame, frame.area()))
        .expect("render filtered News golden frame");
    semantic_hash(terminal.backend().buffer())
}

fn render_monitor_hash(workspace: &WatchlistWorkspace, width: u16, height: u16) -> u64 {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| workspace.render(frame, frame.area()))
        .expect("render recoverable Monitor golden frame");
    semantic_hash(terminal.backend().buffer())
}

fn recoverable_monitor_workspace() -> WatchlistWorkspace {
    let mut workspace =
        WatchlistWorkspace::new(Arc::new(GoldenMarketData), Arc::new(GoldenWatchlistCatalog));
    workspace.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
    workspace.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::SHIFT));
    workspace.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
    workspace.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    workspace
}

struct GoldenMarketData;

impl MarketDataQuery for GoldenMarketData {
    fn quote_snapshots(
        &self,
        _instruments: &[CanonicalInstrumentId],
    ) -> Result<Vec<QuoteSnapshot>, MarketDataError> {
        Err(MarketDataError::TemporarilyUnavailable(
            "deterministic golden".to_owned(),
        ))
    }

    fn price_history(&self, _request: &HistoryRequest) -> Result<Vec<PriceBar>, MarketDataError> {
        Ok(Vec::new())
    }
}

struct GoldenWatchlistCatalog;

impl WatchlistCatalog for GoldenWatchlistCatalog {
    fn load_watchlist(&self, name: Option<&str>) -> Option<WatchlistDefinition> {
        let name = name.unwrap_or("MOVERS");
        name.eq_ignore_ascii_case("MOVERS").then(|| {
            WatchlistDefinition::new(
                "movers",
                "RECOVERABLE MOVERS",
                ["NVDA", "META", "AAPL", "MSFT"]
                    .into_iter()
                    .map(|symbol| {
                        WatchlistItem::new(
                            CanonicalInstrumentId::new(format!(
                                "us:xnas:{}",
                                symbol.to_ascii_lowercase()
                            )),
                            symbol,
                            symbol,
                        )
                    })
                    .collect(),
            )
        })
    }
}

fn semantic_hash(buffer: &ratatui::buffer::Buffer) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for cell in &buffer.content {
        for byte in cell.symbol().as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        hash ^= 0xff;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

struct OfflineMissionQuery;

impl OverviewQuery for OfflineMissionQuery {
    fn load_overview(&self) -> OverviewSnapshot {
        OverviewSnapshot::Live(Box::new(LiveOverviewSnapshot {
            net_asset_value: "—".to_owned(),
            ytd_return: "N/A".to_owned(),
            available_cash: "—".to_owned(),
            sharpe: "N/A".to_owned(),
            portfolio_source: "POSITIONS NOT CONFIGURED".to_owned(),
            portfolio_as_of: "—".to_owned(),
            holdings: Vec::new(),
            headlines: Vec::new(),
            news_status: "LIVE FEED UNAVAILABLE · OFFLINE".to_owned(),
            market_pulse: Vec::new(),
            events: Vec::new(),
            source_health: vec![
                OverviewSourceHealth {
                    source: "PORTFOLIO".to_owned(),
                    state: OverviewHealthState::NotConfigured,
                    detail: "NO POSITION SNAPSHOT".to_owned(),
                    as_of: "—".to_owned(),
                    command: "PORT".to_owned(),
                },
                OverviewSourceHealth {
                    source: "MARKETS".to_owned(),
                    state: OverviewHealthState::Unavailable,
                    detail: "MARKET SNAPSHOT UNAVAILABLE · OFFLINE".to_owned(),
                    as_of: "—".to_owned(),
                    command: "MARKETS".to_owned(),
                },
                OverviewSourceHealth {
                    source: "NEWS".to_owned(),
                    state: OverviewHealthState::Unavailable,
                    detail: "LIVE FEED UNAVAILABLE · OFFLINE".to_owned(),
                    as_of: "CURRENT CACHE".to_owned(),
                    command: "NEWS".to_owned(),
                },
            ],
            saved_work: vec![OverviewSavedWork {
                id: 1,
                label: "Mission Control".to_owned(),
                command: "HOME".to_owned(),
                kind: "COMMAND TILE".to_owned(),
            }],
            priorities: vec![
                OverviewPriority {
                    id: "market-pulse-unavailable".to_owned(),
                    score: 80,
                    title: "Market pulse is unavailable".to_owned(),
                    reason: "MARKET SNAPSHOT UNAVAILABLE · OFFLINE".to_owned(),
                    source: "MARKETS".to_owned(),
                    as_of: "—".to_owned(),
                    command: "MARKETS".to_owned(),
                },
                OverviewPriority {
                    id: "portfolio-missing".to_owned(),
                    score: 65,
                    title: "Portfolio snapshot not configured".to_owned(),
                    reason: "Import positions to personalize priorities".to_owned(),
                    source: "PORTFOLIO".to_owned(),
                    as_of: "—".to_owned(),
                    command: "PORT".to_owned(),
                },
            ],
        }))
    }
}

struct GoldenNewsFeed;

impl NewsFeed for GoldenNewsFeed {
    fn load_news(&self) -> NewsSnapshot {
        NewsSnapshot {
            headlines: vec![
                Headline {
                    time: "16:00".to_owned(),
                    topic: "TOP".to_owned(),
                    title: "Markets gain".to_owned(),
                    region: "US".to_owned(),
                },
                Headline {
                    time: "14:00".to_owned(),
                    topic: "TEC".to_owned(),
                    title: "Chip rally".to_owned(),
                    region: "AS".to_owned(),
                },
            ],
        }
    }
}

fn offline_mission_app() -> App {
    let registry = WorkspaceRegistry::new(vec![Box::new(OverviewWorkspace::new(Arc::new(
        OfflineMissionQuery,
    )))]);
    App::new(registry, OVERVIEW)
}

fn filtered_news_workspace() -> NewsWorkspace {
    let mut workspace = NewsWorkspace::new(Arc::new(GoldenNewsFeed));
    workspace.handle_command(&CommandInvocation {
        function: "NEWS".to_owned(),
        args: vec![
            "--region=AS".to_owned(),
            "--topic=TEC".to_owned(),
            "--symbol=NVDA".to_owned(),
            "--events".to_owned(),
        ],
    });
    workspace
}

fn prepare_overview(_app: &mut App) {}

fn prepare_spreadsheet(app: &mut App) {
    dispatch(app, "SHEET");
}

fn prepare_find(app: &mut App) {
    dispatch(app, "FIND US");
}

fn prepare_screening(app: &mut App) {
    dispatch(app, "SCREEN momentum");
    for _ in 0..20 {
        std::thread::sleep(std::time::Duration::from_millis(1));
        app.advance_tick();
    }
}

fn prepare_options(app: &mut App) {
    dispatch(app, "OPTIONS AAPL CALL 190 200 30 25 5 0 100");
}

fn prepare_fixed_income(app: &mut App) {
    dispatch(app, "BOND UST-5Y-REFERENCE USD 100 4.5 4.25 5 SEMI 0");
}

fn prepare_help(app: &mut App) {
    dispatch(app, "HELP");
}

fn prepare_workspace_preset_preview(app: &mut App) {
    dispatch(app, "PRESET TRADER");
}

fn prepare_launchpad(app: &mut App) {
    dispatch(app, "LAUNCH");
}

fn prepare_desk_layout(app: &mut App) {
    dispatch(app, "DESK LAYOUT 60 65");
}

fn prepare_security_filings(app: &mut App) {
    dispatch(app, "SEC AAPL US --view=filings");
    std::thread::sleep(std::time::Duration::from_millis(10));
    app.advance_tick();
}

fn prepare_risk(app: &mut App) {
    dispatch(app, "RISK");
}

fn prepare_risk_history(app: &mut App) {
    dispatch(app, "RISK HISTORY");
}

fn prepare_portfolio_attribution(app: &mut App) {
    dispatch(app, "PORT ATTRIBUTION");
    app.handle_key(key(KeyCode::Down));
}

fn prepare_news(app: &mut App) {
    dispatch(app, "NEWS");
}

fn prepare_news_reader(app: &mut App) {
    dispatch(app, "NEWS");
    app.handle_key(key(KeyCode::Enter));
}

fn prepare_alerts(app: &mut App) {
    dispatch(app, "ALERTS");
    std::thread::sleep(std::time::Duration::from_millis(10));
    app.advance_tick();
}

fn prepare_alerts_long_register(app: &mut App) {
    for index in 0..18 {
        dispatch(app, &format!("ALERT T{index:03} > {}", 100 + index));
    }
}

fn prepare_panel_focus(app: &mut App) {
    app.handle_key(key(KeyCode::Esc));
}

fn prepare_follow_hints(app: &mut App) {
    app.handle_key(key(KeyCode::Char('f')));
}

fn prepare_spreadsheet_error(app: &mut App) {
    dispatch(app, "SHEET");
    type_keys(app, "=1/0");
    app.handle_key(key(KeyCode::Enter));
}

fn prepare_formula_editor(app: &mut App) {
    dispatch(app, "SHEET");
    type_keys(app, "=SUM(");
}

fn type_keys(app: &mut App, value: &str) {
    for character in value.chars() {
        app.handle_key(key(KeyCode::Char(character)));
    }
}

fn dispatch(app: &mut App, command: &str) {
    app.handle_key(key(KeyCode::Char('/')));
    type_keys(app, command);
    app.handle_key(key(KeyCode::Enter));
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}
