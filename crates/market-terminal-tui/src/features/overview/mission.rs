use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table},
    Frame,
};

use crate::ui::theme::{self, AMBER, BG, CYAN, GREEN, INK, MUTED, NAV_BG, RED, YELLOW};

use super::{LiveOverviewSnapshot, OverviewHealthState};

#[derive(Debug, Clone, Copy)]
pub(super) struct MissionAreas {
    pub header: Rect,
    pub pulse: Rect,
    pub kpis: Rect,
    pub priorities: Rect,
    pub positions: Rect,
    pub events: Rect,
    pub health: Rect,
    pub saved: Rect,
    pub footer: Rect,
}

pub(super) fn mission_areas(area: Rect) -> MissionAreas {
    let compact = area.height < 30;
    let rows = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(if compact { 3 } else { 4 }),
        Constraint::Length(if compact { 3 } else { 4 }),
        Constraint::Min(5),
        Constraint::Length(1),
    ])
    .split(area);
    if area.width >= 110 {
        let columns = Layout::horizontal([Constraint::Percentage(54), Constraint::Percentage(46)])
            .split(rows[3]);
        let left = Layout::vertical([Constraint::Percentage(54), Constraint::Percentage(46)])
            .split(columns[0]);
        let right = Layout::vertical([
            Constraint::Percentage(36),
            Constraint::Percentage(34),
            Constraint::Percentage(30),
        ])
        .split(columns[1]);
        MissionAreas {
            header: rows[0],
            pulse: rows[1],
            kpis: rows[2],
            priorities: left[0],
            positions: left[1],
            events: right[0],
            health: right[1],
            saved: right[2],
            footer: rows[4],
        }
    } else {
        let main = Layout::vertical([
            Constraint::Ratio(3, 10),
            Constraint::Ratio(2, 10),
            Constraint::Ratio(2, 10),
            Constraint::Ratio(2, 10),
            Constraint::Ratio(1, 10),
        ])
        .split(rows[3]);
        MissionAreas {
            header: rows[0],
            pulse: rows[1],
            kpis: rows[2],
            priorities: main[0],
            positions: main[1],
            events: main[2],
            health: main[3],
            saved: main[4],
            footer: rows[4],
        }
    }
}

pub(super) fn card_row_area(area: Rect, index: usize) -> Option<Rect> {
    let y = area.y.saturating_add(2).saturating_add(index as u16);
    (area.width > 2 && y < area.bottom())
        .then(|| Rect::new(area.x.saturating_add(1), y, area.width.saturating_sub(2), 1))
}

pub(super) fn visible_card_rows(area: Rect, rows: usize) -> usize {
    usize::from(area.height.saturating_sub(2)).min(rows)
}

pub(super) fn render_mission(
    frame: &mut Frame,
    area: Rect,
    snapshot: &LiveOverviewSnapshot,
    footer: Line<'static>,
) {
    let areas = mission_areas(area);
    render_header(frame, areas.header, snapshot);
    render_pulse(frame, areas.pulse, snapshot);
    render_kpis(frame, areas.kpis, snapshot);
    render_priorities(frame, areas.priorities, snapshot);
    render_positions(frame, areas.positions, snapshot);
    render_events(frame, areas.events, snapshot);
    render_health(frame, areas.health, snapshot);
    render_saved(frame, areas.saved, snapshot);
    frame.render_widget(
        Paragraph::new(footer).style(Style::new().bg(NAV_BG.into())),
        areas.footer,
    );
}

fn render_header(frame: &mut Frame, area: Rect, snapshot: &LiveOverviewSnapshot) {
    let urgent = snapshot
        .priorities
        .iter()
        .filter(|item| item.score >= 70)
        .count();
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled(
                    " MISSION CONTROL ",
                    Style::new().bg(CYAN.into()).fg(BG.into()).bold(),
                ),
                Span::styled(format!(" {} ", snapshot.portfolio_source), INK),
                Span::styled("AS OF ", AMBER),
                Span::styled(&snapshot.portfolio_as_of, MUTED),
            ]),
            Line::from(vec![
                Span::styled(
                    format!(" {urgent} HIGH-PRIORITY "),
                    if urgent > 0 { RED } else { GREEN },
                ),
                Span::styled(
                    "Scores are deterministic · select any row to inspect its owning context",
                    MUTED,
                ),
            ]),
        ]),
        area,
    );
}

