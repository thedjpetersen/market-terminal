use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, MouseEvent};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    symbols,
    text::{Line, Span},
    widgets::{Axis, Chart, Dataset, GraphType, Paragraph},
    Frame,
};

use crate::{
    app::{AppIntent, Workspace, WorkspaceDescriptor},
    ui::{
        components::{render_pairs, render_table, styled_row, terminal_block},
        table_row_at,
        theme::{AMBER, CYAN, INK, MUTED, YELLOW},
    },
};

use super::{LiveMarketsSnapshot, MarketIndex, MarketsQuery, MarketsSnapshot, ID};

pub struct MarketsWorkspace {
    query: Arc<dyn MarketsQuery>,
    pending_intents: Vec<AppIntent>,
}

impl MarketsWorkspace {
    pub fn new(query: Arc<dyn MarketsQuery>) -> Self {
        Self {
            query,
            pending_intents: Vec::new(),
        }
    }
}

impl Workspace for MarketsWorkspace {
    fn descriptor(&self) -> WorkspaceDescriptor {
        WorkspaceDescriptor {
            id: ID,
            label: "MARKETS",
            hotkey: 'm',
            commands: &["MARKET", "WEI", "CURVE"],
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::F(9) | KeyCode::Char('r' | 'R') => {
                self.query.request_refresh();
                true
            }
            _ => false,
        }
    }

    fn handle_mouse(&mut self, event: MouseEvent, area: Rect) -> bool {
        let (rows_area, symbols) = match self.query.load_markets() {
            MarketsSnapshot::Gallery { indices, .. } => {
                let columns =
                    Layout::horizontal([Constraint::Percentage(66), Constraint::Percentage(34)])
                        .split(area);
                let left = Layout::vertical([
                    Constraint::Percentage(48),
                    Constraint::Percentage(22),
                    Constraint::Percentage(30),
                ])
                .split(columns[0]);
                (
                    left[0],
                    indices
                        .iter()
                        .map(|index| index.symbol.to_owned())
                        .collect::<Vec<_>>(),
                )
            }
            MarketsSnapshot::Live(snapshot) => {
                let areas = live_areas(area);
                (
                    areas[1],
                    snapshot
                        .rows
                        .iter()
                        .map(|row| row.symbol.clone())
                        .collect::<Vec<_>>(),
                )
            }
        };
        let Some(index) = table_row_at(event, rows_area, symbols.len()) else {
            return false;
        };
        self.pending_intents.push(AppIntent::DispatchCommand {
            command: format!("CHART {}", symbols[index]),
            origin: ID,
        });
        true
    }

    fn poll_intents(&mut self) -> Vec<AppIntent> {
        std::mem::take(&mut self.pending_intents)
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        match self.query.load_markets() {
            MarketsSnapshot::Gallery {
                indices,
                treasury_curve,
            } => render_gallery(frame, area, indices, treasury_curve),
            MarketsSnapshot::Live(snapshot) => render_live(frame, area, &snapshot),
        }
    }
}

fn live_areas(area: Rect) -> std::rc::Rc<[Rect]> {
    Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(12),
        Constraint::Length(11),
        Constraint::Length(1),
    ])
    .split(area)
}

