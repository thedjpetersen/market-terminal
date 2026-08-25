use ratatui::{
    layout::{Constraint, Rect},
    style::{Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table},
    Frame,
};

use crate::ui::theme::{self, AMBER, BG, INK};

pub fn terminal_block(code: &'static str, title: &'static str) -> Block<'static> {
    Block::new()
        .borders(Borders::ALL)
        .border_style(AMBER)
        .title(Line::from(vec![
            Span::styled(
                format!(" {code} "),
                Style::new().bg(AMBER).fg(BG).bold(),
            ),
            Span::styled(format!(" {title} "), AMBER),
        ]))
}

pub fn render_pairs<const N: usize>(
    frame: &mut Frame,
    area: Rect,
    code: &'static str,
    title: &'static str,
    data: &[[&'static str; N]],
) {
    let lines = data
        .iter()
        .map(|row| {
            let spans = row
                .iter()
                .enumerate()
                .map(|(index, value)| {
                    let is_value = index + 1 == N;
                    Span::styled(
                        format!("{:<width$}", value, width = if is_value { 1 } else { 22 }),
                        if is_value { theme::value(value) } else { Style::new().fg(INK) },
                    )
                })
                .collect::<Vec<_>>();
            Line::from(spans)
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines).block(terminal_block(code, title)), area);
}

pub fn render_table<const N: usize>(
    frame: &mut Frame,
    area: Rect,
    code: &'static str,
    title: &'static str,
    header: [&'static str; N],
    rows: Vec<Row<'static>>,
    widths: [Constraint; N],
) {
    let table = Table::new(rows, widths)
        .header(
            Row::new(header)
                .style(Style::new().fg(AMBER).add_modifier(Modifier::BOLD))
                .bottom_margin(1),
        )
        .column_spacing(1)
        .block(terminal_block(code, title));
    frame.render_widget(table, area);
}

pub fn styled_row<const N: usize>(values: [&'static str; N]) -> Row<'static> {
    Row::new(
        values
            .into_iter()
            .map(|value| Cell::from(value).style(theme::value(value))),
    )
}
