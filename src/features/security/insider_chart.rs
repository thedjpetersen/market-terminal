//! Recent Form 4 value visualization.
//!
//! The log-value scatter, selected-mark emphasis, collision nudge, and
//! two-sided weekly bars adapt `makeev/alphai-tui` at commit
//! `9143d2e1176d0a67a9f26960427cf370187fc2e6`.
//! Copyright (c) 2026 Mikhail Makeev, used under the MIT License. See
//! `THIRD_PARTY_NOTICES.md` at the repository root.

use std::collections::BTreeMap;

use chrono::{Datelike, NaiveDate};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::ui::{
    components::terminal_block,
    theme::{AMBER, BG, CYAN, GREEN, MUTED, RED},
};

use super::InsiderTransaction;

const SCATTER_ROWS: u16 = 5;
const BAR_ROWS: u16 = 4;

struct Mark {
    index: usize,
    date: NaiveDate,
    value: Option<f64>,
}

#[derive(Default)]
struct WeekValue {
    first_date: Option<NaiveDate>,
    acquisitions: f64,
    dispositions: f64,
    acquisition_count: usize,
    disposition_count: usize,
}

pub(super) fn render(
    frame: &mut Frame,
    area: Rect,
    transactions: &[InsiderTransaction],
    selected: usize,
) {
    let summary = summary_line(transactions);
    let block = terminal_block("OWN", "FORM 4 VALUE · LOADED SAMPLE")
        .title_bottom(summary)
        .border_style(AMBER);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let marks = marks(transactions);
    if marks.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::styled(
                "NO DATED FORM 4 TRANSACTIONS IN THE LOADED SAMPLE",
                MUTED,
            )),
            inner,
        );
        return;
    }
    let priced = marks
        .iter()
        .filter_map(|mark| mark.value)
        .collect::<Vec<_>>();
    if priced.is_empty() || inner.height < 4 || inner.width < 18 {
        frame.render_widget(
            Paragraph::new(Line::styled(
                "DATED ACTIVITY LOADED · NO REPORTED PRICE-DERIVED VALUES",
                MUTED,
            )),
            inner,
        );
        return;
    }
    let (low_exp, high_exp) = log_domain(&priced);
    let Some(geometry) = chart_geometry(inner, low_exp, high_exp) else {
        return;
    };
    let first = marks.iter().map(|mark| mark.date).min().unwrap_or_default();
    let last = marks.iter().map(|mark| mark.date).max().unwrap_or(first);
    let span_days = (last - first).num_days().max(1) as f64;
    let column = |date: NaiveDate| {
        let offset = (date - first).num_days().max(0) as f64;
        geometry.plot.x
            + ((offset / span_days * f64::from(geometry.plot.width.saturating_sub(1))).round()
                as u16)
                .min(geometry.plot.width.saturating_sub(1))
    };
    let buffer = frame.buffer_mut();
    render_log_labels(buffer, geometry.plot, low_exp, high_exp);
    render_marks(
        buffer,
        geometry.plot,
        &marks,
        selected,
        low_exp,
        high_exp,
        &column,
        transactions,
    );
    if let Some(bars) = geometry.bars {
        render_weekly_bars(buffer, bars, transactions, &column);
    }
    render_axis(buffer, geometry.axis_y, geometry.plot, first, last);
}

pub(super) fn selected_at_column(
    area: Rect,
    transactions: &[InsiderTransaction],
    column: u16,
) -> Option<usize> {
    let marks = marks(transactions);
    let priced = marks
        .iter()
        .filter_map(|mark| mark.value)
        .collect::<Vec<_>>();
    let (low_exp, high_exp) = log_domain(&priced);
    let inner = Rect {
        x: area.x.saturating_add(1),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    };
    let geometry = chart_geometry(inner, low_exp, high_exp)?;
    if column < geometry.plot.x || column >= geometry.plot.x.saturating_add(geometry.plot.width) {
        return None;
    }
    let first = marks.iter().map(|mark| mark.date).min()?;
    let last = marks.iter().map(|mark| mark.date).max().unwrap_or(first);
    let span_days = (last - first).num_days().max(1) as f64;
    marks
        .iter()
        .map(|mark| {
            let offset = (mark.date - first).num_days().max(0) as f64;
            let mark_column = geometry.plot.x
                + ((offset / span_days * f64::from(geometry.plot.width.saturating_sub(1))).round()
                    as u16)
                    .min(geometry.plot.width.saturating_sub(1));
            (mark.index, mark_column.abs_diff(column))
        })
        .min_by_key(|(_, distance)| *distance)
        .map(|(index, _)| index)
}

