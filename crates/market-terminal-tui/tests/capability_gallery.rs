use std::{collections::BTreeSet, fs, path::PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use market_terminal::{bootstrap, runtime, App};
use ratatui::{
    backend::TestBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph},
    Terminal,
};
use serde_json::Value;

const REQUIRED_STATES: [&str; 8] = [
    "loading",
    "populated",
    "empty",
    "delayed",
    "stale",
    "denied",
    "partial",
    "failed",
];
const SIZES: [(u16, u16); 3] = [(80, 24), (120, 36), (160, 48)];
const FNV_OFFSET: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

fn document(name: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("TUI crate should live under <repository>/crates")
        .join("docs")
        .join(name);
    let raw = fs::read_to_string(path).unwrap_or_else(|error| panic!("read {name}: {error}"));
    serde_json::from_str(&raw).unwrap_or_else(|error| panic!("parse {name}: {error}"))
}

#[test]
fn every_covered_capability_has_a_complete_three_size_state_gallery() {
    let ledger = document("openterminalui-parity-ledger.json");
    let gallery = document("capability-gallery.json");
    assert_eq!(gallery["schema_version"], 1);
    assert_eq!(gallery["required_status"], "covered");
    assert_eq!(strings(&gallery, "states"), REQUIRED_STATES);
    assert_eq!(strings(&gallery, "sizes"), ["80x24", "120x36", "160x48"]);

    let covered = ledger["capabilities"]
        .as_array()
        .expect("ledger capabilities")
        .iter()
        .filter(|entry| entry["market_status"] == "covered")
        .map(|entry| entry["id"].as_str().expect("covered id"))
        .collect::<BTreeSet<_>>();
    let frames = gallery["frames"].as_array().expect("gallery frames");
    let mut frame_ids = BTreeSet::new();
    let mut matrix = BTreeSet::new();
    let mut differences = Vec::new();

    for frame in frames {
        let id = text(frame, "id");
        let capability = text(frame, "capability_id");
        let state = text(frame, "state");
        let disposition = text(frame, "disposition");
        let scenario = text(frame, "scenario");
        let reason = text(frame, "reason");
        assert!(frame_ids.insert(id), "duplicate gallery frame {id}");
        assert!(
            covered.contains(capability),
            "{id} is not a covered capability"
        );
        assert!(
            REQUIRED_STATES.contains(&state),
            "{id} has invalid state {state}"
        );
        assert!(
            matches!(disposition, "rendered" | "not_applicable"),
            "{id} has invalid disposition {disposition}"
        );
        assert!(
            matrix.insert((capability, state)),
            "duplicate {capability}/{state}"
        );
        if disposition == "not_applicable" {
            assert!(
                reason.len() >= 24,
                "{id} needs a useful not-applicable reason"
            );
        }

        let expected = u64::from_str_radix(text(frame, "semantic_hash"), 16)
            .unwrap_or_else(|error| panic!("{id} semantic_hash: {error}"));
        let actual = aggregate_hash(capability, state, disposition, scenario, reason);
        if actual != expected {
            differences.push(format!(
                "{id}: expected {expected:016x}, actual {actual:016x}"
            ));
        }
    }

    for capability in &covered {
        for state in REQUIRED_STATES {
            assert!(
                matrix.contains(&(capability, state)),
                "missing gallery frame for {capability}/{state}"
            );
        }
        assert!(
            frames.iter().any(|frame| {
                frame["capability_id"] == *capability
                    && frame["state"] == "populated"
                    && frame["disposition"] == "rendered"
            }),
            "{capability} needs a rendered populated reference"
        );
    }
    assert_eq!(
        frames.len(),
        covered.len() * REQUIRED_STATES.len(),
        "the gallery must contain exactly one frame per covered capability/state"
    );
    assert!(
        differences.is_empty(),
        "capability gallery changed; review before updating hashes:\n{}",
        differences.join("\n")
    );
}

fn aggregate_hash(
    capability: &str,
    state: &str,
    disposition: &str,
    scenario: &str,
    reason: &str,
) -> u64 {
    let mut aggregate = FNV_OFFSET;
    for (width, height) in SIZES {
        let frame_hash = render_hash(
            width,
            height,
            capability,
            state,
            disposition,
            scenario,
            reason,
        );
        for byte in frame_hash.to_le_bytes() {
            aggregate ^= u64::from(byte);
            aggregate = aggregate.wrapping_mul(FNV_PRIME);
        }
    }
    aggregate
}

fn render_hash(
    width: u16,
    height: u16,
    capability: &str,
    state: &str,
    disposition: &str,
    scenario: &str,
    reason: &str,
) -> u64 {
    let mut app = bootstrap::demo_app();
    app.set_terminal_area(Rect::new(0, 0, width, height));
    prepare(&mut app, scenario);
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("gallery terminal");
    terminal
        .draw(|frame| {
            runtime::render(frame, &app);
            render_evidence_banner(frame, capability, state, disposition, reason);
        })
        .expect("render gallery frame");

    let mut hash = FNV_OFFSET;
    for cell in &terminal.backend().buffer().content {
        for byte in cell.symbol().as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(FNV_PRIME);
        }
        for component in [cell.fg, cell.bg] {
            for byte in format!("{component:?}").bytes() {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(FNV_PRIME);
            }
        }
        hash ^= u64::from(cell.modifier.bits());
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

fn prepare(app: &mut App, scenario: &str) {
    match scenario {
        "baseline" => {}
        "help-directory" => dispatch(app, "HELP"),
        "empty-command" => app.handle_key(key(KeyCode::Char('/'))),
        "malformed-command" => {
            dispatch(app, "CHART \"");
        }
        "follow-hints" => app.handle_key(key(KeyCode::Char('f'))),
        "empty-panel-focus" => {
            dispatch(app, "CHAT");
            app.handle_key(key(KeyCode::Esc));
        }
        "nord-theme" => dispatch(app, "THEME NORD"),
        "invalid-theme" => dispatch(app, "THEME NOT-A-THEME"),
        unknown => panic!("unknown gallery scenario {unknown}"),
    }
}

fn render_evidence_banner(
    frame: &mut ratatui::Frame,
    capability: &str,
    state: &str,
    disposition: &str,
    reason: &str,
) {
    let area = frame.area();
    let banner_height = area.height.min(4);
    let banner = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(banner_height)])
        .split(area)[1];
    let status = if disposition == "rendered" {
        "RENDERED"
    } else {
        "NOT APPLICABLE"
    };
    let color = if disposition == "rendered" {
        Color::Cyan
    } else {
        Color::Yellow
    };
    let title = format!(
        " EVIDENCE {capability} · {} · {status} ",
        state.to_ascii_uppercase()
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .title(Line::from(title).style(Style::new().fg(color).add_modifier(Modifier::BOLD)));
    frame.render_widget(
        Paragraph::new(reason)
            .block(block)
            .alignment(Alignment::Left)
            .style(Style::new().fg(Color::White).bg(Color::Black)),
        banner,
    );
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

fn strings<'a>(value: &'a Value, field: &str) -> Vec<&'a str> {
    value[field]
        .as_array()
        .unwrap_or_else(|| panic!("{field} should be an array"))
        .iter()
        .map(|entry| entry.as_str().unwrap_or_else(|| panic!("{field} string")))
        .collect()
}

fn text<'a>(value: &'a Value, field: &str) -> &'a str {
    value[field]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| panic!("{field} should be a non-empty string"))
}
