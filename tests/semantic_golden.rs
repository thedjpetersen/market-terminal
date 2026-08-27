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
        hashes: [0xad1bc4ee81ae2cfe, 0x4d20ca09a3d22f4c, 0x46cc83cf62018dee],
    },
    Golden {
        name: "instrument-find",
        prepare: prepare_find,
        hashes: [0xf3034720659b3376, 0x22352acf1570e01d, 0x6e445058c09e01e6],
    },
    Golden {
        name: "help",
        prepare: prepare_help,
        hashes: [0xa8d3eaac702baf6e, 0xa38286539f15469e, 0x6eb9e8623cf96568],
    },
    Golden {
        name: "spreadsheet-error",
        prepare: prepare_spreadsheet_error,
        hashes: [0x41f72239d704a8df, 0xa813ee830550ef95, 0xe84105388a50d901],
    },
    Golden {
        name: "formula-editor",
        prepare: prepare_formula_editor,
        hashes: [0x49878e462c384b06, 0xd4ba18ca262ec3b0, 0x712066c23fb8f077],
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