struct ChartGeometry {
    plot: Rect,
    bars: Option<Rect>,
    axis_y: u16,
}

fn chart_geometry(inner: Rect, low_exp: i32, high_exp: i32) -> Option<ChartGeometry> {
    let gutter = [decade_label(low_exp), decade_label(high_exp)]
        .into_iter()
        .map(|label| label.chars().count() as u16)
        .max()
        .unwrap_or(0)
        .saturating_add(1);
    if inner.width <= gutter.saturating_add(4) || inner.height < 4 {
        return None;
    }
    let bars_height = if inner.height > SCATTER_ROWS + BAR_ROWS {
        BAR_ROWS
    } else {
        0
    };
    let scatter_height = SCATTER_ROWS.min(inner.height.saturating_sub(bars_height + 1));
    let plot = Rect {
        x: inner.x.saturating_add(gutter),
        y: inner.y,
        width: inner.width.saturating_sub(gutter),
        height: scatter_height,
    };
    let bars = (bars_height > 0).then_some(Rect {
        x: plot.x,
        y: plot.y.saturating_add(plot.height),
        width: plot.width,
        height: bars_height,
    });
    Some(ChartGeometry {
        plot,
        bars,
        axis_y: inner.y.saturating_add(inner.height.saturating_sub(1)),
    })
}

fn marks(transactions: &[InsiderTransaction]) -> Vec<Mark> {
    transactions
        .iter()
        .enumerate()
        .filter_map(|(index, transaction)| {
            let date = NaiveDate::parse_from_str(&transaction.transaction_date, "%Y-%m-%d").ok()?;
            Some(Mark {
                index,
                date,
                value: transaction
                    .value_usd
                    .filter(|value| value.is_finite() && *value > 0.0),
            })
        })
        .collect()
}

fn render_log_labels(buffer: &mut Buffer, plot: Rect, low_exp: i32, high_exp: i32) {
    let labels = [(high_exp, plot.y), (low_exp, plot.y + plot.height - 1)];
    for (exponent, row) in labels {
        let label = decade_label(exponent);
        let x = plot.x.saturating_sub(label.chars().count() as u16 + 1);
        buffer.set_string(x, row, label, Style::new().fg(MUTED));
    }
}

#[allow(clippy::too_many_arguments)]
fn render_marks(
    buffer: &mut Buffer,
    plot: Rect,
    marks: &[Mark],
    selected: usize,
    low_exp: i32,
    high_exp: i32,
    column: &dyn Fn(NaiveDate) -> u16,
    transactions: &[InsiderTransaction],
) {
    let mut occupied = vec![false; usize::from(plot.width) * usize::from(plot.height)];
    let (chosen, rest): (Vec<&Mark>, Vec<&Mark>) =
        marks.iter().partition(|mark| mark.index == selected);
    for mark in rest.into_iter().chain(chosen) {
        let Some(value) = mark.value else {
            continue;
        };
        let x = column(mark.date);
        let ideal = value_row(value, low_exp, high_exp, plot.height);
        let row = nudge(
            &occupied,
            plot.width,
            x.saturating_sub(plot.x),
            ideal,
            plot.height,
        );
        occupied
            [usize::from(row) * usize::from(plot.width) + usize::from(x.saturating_sub(plot.x))] =
            true;
        let transaction = &transactions[mark.index];
        let (glyph, color) = match transaction.acquisition_disposition.as_str() {
            "ACQ" => ('▲', GREEN),
            "DISP" => ('▼', RED),
            _ => ('·', MUTED),
        };
        let mut style = Style::new().fg(color);
        if transaction.plan_10b5_1 {
            style = style.add_modifier(Modifier::DIM);
        }
        if mark.index == selected {
            style = style.bg(CYAN).fg(BG).bold();
        }
        if let Some(cell) = buffer.cell_mut((x, plot.y.saturating_add(row))) {
            cell.set_char(glyph).set_style(style);
        }
    }
}

