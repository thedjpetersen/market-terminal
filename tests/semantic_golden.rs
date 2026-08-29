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
        hashes: [0x348f31f016a049e5, 0x1b4b5178b0b68201, 0xec9ee27f517ea8a7],
    },
    Golden {
        name: "instrument-find",
        prepare: prepare_find,
        hashes: [0x71f625791ea64bac, 0x023bef0fa9a381d4, 0x7f41ce0bff731b20],
    },
    Golden {
        name: "screening",
        prepare: prepare_screening,
        hashes: [0x002fb95dfe8fd5db, 0x96ce681b90918f31, 0x39c6cbecb8897a07],
    },
    Golden {
        name: "options-model",
        prepare: prepare_options,
        hashes: [0x6c109a0e38d556d9, 0x37db2f652388beb6, 0xb11a2e22c5e46443],
    },
    Golden {
        name: "fixed-income-model",
        prepare: prepare_fixed_income,
        hashes: [0x3595825619247cf2, 0xfef236b965db2e2e, 0x9d38b16843037df6],
    },
    Golden {
        name: "help",
        prepare: prepare_help,
        hashes: [0x44b797e9058346be, 0xa2c521511dfd15a4, 0xad1486544098d064],
    },
    Golden {
        name: "workspace-preset-preview",
        prepare: prepare_workspace_preset_preview,
        hashes: [0x6bcbf9b5b2d3068e, 0x4a2145bda9288031, 0x94cebd8494cfbd2d],
    },
    Golden {
        name: "launchpad",
        prepare: prepare_launchpad,
        hashes: [0x759a01e13135056b, 0x2ebeefd55789d13e, 0x0947fe47f37b361f],
    },
    Golden {
        name: "desk-layout",
        prepare: prepare_desk_layout,
        hashes: [0x25721fb84a222df1, 0x2086e600fe23b906, 0x22aba07c701e79e0],
    },
    Golden {
        name: "security-filings",
        prepare: prepare_security_filings,
        hashes: [0xf103e810cd528534, 0xbe2480070c52e9a8, 0x894dfb7feaae813e],
    },
    Golden {
        name: "spreadsheet-error",
        prepare: prepare_spreadsheet_error,
        hashes: [0xe20f49fbabedc1bc, 0x6cc81e54570bde28, 0xbb7a539b51f8bd24],
    },
    Golden {
        name: "formula-editor",
        prepare: prepare_formula_editor,
        hashes: [0xed4d9e54266f0326, 0x9ee89e35d51f6352, 0x619c25ba3a400f36],
    },
    Golden {
        name: "risk",
        prepare: prepare_risk,
        hashes: [0xf2025630f0df039b, 0x0ba08b07113c8e6f, 0xe423a99991bf1f32],
    },
    Golden {
        name: "risk-history",
        prepare: prepare_risk_history,
        hashes: [0x488c3c8a4a7e593d, 0x39076603416acd6f, 0xa50d790819644cf6],
    },
    Golden {
        name: "portfolio-attribution",
        prepare: prepare_portfolio_attribution,
        hashes: [0x4aefc7210d59007b, 0xd1d5fb5b1abda10a, 0x9ef9fea62cc991ad],
    },
    Golden {
        name: "news",
        prepare: prepare_news,
        hashes: [0x805d8a9df1bfbe74, 0x6073aa4a27122aa3, 0x07d6fbebbf2c303e],
    },
    Golden {
        name: "news-reader",
        prepare: prepare_news_reader,
        hashes: [0x2d1dffcf21e05e74, 0x2d48d33b39bcca9a, 0x293ec73e84efe14e],
    },
    Golden {
        name: "alerts",
        prepare: prepare_alerts,
        hashes: [0x643bd7ff822dc668, 0x768d62b2a5dcbf15, 0x9d5b524c32b98ab8],
    },
    Golden {
        name: "alerts-long-register",
        prepare: prepare_alerts_long_register,
        hashes: [0x9a75a634312cf209, 0x74ed799c5a29b5fe, 0x1c38e8573e4f89af],
    },
    Golden {
        name: "panel-focus",
        prepare: prepare_panel_focus,
        hashes: [0xdd251026c08fb468, 0x1c597ec8b6e164e4, 0x0e8dfb588e822e68],
    },
    Golden {
        name: "follow-hints",
        prepare: prepare_follow_hints,
        hashes: [0xa3c9b589dab3f911, 0xee6cb17a3560fca3, 0x8d623ebb76e220cf],
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
