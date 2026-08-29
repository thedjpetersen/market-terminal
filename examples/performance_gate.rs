use std::{error::Error, time::Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use market_terminal::{
    bootstrap,
    features::{
        backtesting::{run_backtest, BacktestBar, BacktestConfig},
        screening::{
            evaluate_screen, universe_content_digest, Comparison, ScreenClause, ScreenDefinition,
            ScreenExpression, ScreenField, ScreenSortDirection, UniverseMember, UniverseSnapshot,
            MAX_UNIVERSE_MEMBERS,
        },
        spreadsheet::{
            domain::{CellAddress, Workbook, Worksheet},
            Spreadsheet,
        },
    },
    foundation::InstrumentId,
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
        discovery_search_case()?,
        visible_action_routing_case()?,
        responsive_theme_render_case()?,
        backtest_replay_case()?,
        screening_evaluation_case()?,
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

fn backtest_replay_case() -> Result<CaseResult, Box<dyn Error>> {
    let config = BacktestConfig::moving_average_cross("performance:instrument", "PERF");
    let bars = (0..5_000)
        .map(|index| {
            let trend = 100_000_000_i64 + index as i64 * 2_500;
            let cycle = ((index % 180) as i64 - 90).abs() * 20_000;
            let close = trend + cycle;
            BacktestBar {
                timestamp: 1_600_000_000 + index as i64 * 86_400,
                open_micros: close - 10_000,
                high_micros: close + 100_000,
                low_micros: close - 100_000,
                close_micros: close,
                volume: 10_000_000 + index as u64,
            }
        })
        .collect::<Vec<_>>();
    let expected = run_backtest(&config, &bars, "PERFORMANCE", "REPLAY", "PERF-V1")?.run_digest;
    let p95_ms = measure(|_| {
        let run = run_backtest(&config, &bars, "PERFORMANCE", "REPLAY", "PERF-V1")?;
        if run.run_digest != expected {
            return Err("backtest replay digest changed".into());
        }
        Ok(())
    })?;
    Ok(CaseResult {
        name: "backtest_replay",
        p95_ms,
        context: "bars=5000 next_open=true costs=true digest=verified".to_owned(),
    })
}

fn discovery_search_case() -> Result<CaseResult, Box<dyn Error>> {
    let mut app = bootstrap::demo_app();
    search_discovery(&mut app);
    let p95_ms = measure(|_| {
        search_discovery(&mut app);
        Ok(())
    })?;
    Ok(CaseResult {
        name: "discovery_search",
        p95_ms,
        context: "commands+workspaces+launchpad query=portfolio".to_owned(),
    })
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

fn screening_evaluation_case() -> Result<CaseResult, Box<dyn Error>> {
    let definition = ScreenDefinition::new_expression(
        "performance",
        "PERFORMANCE",
        "performance",
        ScreenExpression::All(vec![
            ScreenExpression::Any(vec![
                ScreenExpression::Predicate(ScreenClause::new(
                    ScreenField::ChangePercent,
                    Comparison::GreaterThan,
                    0.0,
                )?),
                ScreenExpression::Predicate(ScreenClause::new(
                    ScreenField::Volume,
                    Comparison::GreaterThanOrEqual,
                    10_000_000.0,
                )?),
            ]),
            ScreenExpression::Not(Box::new(ScreenExpression::Predicate(ScreenClause::new(
                ScreenField::SpreadBps,
                Comparison::GreaterThan,
                7.5,
            )?))),
            ScreenExpression::Any(vec![
                ScreenExpression::Predicate(ScreenClause::new(
                    ScreenField::DayRangePercent,
                    Comparison::GreaterThanOrEqual,
                    1.0,
                )?),
                ScreenExpression::Predicate(ScreenClause::new(
                    ScreenField::Last,
                    Comparison::LessThan,
                    125.0,
                )?),
            ]),
        ]),
        ScreenField::ChangePercent,
        ScreenSortDirection::Descending,
        200,
        false,
    )?;
    let universe = UniverseSnapshot::new(
        "performance",
        "PERFORMANCE",
        1,
        "2026-08-29T00:00:00Z",
        "DETERMINISTIC PERFORMANCE FIXTURE",
        (0..MAX_UNIVERSE_MEMBERS)
            .map(|index| UniverseMember {
                instrument_id: InstrumentId::new(format!("test:listed:t{index:04}")),
                symbol: format!("T{index:04}"),
                description: format!("PERFORMANCE MEMBER {index}"),
                currency: "USD".to_owned(),
                last: Some(10.0 + index as f64 / 10.0),
                change_percent: Some((index % 41) as f64 - 20.0),
                volume: Some(750_000.0 + index as f64 * 25_000.0),
                spread_bps: Some((index % 17) as f64 / 2.0),
                day_range_percent: Some((index % 11) as f64 / 3.0),
                quality: "DETERMINISTIC".to_owned(),
                provider: "PERFORMANCE FIXTURE".to_owned(),
            })
            .collect(),
    )?;
    let expected_digest = universe_content_digest(&universe);
    evaluate_screen(&definition, universe.clone())?;
    let p95_ms = measure(|_| {
        if universe_content_digest(&universe) != expected_digest {
            return Err("screening replay digest changed".into());
        }
        evaluate_screen(&definition, universe.clone())?;
        Ok(())
    })?;
    Ok(CaseResult {
        name: "screening_evaluation",
        p95_ms,
        context: format!(
            "universe_members={} nested_predicates={} limit={} replay_digest=verified",
            universe.members.len(),
            definition.clauses.len(),
            definition.limit
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

fn search_discovery(app: &mut market_terminal::App) {
    dispatch(app, "DISCOVER portfolio");
    app.handle_key(key(KeyCode::Esc));
    app.handle_key(key(KeyCode::Esc));
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
