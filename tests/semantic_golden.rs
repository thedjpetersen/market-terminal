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
        hashes: [0x0c64a4a66870c3f4, 0x7bbab2ae683f6150, 0x2c80f38f97eed819],
    },
    Golden {
        name: "spreadsheet",
        prepare: prepare_spreadsheet,
        hashes: [0xf064835119e3686b, 0xe2bfaf5777557e38, 0xb2b60469e38e58b9],
    },
    Golden {
        name: "instrument-find",
        prepare: prepare_find,
        hashes: [0x7bf921a1d5542973, 0xec4a6d37ee19d5c6, 0x82f485327339bd79],
    },
    Golden {
        name: "help",
        prepare: prepare_help,
        hashes: [0x9633e81a01822201, 0xb9a367ae67273d60, 0xded1509f1ea7c3a7],
    },
    Golden {
        name: "spreadsheet-error",
        prepare: prepare_spreadsheet_error,
        hashes: [0x3df370b41828843a, 0xe07effa626eb77c1, 0xa5009ce23f8164ce],
    },
    Golden {
        name: "formula-editor",
        prepare: prepare_formula_editor,
        hashes: [0x1125e167d1dbe683, 0x4bba6d282c26f8e4, 0xc7778e330797e924],
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
        name: "panel-focus",
        prepare: prepare_panel_focus,
        hashes: [0x083f507844abbfca, 0xc81d613d8914778c, 0x7f9d0ce5b3f01b47],
    },
    Golden {
        name: "follow-hints",
        prepare: prepare_follow_hints,
        hashes: [0xe2f842a7fda34a62, 0x7d8ba10c4a0f4fe1, 0xce5fec314a6f72cd],
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

fn prepare_risk(app: &mut App) {
    dispatch(app, "RISK");
}

fn prepare_risk_history(app: &mut App) {
    dispatch(app, "RISK HISTORY");
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