fn render_weekly_bars(
    buffer: &mut Buffer,
    area: Rect,
    transactions: &[InsiderTransaction],
    column: &dyn Fn(NaiveDate) -> u16,
) {
    let mut weeks = BTreeMap::<(i32, u32), WeekValue>::new();
    for transaction in transactions {
        let Ok(date) = NaiveDate::parse_from_str(&transaction.transaction_date, "%Y-%m-%d") else {
            continue;
        };
        let week = date.iso_week();
        let value = transaction.value_usd.unwrap_or_default().max(0.0);
        let entry = weeks.entry((week.year(), week.week())).or_default();
        entry.first_date = Some(entry.first_date.map_or(date, |current| current.min(date)));
        match transaction.acquisition_disposition.as_str() {
            "ACQ" => {
                entry.acquisitions += value;
                entry.acquisition_count += 1;
            }
            "DISP" => {
                entry.dispositions += value;
                entry.disposition_count += 1;
            }
            _ => {}
        }
    }
    let maximum = weeks.values().fold(0.0_f64, |current, week| {
        current.max(week.acquisitions).max(week.dispositions)
    });
    let half = area.height / 2;
    for week in weeks.values() {
        let Some(date) = week.first_date else {
            continue;
        };
        let x = column(date);
        draw_vertical_bar(
            buffer,
            x,
            area.y,
            half,
            filled_rows(week.acquisitions, week.acquisition_count, maximum, half),
            GREEN,
            true,
        );
        draw_vertical_bar(
            buffer,
            x,
            area.y.saturating_add(half),
            area.height.saturating_sub(half),
            filled_rows(
                week.dispositions,
                week.disposition_count,
                maximum,
                area.height.saturating_sub(half),
            ),
            RED,
            false,
        );
    }
}

fn filled_rows(value: f64, count: usize, maximum: f64, height: u16) -> u16 {
    if count == 0 || height == 0 {
        return 0;
    }
    if maximum <= 0.0 {
        return 1;
    }
    ((value / maximum * f64::from(height)).round() as u16).clamp(1, height)
}

#[allow(clippy::too_many_arguments)]
fn draw_vertical_bar(
    buffer: &mut Buffer,
    x: u16,
    y: u16,
    height: u16,
    filled: u16,
    color: ratatui::style::Color,
    upward: bool,
) {
    for offset in 0..filled {
        let row = if upward {
            y.saturating_add(height.saturating_sub(offset + 1))
        } else {
            y.saturating_add(offset)
        };
        if let Some(cell) = buffer.cell_mut((x, row)) {
            cell.set_char('█').set_fg(color);
        }
    }
}

fn render_axis(buffer: &mut Buffer, row: u16, plot: Rect, first: NaiveDate, last: NaiveDate) {
    let first_label = first.format("%b %d").to_string();
    let last_label = last.format("%b %d").to_string();
    buffer.set_string(plot.x, row, &first_label, Style::new().fg(MUTED));
    let last_x = plot
        .x
        .saturating_add(plot.width)
        .saturating_sub(last_label.chars().count() as u16);
    if last_x
        > plot
            .x
            .saturating_add(first_label.chars().count() as u16 + 1)
    {
        buffer.set_string(last_x, row, &last_label, Style::new().fg(MUTED));
    }
}

fn summary_line(transactions: &[InsiderTransaction]) -> Line<'static> {
    let mut acquisitions = 0.0;
    let mut dispositions = 0.0;
    let mut plans = 0;
    for transaction in transactions {
        let value = transaction.value_usd.unwrap_or_default().max(0.0);
        match transaction.acquisition_disposition.as_str() {
            "ACQ" => acquisitions += value,
            "DISP" => dispositions += value,
            _ => {}
        }
        plans += usize::from(transaction.plan_10b5_1);
    }
    let plan_percent = if transactions.is_empty() {
        0
    } else {
        (plans * 100 + transactions.len() / 2) / transactions.len()
    };
    Line::from(vec![
        Span::styled(format!(" ▲ {} ", compact_usd(acquisitions)), GREEN),
        Span::styled(format!("▼ {} ", compact_usd(dispositions)), RED),
        Span::styled(
            format!(
                "· {} TXNS · {plan_percent}% 10b5-1 · LOADED SAMPLE ",
                transactions.len()
            ),
            MUTED,
        ),
    ])
}

