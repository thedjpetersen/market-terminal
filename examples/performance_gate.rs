use std::{error::Error, time::Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use market_terminal::{
    bootstrap,
    features::spreadsheet::{
        domain::{CellAddress, Workbook, Worksheet},
        Spreadsheet,
    },
    runtime,
};
use ratatui::{backend::TestBackend, layout::Rect, Terminal};

const SHEETS: usize = 5;
const COLUMNS: u8 = 20;
const ROWS: u16 = 100;
const SAMPLES: usize = 100;

fn main() -> Result<(), Box<dyn Error>> {
    let limit_ms = std::env::args()
        .nth(1)
        .map(|value| value.parse::<f64>())
        .transpose()?
        .unwrap_or(50.0);
    let results = [
        command_dispatch_case()?,
        visible_action_routing_case()?,
        responsive_theme_render_case()?,
        spreadsheet_edit_case()?,
    ];

    for result in results {
        println!(
            "{}_p95_ms={:.3} {} samples={SAMPLES}",
            result.name, result.p95_ms, result.context
        );
        if result.p95_ms > limit_ms {
            return Err(format!(
                "{} p95 {:.3} ms exceeded {:.3} ms",
                result.name, result.p95_ms, limit_ms
            )
            .into());
        }
    }
    Ok(())
}

struct CaseResult {
    name: &'static str,
    p95_ms: f64,
    context: String,
}

fn command_dispatch_case() -> Result<CaseResult, Box<dyn Error>> {
    let mut app = bootstrap::demo_app();
    dispatch_help(&mut app);
    app.handle_key(key(KeyCode::Esc));
    let p95_ms = measure(|_| {
        dispatch_help(&mut app);
        app.handle_key(key(KeyCode::Esc));
        Ok(())
    })?;
    Ok(CaseResult {
        name: "command_dispatch",
        p95_ms,
        context: "exact_help_command=true".to_owned(),
    })
}

fn visible_action_routing_case() -> Result<CaseResult, Box<dyn Error>> {
    let mut app = bootstrap::demo_app();
    app.set_terminal_area(Rect::new(0, 0, 160, 48));
    route_visible_action(&mut app);
    let p95_ms = measure(|_| {
        route_visible_action(&mut app);
        Ok(())
    })?;
    Ok(CaseResult {
        name: "visible_action_routing",
        p95_ms,
        context: "viewport=160x48".to_owned(),
    })
}

fn responsive_theme_render_case() -> Result<CaseResult, Box<dyn Error>> {
    let mut app = bootstrap::demo_app();
    app.set_terminal_area(Rect::new(0, 0, 160, 48));
    dispatch(&mut app, "THEME NORD");
    let backend = TestBackend::new(160, 48);
    let mut terminal = Terminal::new(backend)?;
    terminal.draw(|frame| runtime::render(frame, &app))?;
    let p95_ms = measure(|_| {
        terminal.draw(|frame| runtime::render(frame, &app))?;
        Ok(())
    })?;
    Ok(CaseResult {
        name: "responsive_theme_render",
        p95_ms,
        context: "theme=nord viewport=160x48".to_owned(),
    })
}

fn spreadsheet_edit_case() -> Result<CaseResult, Box<dyn Error>> {
    let mut spreadsheet = seeded_spreadsheet()?;
    spreadsheet.set_cell("A1", "1")?;
    spreadsheet.clear_history();
    let p95_ms = measure(|sample| {
        spreadsheet.set_cell("A1", sample.to_string())?;
        spreadsheet.clear_history();
        Ok(())
    })?;
    Ok(CaseResult {
        name: "spreadsheet_edit",
        p95_ms,
        context: format!(
            "populated_cells={}",
            SHEETS * usize::from(COLUMNS) * usize::from(ROWS)
        ),
    })
}

fn measure(
    mut operation: impl FnMut(usize) -> Result<(), Box<dyn Error>>,
) -> Result<f64, Box<dyn Error>> {
    let mut samples = Vec::with_capacity(SAMPLES);
    for sample in 0..SAMPLES {
        let started = Instant::now();
        operation(sample)?;
        samples.push(started.elapsed());
    }
    samples.sort_unstable();
    let p95 = samples[(SAMPLES * 95).div_ceil(100) - 1];
    Ok(p95.as_secs_f64() * 1_000.0)
}

fn dispatch_help(app: &mut market_terminal::App) {
    dispatch(app, "HELP");
}

fn dispatch(app: &mut market_terminal::App, command: &str) {
    app.handle_key(key(KeyCode::Char('/')));
    for character in command.chars() {
        app.handle_key(key(KeyCode::Char(character)));
    }
    app.handle_key(key(KeyCode::Enter));
}

fn route_visible_action(app: &mut market_terminal::App) {
    app.handle_key(key(KeyCode::Esc));
    app.handle_key(key(KeyCode::Right));
    app.handle_key(key(KeyCode::Esc));
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn seeded_spreadsheet() -> Result<Spreadsheet, Box<dyn Error>> {
    let mut sheets = Vec::with_capacity(SHEETS);
    for sheet_index in 0..SHEETS {
        let mut sheet = Worksheet::new(format!("Sheet{}", sheet_index + 1))?;
        for row in 1..=ROWS {
            for column in 1..=COLUMNS {
                sheet.set(
                    CellAddress::new(column, row)?,
                    (usize::from(row) * usize::from(column)).to_string(),
                );
            }
        }
        sheets.push(sheet);
    }
    Ok(Spreadsheet::from_workbook(Workbook::from_sheets(
        sheets, 0,
    )?))
}