fn render_live(frame: &mut Frame, area: Rect, snapshot: &LiveMarketsSnapshot) {
    let areas = live_areas(area);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled(" EXTERNAL SNAPSHOTS ", CYAN),
                Span::styled("LISTED INSTRUMENTS FROM YOUR SELECTED PROVIDER", INK),
            ]),
            Line::styled(&snapshot.status, MUTED),
        ]),
        areas[0],
    );

    if snapshot.rows.is_empty() {
        frame.render_widget(
            Paragraph::new(vec![
                Line::styled("NO USABLE MARKET SNAPSHOTS", AMBER),
                Line::raw(""),
                Line::styled(
                    "Check SETTINGS for provider credentials and symbols, then press F9.",
                    MUTED,
                ),
            ])
            .block(terminal_block("MKT", "PROVIDER SNAPSHOTS")),
            areas[1],
        );
    } else {
        render_table(
            frame,
            areas[1],
            "MKT",
            "PROVIDER SNAPSHOTS · CLICK A ROW FOR CHART",
            [
                "SYMBOL", "LAST", "NET CHG", "% CHG", "QUALITY", "AS OF", "PROVIDER",
            ],
            snapshot
                .rows
                .iter()
                .map(|row| {
                    styled_row([
                        row.symbol.clone(),
                        row.last.clone(),
                        row.net_change.clone(),
                        row.percent_change.clone(),
                        row.quality.clone(),
                        row.as_of.clone(),
                        row.provider.clone(),
                    ])
                })
                .collect(),
            [
                Constraint::Percentage(10),
                Constraint::Percentage(12),
                Constraint::Percentage(11),
                Constraint::Percentage(10),
                Constraint::Percentage(13),
                Constraint::Percentage(26),
                Constraint::Percentage(18),
            ],
        );
    }

    let limitations = Layout::horizontal([Constraint::Ratio(1, 2); 2]).split(areas[2]);
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled("UNAVAILABLE FROM THE CURRENT QUOTE PORT", AMBER),
            Line::raw(""),
            Line::styled("• sovereign yield curves and rates", MUTED),
            Line::styled("• currencies and commodity futures", MUTED),
            Line::styled("• exchange breadth and new highs/lows", MUTED),
        ])
        .block(terminal_block("BOUND", "CROSS-ASSET DATA")),
        limitations[0],
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled("NO SUBSTITUTE OR PROXY VALUES", AMBER),
            Line::raw(""),
            Line::styled("• sector constituent aggregation", MUTED),
            Line::styled("• official economic calendars", MUTED),
            Line::styled("• market status/session calendars", MUTED),
        ])
        .block(terminal_block("BOUND", "ANALYTICS DATA")),
        limitations[1],
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" F9/R ", AMBER),
            Span::styled("REFRESH SNAPSHOTS   ", INK),
            Span::styled("SETTINGS ", AMBER),
            Span::styled("PROVIDER STATUS   ", INK),
            Span::styled("MON ", AMBER),
            Span::styled("WATCHLIST", INK),
        ])),
        areas[3],
    );
}

