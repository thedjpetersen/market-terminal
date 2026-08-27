use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use market_terminal::{bootstrap, runtime, App};
use ratatui::{backend::TestBackend, Terminal};

struct Golden {
    name: &'static str,
    command: Option<&'static str>,
    hashes: [u64; 3],
}

const SIZES: [(u16, u16); 3] = [(80, 24), (120, 36), (160, 48)];
const GOLDENS: &[Golden] = &[
    Golden {
        name: "overview",
        command: None,
        hashes: [0x0c64a4a66870c3f4, 0x7bbab2ae683f6150, 0x2c80f38f97eed819],
    },
    Golden {
        name: "spreadsheet",
        command: Some("SHEET"),
        hashes: [0x8be87768e91eadca, 0x7612ad583f31a7ad, 0xd13b648077ba9ac1],
    },
    Golden {
        name: "instrument-find",
        command: Some("FIND US"),
        hashes: [0xf3034720659b3376, 0x22352acf1570e01d, 0x6e445058c09e01e6],
    },
];

#[test]
fn key_workspaces_match_semantic_goldens_at_standard_sizes() {
    let mut differences = Vec::new();
    for golden in GOLDENS {
        for (index, (width, height)) in SIZES.into_iter().enumerate() {
            let actual = render_hash(golden.command, width, height);
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

fn render_hash(command: Option<&str>, width: u16, height: u16) -> u64 {
    let mut app = bootstrap::demo_app();
    if let Some(command) = command {
        dispatch(&mut app, command);
    }
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

fn dispatch(app: &mut App, command: &str) {
    app.handle_key(key(KeyCode::Char('/')));
    for character in command.chars() {
        app.handle_key(key(KeyCode::Char(character)));
    }
    app.handle_key(key(KeyCode::Enter));
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}