fn render_pulse(frame: &mut Frame, area: Rect, snapshot: &LiveOverviewSnapshot) {
    let block = mission_block("Live market pulse · exact provider snapshots");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if snapshot.market_pulse.is_empty() {
        let detail = snapshot
            .source_health
            .iter()
            .find(|health| health.source == "MARKETS")
            .map(|health| health.detail.as_str())
            .unwrap_or("NO MARKET SOURCE ATTACHED");
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("UNAVAILABLE · ", AMBER),
                Span::styled(detail, MUTED),
            ])),
            inner,
        );
        return;
    }
    let mut values = Vec::new();
    for row in snapshot.market_pulse.iter().take(6) {
        values.extend([
            Span::styled(
                format!(" {} ", row.symbol),
                Style::new().fg(CYAN.into()).bold(),
            ),
            Span::styled(format!("{} ", row.last), INK),
            Span::styled(
                format!("{}  ", row.percent_change),
                theme::value(&row.percent_change),
            ),
        ]);
    }
    let provenance = snapshot.market_pulse.first().map(|row| {
        Line::from(vec![
            Span::styled(format!("{} · {} · ", row.provider, row.quality), MUTED),
            Span::styled(&row.as_of, AMBER),
        ])
    });
    let mut lines = vec![Line::from(values)];
    if let Some(provenance) = provenance {
        lines.push(provenance);
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_kpis(frame: &mut Frame, area: Rect, snapshot: &LiveOverviewSnapshot) {
    let columns = Layout::horizontal([Constraint::Ratio(1, 4); 4]).split(area);
    for (index, (label, value)) in [
        ("NET ASSET VALUE", snapshot.net_asset_value.as_str()),
        ("YTD RETURN", snapshot.ytd_return.as_str()),
        ("AVAILABLE CASH", snapshot.available_cash.as_str()),
        ("SHARPE", snapshot.sharpe.as_str()),
    ]
    .iter()
    .enumerate()
    {
        let lines = if area.height < 4 {
            vec![Line::from(vec![
                Span::styled(format!("{label} "), MUTED),
                Span::styled(
                    *value,
                    if index == 1 {
                        theme::value(value)
                    } else {
                        CYAN.into()
                    },
                ),
            ])]
        } else {
            vec![
                Line::styled(*label, MUTED),
                Line::styled(
                    *value,
                    if index == 1 {
                        theme::value(value)
                    } else {
                        CYAN.into()
                    },
                ),
            ]
        };
        frame.render_widget(
            Paragraph::new(lines)
                .block(Block::new().borders(Borders::ALL).border_style(AMBER))
                .alignment(Alignment::Center),
            columns[index],
        );
    }
}

fn render_priorities(frame: &mut Frame, area: Rect, snapshot: &LiveOverviewSnapshot) {
    if snapshot.priorities.is_empty() {
        render_empty(frame, area, "Ranked priorities", "NO UNRESOLVED PRIORITIES");
        return;
    }
    let rows = snapshot.priorities.iter().map(|item| {
        Row::new([
            Cell::from(format!("{:03}", item.score)).style(priority_style(item.score)),
            Cell::from(item.title.clone()).style(INK),
            Cell::from(item.reason.clone()).style(MUTED),
            Cell::from(item.source.clone()).style(AMBER),
        ])
    });
    frame.render_widget(
        Table::new(
            rows,
            [
                Constraint::Length(4),
                Constraint::Percentage(31),
                Constraint::Min(18),
                Constraint::Length(11),
            ],
        )
        .header(header_row(["SCORE", "ACTION", "WHY", "OWNER"]))
        .column_spacing(1)
        .block(mission_block("Ranked priorities · score and rationale")),
        area,
    );
}

fn render_positions(frame: &mut Frame, area: Rect, snapshot: &LiveOverviewSnapshot) {
    if snapshot.holdings.is_empty() {
        render_empty(
            frame,
            area,
            "Portfolio summary",
            "NOT CONFIGURED · OPEN PORTFOLIO TO IMPORT",
        );
        return;
    }
    let rows = snapshot.holdings.iter().map(|holding| {
        Row::new([
            Cell::from(holding.symbol.clone()).style(Style::new().fg(CYAN.into()).bold()),
            Cell::from(holding.market_value.clone()),
            Cell::from(holding.pnl.clone()).style(theme::value(&holding.pnl)),
            Cell::from(holding.weight.clone()),
        ])
    });
    frame.render_widget(
        Table::new(
            rows,
            [
                Constraint::Percentage(28),
                Constraint::Percentage(27),
                Constraint::Percentage(22),
                Constraint::Percentage(23),
            ],
        )
        .header(header_row(["SYMBOL", "VALUE", "P&L", "WEIGHT"]))
        .column_spacing(1)
        .block(mission_block(
            "Portfolio summary · exact imported positions",
        )),
        area,
    );
}

fn render_events(frame: &mut Frame, area: Rect, snapshot: &LiveOverviewSnapshot) {
    let mut rows = snapshot
        .events
        .iter()
        .map(|event| {
            Row::new([
                event.time.clone(),
                event.region.clone(),
                event.importance.clone(),
                event.title.clone(),
            ])
        })
        .collect::<Vec<_>>();
    if rows.is_empty() {
        rows.push(Row::new([
            "—",
            "—",
            "N/A",
            "NO PROVIDER-BACKED CALENDAR EVENTS",
        ]));
    }
    for headline in snapshot.headlines.iter().take(2) {
        rows.push(Row::new([
            headline.time.clone(),
            headline.region.clone(),
            "NEWS".to_owned(),
            headline.title.clone(),
        ]));
    }
    frame.render_widget(
        Table::new(
            rows,
            [
                Constraint::Length(6),
                Constraint::Length(4),
                Constraint::Length(6),
                Constraint::Min(16),
            ],
        )
        .header(header_row(["TIME", "REG", "TYPE", "EVENT / HEADLINE"]))
        .column_spacing(1)
        .block(mission_block("Upcoming events & current news")),
        area,
    );
}

fn render_health(frame: &mut Frame, area: Rect, snapshot: &LiveOverviewSnapshot) {
    let rows = snapshot.source_health.iter().map(|health| {
        Row::new([
            Cell::from(health.source.clone()).style(INK),
            Cell::from(health.state.label()).style(health_style(health.state)),
            Cell::from(health.detail.clone()).style(MUTED),
            Cell::from(health.as_of.clone()).style(AMBER),
        ])
    });
    frame.render_widget(
        Table::new(
            rows,
            [
                Constraint::Length(11),
                Constraint::Length(14),
                Constraint::Min(15),
                Constraint::Length(13),
            ],
        )
        .header(header_row(["SOURCE", "STATE", "DETAIL", "AS OF"]))
        .column_spacing(1)
        .block(mission_block("Source health · no silent fixture fallback")),
        area,
    );
}

fn render_saved(frame: &mut Frame, area: Rect, snapshot: &LiveOverviewSnapshot) {
    if snapshot.saved_work.is_empty() {
        render_empty(frame, area, "Saved work", "NO SAVED TILES · OPEN LAUNCHPAD");
        return;
    }
    let rows = snapshot.saved_work.iter().map(|work| {
        Row::new([
            Cell::from(work.label.clone()).style(CYAN),
            Cell::from(work.kind.clone()).style(AMBER),
            Cell::from(work.command.clone()).style(MUTED),
        ])
    });
    frame.render_widget(
        Table::new(
            rows,
            [
                Constraint::Percentage(38),
                Constraint::Length(13),
                Constraint::Min(12),
            ],
        )
        .header(header_row(["SAVED", "TYPE", "COMMAND"]))
        .column_spacing(1)
        .block(mission_block("Saved work · startup snapshot")),
        area,
    );
}

fn render_empty(frame: &mut Frame, area: Rect, title: &'static str, message: &'static str) {
    frame.render_widget(
        Paragraph::new(Line::styled(message, MUTED)).block(mission_block(title)),
        area,
    );
}

fn mission_block(title: &'static str) -> Block<'static> {
    Block::new()
        .borders(Borders::ALL)
        .border_style(AMBER)
        .title(Span::styled(
            format!(" {title} "),
            Style::new().fg(AMBER.into()).add_modifier(Modifier::BOLD),
        ))
}

fn header_row<const N: usize>(labels: [&'static str; N]) -> Row<'static> {
    Row::new(labels)
        .style(Style::new().fg(INK.into()).bold())
        .bottom_margin(0)
}

fn priority_style(score: u16) -> Style {
    if score >= 70 {
        RED.into()
    } else if score >= 40 {
        YELLOW.into()
    } else {
        GREEN.into()
    }
}

fn health_style(state: OverviewHealthState) -> Style {
    match state {
        OverviewHealthState::Ready => GREEN.into(),
        OverviewHealthState::Partial | OverviewHealthState::Loading => YELLOW.into(),
        OverviewHealthState::Unavailable => RED.into(),
        OverviewHealthState::NotConfigured => MUTED.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_mission_region_stays_inside_supported_viewports() {
        for area in [
            Rect::new(0, 0, 80, 24),
            Rect::new(2, 3, 120, 36),
            Rect::new(0, 0, 160, 48),
        ] {
            let regions = mission_areas(area);
            for region in [
                regions.header,
                regions.pulse,
                regions.kpis,
                regions.priorities,
                regions.positions,
                regions.events,
                regions.health,
                regions.saved,
                regions.footer,
            ] {
                assert!(region.x >= area.x);
                assert!(region.y >= area.y);
                assert!(region.right() <= area.right());
                assert!(region.bottom() <= area.bottom());
            }
        }
    }
}
