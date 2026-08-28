use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use market_terminal::{bootstrap, runtime, App};
use ratatui::{backend::TestBackend, Terminal};

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
        hashes: [0xde9842c043fdb13c, 0x12cda5e51d679347, 0xfdb113f90e395920],
    },
    Golden {
        name: "instrument-find",
        prepare: prepare_find,
        hashes: [0x7bf921a1d5542973, 0xec4a6d37ee19d5c6, 0x82f485327339bd79],
    },
    Golden {
        name: "help",
        prepare: prepare_help,
        hashes: [0xc606e9b59e3d94f3, 0x801ada1ed7bff7d0, 0xaf23406824fdb6cb],
    },
    Golden {
        name: "workspace-preset-preview",
        prepare: prepare_workspace_preset_preview,
        hashes: [0x4cc072226fb733a1, 0x1e2d6be4abc5b584, 0xba1d8f937e51d562],
    },
    Golden {
        name: "spreadsheet-error",
        prepare: prepare_spreadsheet_error,
        hashes: [0x8f7d96e4015cc375, 0x0e805fb6022cc0a6, 0x027dfcbf10f4d98b],
    },
    Golden {
        name: "formula-editor",
        prepare: prepare_formula_editor,
        hashes: [0xd17d1354baf81fe3, 0x19a563994925ade0, 0x56b9ec16cfaffb09],
    },
    Golden {
        name: "risk",
        prepare: prepare_risk,
        hashes: [0xe1b4613461b2e418, 0xd0d495a90a0fc1c9, 0xf490670878307079],
    },
    Golden {
        name: "risk-history",
        prepare: prepare_risk_history,
        hashes: [0x2585b21f3c947110, 0x3726a67132e1222d, 0xe0134fa41e96413d],
    },
    Golden {
        name: "news",
        prepare: prepare_news,
        hashes: [0xe845a20f59083ce3, 0x559da760c32dc499, 0x58ab3ba74fbd5669],
    },
    Golden {
        name: "news-reader",
        prepare: prepare_news_reader,
        hashes: [0xff94c4456267fe0f, 0x30c7fb0a3ae066b0, 0xe9b2272fa91ef6f1],
    },
    Golden {
        name: "alerts",
        prepare: prepare_alerts,
        hashes: [0x51989730c470a18b, 0x18db67236fdd3013, 0xc889b0b131fdc289],
    },
    Golden {
        name: "panel-focus",
        prepare: prepare_panel_focus,
        hashes: [0x4e829f219f1a7025, 0x4ba506f53b8f138a, 0x117a60a4e7574cc9],
    },
    Golden {
        name: "follow-hints",
        prepare: prepare_follow_hints,
        hashes: [0xf4bb75e587d86b7f, 0xbdae658b18414f1a, 0xb34a90345098a389],
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

fn render_hash(prepare: fn(&mut App), width: u16, height: u16) -> u64 {
    let mut app = bootstrap::demo_app();
    prepare(&mut app);
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| runtime::render(frame, &app))
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