fn compact_usd(value: f64) -> String {
    let (value, suffix) = if value >= 1_000_000_000.0 {
        (value / 1_000_000_000.0, "B")
    } else if value >= 1_000_000.0 {
        (value / 1_000_000.0, "M")
    } else if value >= 1_000.0 {
        (value / 1_000.0, "K")
    } else {
        return format!("${value:.0}");
    };
    format!("${value:.1}{suffix}")
}

fn log_domain(values: &[f64]) -> (i32, i32) {
    let (mut low, mut high) = (f64::INFINITY, f64::NEG_INFINITY);
    for value in values {
        if value.is_finite() && *value > 0.0 {
            low = low.min(*value);
            high = high.max(*value);
        }
    }
    if !low.is_finite() {
        return (3, 7);
    }
    let low_exp = low.log10().floor() as i32;
    let mut high_exp = high.log10().ceil() as i32;
    if high_exp <= low_exp {
        high_exp = low_exp + 1;
    }
    (low_exp, high_exp)
}

fn decade_label(exponent: i32) -> String {
    if exponent >= 9 {
        format!("${:.0}B", 10f64.powi(exponent - 9))
    } else if exponent >= 6 {
        format!("${:.0}M", 10f64.powi(exponent - 6))
    } else if exponent >= 3 {
        format!("${:.0}K", 10f64.powi(exponent - 3))
    } else {
        format!("${:.0}", 10f64.powi(exponent))
    }
}

fn value_row(value: f64, low_exp: i32, high_exp: i32, rows: u16) -> u16 {
    if rows <= 1 {
        return 0;
    }
    let fraction = (value.max(1.0).log10() - f64::from(low_exp)) / f64::from(high_exp - low_exp);
    ((1.0 - fraction.clamp(0.0, 1.0)) * f64::from(rows - 1)).round() as u16
}

fn nudge(occupied: &[bool], width: u16, x: u16, ideal: u16, rows: u16) -> u16 {
    let free = |row: u16| !occupied[usize::from(row) * usize::from(width) + usize::from(x)];
    if free(ideal) {
        return ideal;
    }
    for distance in 1..rows {
        if ideal >= distance && free(ideal - distance) {
            return ideal - distance;
        }
        if ideal + distance < rows && free(ideal + distance) {
            return ideal + distance;
        }
    }
    ideal
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    fn transaction(date: &str, side: &str, value: f64) -> InsiderTransaction {
        InsiderTransaction {
            filed: date.to_owned(),
            transaction_date: date.to_owned(),
            owner: "TEST INSIDER".to_owned(),
            role: "DIRECTOR".to_owned(),
            transaction_code: "S".to_owned(),
            acquisition_disposition: side.to_owned(),
            shares: 100.0,
            price_per_share: Some(value / 100.0),
            value_usd: Some(value),
            shares_after: Some(900.0),
            ownership_nature: "DIRECT".to_owned(),
            plan_10b5_1: side == "DISP",
            accession: date.to_owned(),
            document_url: None,
        }
    }

    #[test]
    fn log_rows_preserve_value_order_and_flat_domains() {
        assert_eq!(log_domain(&[1_000.0, 1_000.0]), (3, 4));
        let (low, high) = log_domain(&[1_000.0, 10_000_000.0]);
        assert!(value_row(10_000_000.0, low, high, 5) < value_row(1_000.0, low, high, 5));
    }

    #[test]
    fn collisions_nudge_to_the_nearest_free_row() {
        let mut occupied = vec![false; 10];
        occupied[2] = true;
        assert_eq!(nudge(&occupied, 2, 0, 1, 5), 0);
        occupied[0] = true;
        assert_eq!(nudge(&occupied, 2, 0, 1, 5), 2);
    }

    #[test]
    fn chart_renders_raw_sides_rollup_and_loaded_sample_boundary() {
        let transactions = vec![
            transaction("2026-06-01", "ACQ", 1_000_000.0),
            transaction("2026-07-28", "DISP", 10_000_000.0),
        ];
        let mut terminal = Terminal::new(TestBackend::new(80, 18)).unwrap();

        terminal
            .draw(|frame| render(frame, frame.area(), &transactions, 1))
            .unwrap();

        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("LOADED SAMPLE"));
        assert!(rendered.contains('▲'));
        assert!(rendered.contains('▼'));
        assert!(rendered.contains("50% 10b5-1"));
    }
}
