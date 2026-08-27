use std::{error::Error, time::Instant};

use market_terminal::features::spreadsheet::{
    domain::{CellAddress, Workbook, Worksheet},
    Spreadsheet,
};

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
    let mut spreadsheet = seeded_spreadsheet()?;

    // Warm allocator and clone paths before recording the bounded sample.
    spreadsheet.set_cell("A1", "1")?;
    spreadsheet.clear_history();

    let mut samples = Vec::with_capacity(SAMPLES);
    for sample in 0..SAMPLES {
        let started = Instant::now();
        spreadsheet.set_cell("A1", sample.to_string())?;
        samples.push(started.elapsed());
        spreadsheet.clear_history();
    }
    samples.sort_unstable();
    let p95 = samples[(SAMPLES * 95).div_ceil(100) - 1];
    let p95_ms = p95.as_secs_f64() * 1_000.0;
    println!(
        "spreadsheet_edit_p95_ms={p95_ms:.3} populated_cells={} samples={SAMPLES}",
        SHEETS * usize::from(COLUMNS) * usize::from(ROWS)
    );
    if p95_ms > limit_ms {
        return Err(format!("10k-cell edit p95 {p95_ms:.3} ms exceeded {limit_ms:.3} ms").into());
    }
    Ok(())
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
