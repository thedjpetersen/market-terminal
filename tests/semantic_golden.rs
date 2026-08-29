use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use market_terminal::{
    app::WorkspaceRegistry,
    bootstrap,
    features::overview::{
        LiveOverviewSnapshot, OverviewHealthState, OverviewPriority, OverviewQuery,
        OverviewSavedWork, OverviewSnapshot, OverviewSourceHealth, OverviewWorkspace,
        ID as OVERVIEW,
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
        name: "help",
        prepare: prepare_help,
        hashes: [0x14055b2667c88df3, 0xf3e5c9807260781f, 0x5032f11ee2437987],
    },
    Golden {
        name: "workspace-preset-preview",
        prepare: prepare_workspace_preset_preview,
        hashes: [0x7a04494f6250d87a, 0xe69eff31457179ee, 0xf344b282141f4221],
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

    let mut hash = 0xcbf29ce484222325_u64;
    for cell in &terminal.backend().buffer().content {
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

fn offline_mission_app() -> App {
    let registry = WorkspaceRegistry::new(vec![Box::new(OverviewWorkspace::new(Arc::new(
        OfflineMissionQuery,
    )))]);
    App::new(registry, OVERVIEW)
}

fn prepare_overview(_app: &mut App) {}

fn prepare_spreadsheet(app: &mut App) {
    dispatch(app, "SHEET");
}

fn prepare_find(app: &mut App) {
    dispatch(app, "FIND US");
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

fn prepare_risk(app: &mut App) {
    dispatch(app, "RISK");
}

fn prepare_risk_history(app: &mut App) {
    dispatch(app, "RISK HISTORY");
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