fn render_gallery(
    frame: &mut Frame,
    area: Rect,
    indices: &'static [MarketIndex],
    treasury_curve: &'static [(f64, f64)],
) {
    let columns =
        Layout::horizontal([Constraint::Percentage(66), Constraint::Percentage(34)]).split(area);
    let left = Layout::vertical([
        Constraint::Percentage(48),
        Constraint::Percentage(22),
        Constraint::Percentage(30),
    ])
    .split(columns[0]);
    let market_rows = indices
        .iter()
        .map(|index| {
            styled_row([
                index.name,
                index.symbol,
                index.last,
                index.net_change,
                index.percent_change,
            ])
        })
        .collect::<Vec<_>>();
    render_table(
        frame,
        left[0],
        "WEI",
        "WORLD EQUITY INDICES",
        ["INDEX", "SYMBOL", "LAST", "NET CHG", "% CHG"],
        market_rows,
        [
            Constraint::Percentage(27),
            Constraint::Percentage(14),
            Constraint::Percentage(23),
            Constraint::Percentage(19),
            Constraint::Percentage(17),
        ],
    );
    render_pairs(
        frame,
        left[1],
        "XAM",
        "CROSS-ASSET MONITOR",
        &[
            ["US 10Y", "4.312  +3.2BP"],
            ["DXY", "104.72  −0.18%"],
            ["EUR/USD", "1.0837  +0.21%"],
            ["WTI", "78.42  +1.14%"],
            ["GOLD", "2,337.80  −0.36%"],
        ],
    );
    let curve = Chart::new(vec![Dataset::default()
        .marker(symbols::Marker::Braille)
        .graph_type(GraphType::Line)
        .style(YELLOW)
        .data(treasury_curve)])
    .block(terminal_block("GC", "U.S. TREASURY CURVE"))
    .x_axis(
        Axis::default()
            .bounds([0., 100.])
            .labels(["3M", "5Y", "10Y", "30Y"])
            .style(MUTED),
    )
    .y_axis(
        Axis::default()
            .bounds([4.2, 5.5])
            .labels(["4.2", "4.8", "5.5"])
            .style(AMBER),
    );
    frame.render_widget(curve, left[2]);

    let right = Layout::vertical([
        Constraint::Percentage(45),
        Constraint::Percentage(27),
        Constraint::Percentage(28),
    ])
    .split(columns[1]);
    render_pairs(
        frame,
        right[0],
        "IMAP",
        "SECTOR PERFORMANCE",
        &[
            ["TECHNOLOGY", "+1.56%"],
            ["COMMUNICATION", "+1.11%"],
            ["CONS. DISC.", "+0.69%"],
            ["FINANCIALS", "+0.42%"],
            ["HEALTH CARE", "−0.15%"],
            ["UTILITIES", "−0.67%"],
            ["ENERGY", "−1.21%"],
        ],
    );
    render_pairs(
        frame,
        right[1],
        "MBR",
        "MARKET BREADTH",
        &[
            ["NYSE ADV / DEC", "2,181 / 812"],
            ["NEW HIGHS / LOWS", "224 / 31"],
            ["UP / DOWN VOLUME", "4.7X"],
            ["ABOVE 200 DMA", "62.8%"],
        ],
    );
    render_pairs(
        frame,
        right[2],
        "ECO",
        "ECONOMIC CALENDAR",
        &[
            ["08:30", "US INITIAL CLAIMS"],
            ["10:00", "US EXISTING HOMES"],
            ["14:00", "FED BEIGE BOOK"],
        ],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyModifiers, MouseButton, MouseEventKind};
    use ratatui::{backend::TestBackend, Terminal};

    struct LiveQuery;

    impl MarketsQuery for LiveQuery {
        fn load_markets(&self) -> MarketsSnapshot {
            MarketsSnapshot::Live(LiveMarketsSnapshot {
                rows: vec![super::super::LiveMarketRow {
                    symbol: "USER".to_owned(),
                    last: "123.45".to_owned(),
                    net_change: "+1.25".to_owned(),
                    percent_change: "+1.02%".to_owned(),
                    quality: "REALTIME".to_owned(),
                    as_of: "2026-08-26T19:00:00Z".to_owned(),
                    provider: "TEST FEED".to_owned(),
                }],
                status: "TEST FEED · 1/1 SNAPSHOT(S)".to_owned(),
            })
        }
    }

    fn click(column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    #[test]
    fn live_markets_render_provider_rows_without_gallery_cross_asset_values() {
        let workspace = MarketsWorkspace::new(Arc::new(LiveQuery));
        let mut terminal = Terminal::new(TestBackend::new(160, 48)).unwrap();

        terminal
            .draw(|frame| workspace.render(frame, frame.area()))
            .unwrap();

        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("USER"));
        assert!(rendered.contains("TEST FEED"));
        assert!(rendered.contains("NO SUBSTITUTE OR PROXY VALUES"));
        assert!(!rendered.contains("4.312"));
        assert!(!rendered.contains("NYSE ADV / DEC"));
    }

    #[test]
    fn clicking_a_live_market_row_opens_its_chart() {
        let area = Rect::new(0, 0, 160, 48);
        let rows = live_areas(area);
        let mut workspace = MarketsWorkspace::new(Arc::new(LiveQuery));

        assert!(workspace.handle_mouse(click(rows[1].x + 2, rows[1].y + 3), area));
        assert_eq!(
            workspace.poll_intents(),
            vec![AppIntent::DispatchCommand {
                command: "CHART USER".to_owned(),
                origin: ID,
            }]
        );
    }
}
