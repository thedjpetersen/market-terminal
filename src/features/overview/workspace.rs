use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, MouseEvent};
use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{Axis, Block, Borders, Cell, Chart, Dataset, GraphType, Paragraph, Row, Table},
    Frame,
};

use crate::{
    app::{AppIntent, ShellChrome, Workspace, WorkspaceAction, WorkspaceDescriptor},
    ui::{
        contains, is_primary_click,
        theme::{self, AMBER, BG, CYAN, GREEN, INK, MUTED, NAV_BG, YELLOW},
    },
};

use super::{
    controls::{control_areas, gallery_areas, panel_header_area, period_areas, OverviewControl},
    mission::{card_row_area, mission_areas, render_mission, visible_card_rows},
    LiveOverviewSnapshot, OverviewHeadline, OverviewQuery, OverviewSnapshot, ID,
};

const METRIC_HIGHLIGHT: Color = Color::Rgb(77, 58, 10);

const COUNTRIES: [(&str, &str); 5] = [
    ("United States", "63.2%"),
    ("Japan", "5.2%"),
    ("Taiwan", "3.3%"),
    ("South Korea", "3.1%"),
    ("United Kingdom", "3.0%"),
];

const HOLDINGS: [(&str, &str); 10] = [
    ("NVIDIA Corporation", "4.8%"),
    ("Apple Inc.", "4.3%"),
    ("Microsoft Corporation", "2.6%"),
    ("Amazon.com Inc.", "2.3%"),
    ("Alphabet Inc. Class A", "2.0%"),
    ("Taiwan Semiconductor Manufacturing Co. Ltd.", "1.8%"),
    ("Broadcom Inc.", "1.8%"),
    ("Alphabet Inc. Class C", "1.7%"),
    ("Micron Technology Inc.", "1.3%"),
    ("Meta Platforms Inc Class A", "1.2%"),
];

const WINNERS: [(&str, &str); 5] = [
    ("Advantest Corp.", "+15.06%"),
    ("Kioxia Holdings Corporation", "+12.27%"),
    ("SoftBank Group Corp.", "+7.90%"),
    ("Disco Corporation", "+7.81%"),
    ("Tokyo Electron Ltd.", "+7.78%"),
];

const LOSERS: [(&str, &str); 5] = [
    ("MS&AD Insurance Group Holdings", "−4.45%"),
    ("Komatsu Ltd.", "−3.56%"),
    ("National Australia Bank Ltd.", "−3.35%"),
    ("Mitsubishi Heavy Industries", "−3.34%"),
    ("Mitsui & Co. Ltd.", "−3.26%"),
];

const HEADLINES: [&str; 5] = [
    "European stocks edge higher on oil, tech relief",
    "Moonpig shares jump 10% as FY26 profit beats estimates under new CEO",
    "Japan stocks higher at close of trade; Nikkei 225 up 4.69%",
    "Asia stocks rally as Micron outlook revives AI trade; Korea, Japan lead",
    "Why is Halfords stock surging today?",
];

pub struct OverviewWorkspace {
    query: Arc<dyn OverviewQuery>,
    selected_period: usize,
    pending_intents: Vec<AppIntent>,
}

impl OverviewWorkspace {
    pub fn new(query: Arc<dyn OverviewQuery>) -> Self {
        Self {
            query,
            selected_period: 3,
            pending_intents: Vec::new(),
        }
    }

    fn dispatch(&mut self, command: impl Into<String>) {
        self.pending_intents.push(AppIntent::DispatchCommand {
            command: command.into(),
            origin: ID,
        });
    }

    fn control_action_label(control: OverviewControl) -> &'static str {
        match control {
            OverviewControl::Portfolio => "Open the Portfolio workspace",
            OverviewControl::Risk => "Open portfolio Risk analytics",
            OverviewControl::News => "Open the live News workspace",
            OverviewControl::Refresh => "Refresh Overview news data",
        }
    }

    fn activate_control(&mut self, control: OverviewControl) -> bool {
        match control {
            OverviewControl::Portfolio => self.dispatch("PORT"),
            OverviewControl::Risk => self.dispatch("RISK"),
            OverviewControl::News => self.dispatch("NEWS"),
            OverviewControl::Refresh => self.query.request_refresh(),
        }
        true
    }

    fn footer_actions(&self, area: Rect) -> Vec<WorkspaceAction> {
        control_areas(area)
            .into_iter()
            .map(|(control, area)| {
                WorkspaceAction::new(
                    control.action_id(),
                    Self::control_action_label(control),
                    area,
                )
            })
            .collect()
    }

    fn gallery_actions(&self, area: Rect, periods: &[&str]) -> Vec<WorkspaceAction> {
        let areas = gallery_areas(area);
        let mut actions = period_areas(areas.header, periods)
            .into_iter()
            .map(|(index, period_area)| {
                let mut action = WorkspaceAction::new(
                    format!("period:{index}:{}", periods[index]),
                    format!("Show {} Overview period", periods[index]),
                    period_area,
                );
                if index == self.selected_period {
                    action = action.preferred();
                }
                action
            })
            .collect::<Vec<_>>();
        actions.extend([
            WorkspaceAction::new(
                "card:risk",
                "Open Risk from the risk summary",
                panel_header_area(areas.risk),
            ),
            WorkspaceAction::new(
                "card:portfolio",
                "Open Portfolio from the composition summary",
                panel_header_area(areas.composition),
            ),
            WorkspaceAction::new(
                "card:news",
                "Open News from the market briefing",
                panel_header_area(areas.news),
            ),
        ]);
        actions.extend(self.footer_actions(areas.footer));
        actions.retain(|action| action.area.width > 0 && action.area.height > 0);
        actions
    }

    fn live_actions(&self, area: Rect, snapshot: &LiveOverviewSnapshot) -> Vec<WorkspaceAction> {
        let areas = mission_areas(area);
        let mut actions = snapshot
            .priorities
            .iter()
            .take(visible_card_rows(
                areas.priorities,
                snapshot.priorities.len(),
            ))
            .enumerate()
            .filter_map(|(index, priority)| {
                let area = card_row_area(areas.priorities, index)?;
                let identity = text_identity(&format!(
                    "{}\0{}\0{}",
                    priority.id, priority.reason, priority.command
                ));
                Some(WorkspaceAction::new(
                    format!("priority:{index}:{identity:016x}"),
                    format!("Open ranked priority: {}", priority.title),
                    area,
                ))
            })
            .collect::<Vec<_>>();
        actions.extend(snapshot.market_pulse.iter().take(6).enumerate().filter_map(
            |(index, pulse)| {
                let x = areas.pulse.x.saturating_add(1 + index as u16 * 18);
                if x >= areas.pulse.right().saturating_sub(1) {
                    return None;
                }
                let area = Rect::new(
                    x,
                    areas.pulse.y.saturating_add(1),
                    18.min(areas.pulse.right().saturating_sub(1).saturating_sub(x)),
                    1,
                );
                Some(WorkspaceAction::new(
                    format!("market:{index}:{:016x}", text_identity(&pulse.symbol)),
                    format!("Open {} market security", pulse.symbol),
                    area,
                ))
            },
        ));
        actions.extend(
            snapshot
                .holdings
                .iter()
                .take(visible_card_rows(areas.positions, snapshot.holdings.len()))
                .enumerate()
                .filter_map(|(index, holding)| {
                    let area = card_row_area(areas.positions, index)?;
                    Some(WorkspaceAction::new(
                        format!("holding:{index}:{:016x}", text_identity(&holding.symbol)),
                        format!("Open {} security research", holding.symbol),
                        area,
                    ))
                }),
        );
        let visible_events = visible_card_rows(areas.events, snapshot.events.len());
        actions.extend(
            snapshot
                .events
                .iter()
                .take(visible_events)
                .enumerate()
                .filter_map(|(index, event)| {
                    let area = card_row_area(areas.events, index)?;
                    let identity = text_identity(&format!(
                        "{}\0{}\0{}",
                        event.time, event.region, event.title
                    ));
                    Some(WorkspaceAction::new(
                        format!("event:{index}:{identity:016x}"),
                        format!("Open calendar for {}", short_label(&event.title)),
                        area,
                    ))
                }),
        );
        let headline_offset = if snapshot.events.is_empty() {
            1
        } else {
            snapshot.events.len()
        };
        actions.extend(snapshot.headlines.iter().take(2).enumerate().filter_map(
            |(index, headline)| {
                let area = card_row_area(areas.events, headline_offset + index)?;
                Some(WorkspaceAction::new(
                    format!("headline:{index}:{:016x}", headline_identity(headline)),
                    format!("Open News for {}", short_label(&headline.title)),
                    area,
                ))
            },
        ));
        actions.extend(
            snapshot
                .source_health
                .iter()
                .take(visible_card_rows(
                    areas.health,
                    snapshot.source_health.len(),
                ))
                .enumerate()
                .filter_map(|(index, health)| {
                    let area = card_row_area(areas.health, index)?;
                    let identity = text_identity(&format!(
                        "{}\0{}\0{}",
                        health.source, health.detail, health.command
                    ));
                    Some(WorkspaceAction::new(
                        format!("health:{index}:{identity:016x}"),
                        format!("Inspect {} source", health.source),
                        area,
                    ))
                }),
        );
        actions.extend(
            snapshot
                .saved_work
                .iter()
                .take(visible_card_rows(areas.saved, snapshot.saved_work.len()))
                .enumerate()
                .filter_map(|(index, work)| {
                    let area = card_row_area(areas.saved, index)?;
                    let identity = text_identity(&format!("{}\0{}", work.label, work.command));
                    Some(WorkspaceAction::new(
                        format!("saved:{}:{identity:016x}", work.id),
                        format!("Open saved work: {}", work.label),
                        area,
                    ))
                }),
        );
        let mut footer = self.footer_actions(areas.footer);
        if let Some(action) = actions.first_mut().or_else(|| footer.first_mut()) {
            action.preferred = true;
        }
        actions.extend(footer);
        actions
    }

    fn render_context_header(&self, frame: &mut Frame, area: Rect, periods: &[&str]) {
        let rows = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).split(area);
        let mut period_spans = vec![Span::raw(" ")];
        for (index, period) in periods.iter().enumerate() {
            let style = if index == self.selected_period {
                Style::new().bg(CYAN.into()).fg(BG.into()).bold()
            } else {
                Style::new().fg(CYAN.into())
            };
            period_spans.push(Span::styled(format!(" {} {period} ", index + 1), style));
        }
        frame.render_widget(Paragraph::new(Line::from(period_spans)), rows[0]);

        let context = Layout::horizontal([
            Constraint::Percentage(29),
            Constraint::Percentage(46),
            Constraint::Percentage(25),
        ])
        .split(rows[1]);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" ● MARKET OPEN ", GREEN),
                Span::styled("(Regular session)", MUTED),
            ])),
            context[0],
        );
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("Inflation (YTD): ", INK),
                Span::styled("+0.0% (HICP, as of 2025-12)", AMBER),
            ])),
            context[1],
        );
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("S&P Futures ", INK),
                Span::styled("+0.81%", GREEN),
            ]))
            .alignment(Alignment::Right),
            context[2],
        );
    }

    fn render_returns_chart(
        &self,
        frame: &mut Frame,
        area: Rect,
        primary: &[(f64, f64)],
        comparison: &[(f64, f64)],
    ) {
        let primary_steps = stepped_points(primary);
        let comparison_steps = stepped_points(comparison);
        let zero_baseline = [(0.0, 0.0), (100.0, 0.0)];
        let datasets = vec![
            Dataset::default()
                .marker(symbols::Marker::Braille)
                .graph_type(GraphType::Line)
                .style(YELLOW)
                .data(&primary_steps),
            Dataset::default()
                .marker(symbols::Marker::Braille)
                .graph_type(GraphType::Line)
                .style(CYAN)
                .data(&comparison_steps),
            Dataset::default()
                .marker(symbols::Marker::Dot)
                .graph_type(GraphType::Line)
                .style(MUTED)
                .data(&zero_baseline),
        ];
        let chart = Chart::new(datasets)
            .block(reference_block("Returns — YTD (%)"))
            .x_axis(
                Axis::default()
                    .bounds([0.0, 100.0])
                    .labels(["02 Jan 26", "30 Mar 26", "25 Jun 26"])
                    .style(MUTED),
            )
            .y_axis(
                Axis::default()
                    .bounds([-3.0, 18.0])
                    .labels([
                        "−3.0", "−1.7", "−0.3", "1.0", "2.4", "3.7", "5.0", "6.4", "7.7", "9.1",
                        "10.4", "11.7", "13.1", "14.4", "15.8", "17.1",
                    ])
                    .style(AMBER),
            );
        frame.render_widget(chart, area);

        let legend = Line::from(vec![
            Span::styled(" ━ 001 ", Style::new().fg(YELLOW.into()).bold()),
            Span::styled("H +17.1% L −1.6%    ", MUTED),
            Span::styled("━ 002 ", Style::new().fg(CYAN.into()).bold()),
            Span::styled("H +14.3% L −3.0%", MUTED),
        ]);
        frame.render_widget(
            Paragraph::new(legend),
            Rect::new(
                area.x.saturating_add(2),
                area.y.saturating_add(1),
                area.width.saturating_sub(4),
                1,
            ),
        );
    }

    fn render_risk_band(&self, frame: &mut Frame, area: Rect) {
        let columns = Layout::horizontal([
            Constraint::Percentage(50),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
        ])
        .split(area);
        render_risk_table(frame, columns[0]);
        render_metric_pairs(
            frame,
            columns[1],
            "Asset returns — YTD",
            &[
                ("SPYY", "+13.97%"),
                ("IS3R", "+30.31%"),
                ("AVWS", "+22.05%"),
            ],
            Some(1),
        );
        render_metric_pairs(
            frame,
            columns[2],
            "Watchlist — YTD",
            &[
                ("AVWC", "+16.72%"),
                ("DEGC", "+13.15%"),
                ("DEGT", "+12.75%"),
            ],
            Some(0),
        );
    }

    fn render_composition(&self, frame: &mut Frame, area: Rect) {
        let block = reference_block("Current composition · MSCI ACWI (WEBN/SPYY)");
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let columns = Layout::horizontal([
            Constraint::Percentage(26),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(inner);
        render_countries(frame, columns[0]);
        render_holdings(frame, columns[2]);
    }

    fn render_news(&self, frame: &mut Frame, area: Rect) {
        let block = reference_block("News & movers — today");
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let columns = Layout::horizontal([
            Constraint::Percentage(23),
            Constraint::Length(1),
            Constraint::Percentage(23),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(inner);
        render_movers(frame, columns[0], "▲ Winners", &WINNERS);
        render_movers(frame, columns[2], "▼ Losers", &LOSERS);
        render_headlines(frame, columns[4]);
    }

    fn render_function_strip(&self, frame: &mut Frame, area: Rect) {
        let mut spans = control_areas(area)
            .into_iter()
            .map(|(control, _)| {
                let style = if control == OverviewControl::Refresh {
                    AMBER
                } else {
                    INK
                };
                Span::styled(control.text(), style)
            })
            .collect::<Vec<_>>();
        spans.extend([
            Span::styled(" / COMMAND ", INK),
            Span::styled(" Q QUIT ", INK),
        ]);
        frame.render_widget(
            Paragraph::new(Line::from(spans)).style(Style::new().bg(NAV_BG.into())),
            area,
        );
    }

    fn render_live(&self, frame: &mut Frame, area: Rect, snapshot: &LiveOverviewSnapshot) {
        let mut spans = control_areas(mission_areas(area).footer)
            .into_iter()
            .map(|(control, _)| {
                Span::styled(
                    control.text(),
                    if control == OverviewControl::Refresh {
                        AMBER
                    } else {
                        INK
                    },
                )
            })
            .collect::<Vec<_>>();
        spans.extend([
            Span::styled(" / COMMAND ", INK),
            Span::styled(" Q QUIT ", INK),
        ]);
        render_mission(frame, area, snapshot, Line::from(spans));
    }
}

impl Workspace for OverviewWorkspace {
    fn descriptor(&self) -> WorkspaceDescriptor {
        WorkspaceDescriptor {
            id: ID,
            label: "OVERVIEW",
            hotkey: 'g',
            commands: &["OVERVIEW", "HOME", "PERF"],
        }
    }

    fn shell_chrome(&self) -> ShellChrome {
        ShellChrome::Immersive
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char(character @ '1'..='8') => {
                let index = character as usize - '1' as usize;
                let OverviewSnapshot::Gallery { periods, .. } = self.query.load_overview() else {
                    return false;
                };
                if index >= periods.len() {
                    return false;
                }
                self.selected_period = index;
                true
            }
            KeyCode::Left => {
                if !matches!(self.query.load_overview(), OverviewSnapshot::Gallery { .. }) {
                    return false;
                }
                self.selected_period = self.selected_period.saturating_sub(1);
                true
            }
            KeyCode::Right => {
                let OverviewSnapshot::Gallery { periods, .. } = self.query.load_overview() else {
                    return false;
                };
                self.selected_period =
                    (self.selected_period + 1).min(periods.len().saturating_sub(1));
                true
            }
            KeyCode::F(9) | KeyCode::Char('r' | 'R') => {
                self.activate_control(OverviewControl::Refresh)
            }
            _ => false,
        }
    }

    fn handle_mouse(&mut self, event: MouseEvent, area: Rect) -> bool {
        if !is_primary_click(event, area) {
            return false;
        }
        for action in self.actions(area) {
            if contains(action.area, event.column, event.row) {
                return self.activate_action(&action.id);
            }
        }
        false
    }

    fn actions(&self, area: Rect) -> Vec<WorkspaceAction> {
        match self.query.load_overview() {
            OverviewSnapshot::Gallery { periods, .. } => self.gallery_actions(area, periods),
            OverviewSnapshot::Live(snapshot) => self.live_actions(area, &snapshot),
        }
    }

    fn activate_action(&mut self, id: &str) -> bool {
        if let Some(control) = OverviewControl::from_action_id(id) {
            return self.activate_control(control);
        }
        match id {
            "card:risk" => return self.activate_control(OverviewControl::Risk),
            "card:portfolio" => return self.activate_control(OverviewControl::Portfolio),
            "card:news" => return self.activate_control(OverviewControl::News),
            _ => {}
        }
        if let Some(period) = id.strip_prefix("period:") {
            let Some((index, expected_label)) = period.split_once(':') else {
                return false;
            };
            let Ok(index) = index.parse::<usize>() else {
                return false;
            };
            let OverviewSnapshot::Gallery { periods, .. } = self.query.load_overview() else {
                return false;
            };
            if periods.get(index).copied() != Some(expected_label) {
                return false;
            }
            self.selected_period = index;
            return true;
        }
        if let Some(priority) = id.strip_prefix("priority:") {
            let Some((index, expected_identity)) = parse_index_identity(priority) else {
                return false;
            };
            let OverviewSnapshot::Live(snapshot) = self.query.load_overview() else {
                return false;
            };
            let Some(priority) = snapshot.priorities.get(index) else {
                return false;
            };
            let identity = text_identity(&format!(
                "{}\0{}\0{}",
                priority.id, priority.reason, priority.command
            ));
            if identity != expected_identity {
                return false;
            }
            self.dispatch(priority.command.clone());
            return true;
        }
        if let Some(market) = id.strip_prefix("market:") {
            let Some((index, expected_identity)) = parse_index_identity(market) else {
                return false;
            };
            let OverviewSnapshot::Live(snapshot) = self.query.load_overview() else {
                return false;
            };
            let Some(market) = snapshot.market_pulse.get(index) else {
                return false;
            };
            if text_identity(&market.symbol) != expected_identity {
                return false;
            }
            self.dispatch(format!("SEC {} US", market.symbol));
            return true;
        }
        if let Some(event) = id.strip_prefix("event:") {
            let Some((index, expected_identity)) = parse_index_identity(event) else {
                return false;
            };
            let OverviewSnapshot::Live(snapshot) = self.query.load_overview() else {
                return false;
            };
            let Some(event) = snapshot.events.get(index) else {
                return false;
            };
            let identity = text_identity(&format!(
                "{}\0{}\0{}",
                event.time, event.region, event.title
            ));
            if identity != expected_identity {
                return false;
            }
            self.dispatch("NEWS CAL");
            return true;
        }
        if let Some(health) = id.strip_prefix("health:") {
            let Some((index, expected_identity)) = parse_index_identity(health) else {
                return false;
            };
            let OverviewSnapshot::Live(snapshot) = self.query.load_overview() else {
                return false;
            };
            let Some(health) = snapshot.source_health.get(index) else {
                return false;
            };
            let identity = text_identity(&format!(
                "{}\0{}\0{}",
                health.source, health.detail, health.command
            ));
            if identity != expected_identity {
                return false;
            }
            self.dispatch(health.command.clone());
            return true;
        }
        if let Some(saved) = id.strip_prefix("saved:") {
            let Some((saved_id, expected_identity)) = saved.split_once(':') else {
                return false;
            };
            let Ok(saved_id) = saved_id.parse::<u64>() else {
                return false;
            };
            let Some(expected_identity) = parse_identity(expected_identity) else {
                return false;
            };
            let OverviewSnapshot::Live(snapshot) = self.query.load_overview() else {
                return false;
            };
            let Some(saved) = snapshot.saved_work.iter().find(|work| work.id == saved_id) else {
                return false;
            };
            if text_identity(&format!("{}\0{}", saved.label, saved.command)) != expected_identity {
                return false;
            }
            self.dispatch(saved.command.clone());
            return true;
        }
        if let Some(holding) = id.strip_prefix("holding:") {
            let Some((index, expected_identity)) = holding.split_once(':') else {
                return false;
            };
            let Ok(index) = index.parse::<usize>() else {
                return false;
            };
            let Some(expected_identity) = parse_identity(expected_identity) else {
                return false;
            };
            let OverviewSnapshot::Live(snapshot) = self.query.load_overview() else {
                return false;
            };
            let Some(holding) = snapshot.holdings.get(index) else {
                return false;
            };
            if text_identity(&holding.symbol) != expected_identity {
                return false;
            }
            self.dispatch(format!("SEC {} US", holding.symbol));
            return true;
        }
        let Some(headline) = id.strip_prefix("headline:") else {
            return false;
        };
        let Some((index, expected_identity)) = headline.split_once(':') else {
            return false;
        };
        let Ok(index) = index.parse::<usize>() else {
            return false;
        };
        let Some(expected_identity) = parse_identity(expected_identity) else {
            return false;
        };
        let OverviewSnapshot::Live(snapshot) = self.query.load_overview() else {
            return false;
        };
        let Some(headline) = snapshot.headlines.get(index) else {
            return false;
        };
        if headline_identity(headline) != expected_identity {
            return false;
        }
        self.dispatch(format!("NEWS {}", headline.topic));
        true
    }

    fn poll_intents(&mut self) -> Vec<AppIntent> {
        std::mem::take(&mut self.pending_intents)
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let (periods, primary_returns, comparison_returns) = match self.query.load_overview() {
            OverviewSnapshot::Gallery {
                periods,
                primary_returns,
                comparison_returns,
            } => (periods, primary_returns, comparison_returns),
            OverviewSnapshot::Live(snapshot) => {
                self.render_live(frame, area, &snapshot);
                return;
            }
        };
        let areas = gallery_areas(area);
        self.render_context_header(frame, areas.header, periods);
        self.render_returns_chart(frame, areas.returns, primary_returns, comparison_returns);
        self.render_risk_band(frame, areas.risk);
        self.render_composition(frame, areas.composition);
        self.render_news(frame, areas.news);
        self.render_function_strip(frame, areas.footer);
    }
}

fn reference_block(title: &'static str) -> Block<'static> {
    Block::new()
        .borders(Borders::ALL)
        .border_style(AMBER)
        .title(Span::styled(
            format!(" {title} "),
            Style::new().fg(AMBER.into()).bold(),
        ))
}

fn text_identity(value: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn parse_index_identity(value: &str) -> Option<(usize, u64)> {
    let (index, identity) = value.split_once(':')?;
    Some((index.parse().ok()?, parse_identity(identity)?))
}

fn headline_identity(headline: &OverviewHeadline) -> u64 {
    let mut identity = String::with_capacity(
        headline.time.len()
            + headline.topic.len()
            + headline.title.len()
            + headline.region.len()
            + 3,
    );
    identity.push_str(&headline.time);
    identity.push('\0');
    identity.push_str(&headline.topic);
    identity.push('\0');
    identity.push_str(&headline.title);
    identity.push('\0');
    identity.push_str(&headline.region);
    text_identity(&identity)
}

fn parse_identity(identity: &str) -> Option<u64> {
    (identity.len() == 16)
        .then(|| u64::from_str_radix(identity, 16).ok())
        .flatten()
}

fn short_label(value: &str) -> String {
    const MAX_CHARS: usize = 44;
    let mut characters = value.chars();
    let mut label = characters.by_ref().take(MAX_CHARS).collect::<String>();
    if characters.next().is_some() {
        label.push('…');
    }
    label
}

fn stepped_points(points: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let Some(first) = points.first().copied() else {
        return Vec::new();
    };
    let mut stepped = Vec::with_capacity(points.len().saturating_mul(2).saturating_sub(1));
    stepped.push(first);
    for window in points.windows(2) {
        let previous = window[0];
        let current = window[1];
        stepped.push((current.0, previous.1));
        stepped.push(current);
    }
    stepped
}

fn render_risk_table(frame: &mut Frame, area: Rect) {
    let header = Row::new(["Portfolio", "Return", "Max DD", "Std dev (ann.)", "Sharpe"])
        .style(Style::new().fg(INK.into()))
        .bottom_margin(1);
    let rows = vec![
        Row::new([
            Cell::from("001").style(Style::new().fg(INK.into())),
            metric_cell("+17.02%", true),
            metric_cell("−6.3%", true),
            metric_cell("13.2%", false),
            metric_cell("2.79", true),
        ]),
        Row::new([
            Cell::from("002").style(Style::new().fg(INK.into())),
            metric_cell("+13.87%", false),
            metric_cell("−6.6%", false),
            metric_cell("12.8%", true),
            metric_cell("2.28", false),
        ]),
    ];
    let table = Table::new(
        rows,
        [
            Constraint::Percentage(19),
            Constraint::Percentage(19),
            Constraint::Percentage(18),
            Constraint::Percentage(26),
            Constraint::Percentage(18),
        ],
    )
    .header(header)
    .column_spacing(1)
    .block(reference_block("Risk & return — YTD"));
    frame.render_widget(table, area);
}

fn metric_cell(value: &'static str, highlighted: bool) -> Cell<'static> {
    let mut style = theme::value(value);
    if highlighted {
        style = style.bg(METRIC_HIGHLIGHT);
    }
    Cell::from(Line::from(value).alignment(Alignment::Right)).style(style)
}

fn render_metric_pairs(
    frame: &mut Frame,
    area: Rect,
    title: &'static str,
    values: &[(&'static str, &'static str)],
    highlighted: Option<usize>,
) {
    let rows = values.iter().enumerate().map(|(index, (label, value))| {
        let mut value_style = theme::value(value);
        if highlighted == Some(index) {
            value_style = value_style.bg(METRIC_HIGHLIGHT);
        }
        Row::new([
            Cell::from(*label).style(Style::new().fg(INK.into())),
            Cell::from(Line::from(*value).alignment(Alignment::Right)).style(value_style),
        ])
    });
    frame.render_widget(
        Table::new(
            rows,
            [Constraint::Percentage(62), Constraint::Percentage(38)],
        )
        .block(reference_block(title)),
        area,
    );
}

fn render_countries(frame: &mut Frame, area: Rect) {
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(5)]).split(area);
    frame.render_widget(
        Paragraph::new("Largest country exposure")
            .alignment(Alignment::Center)
            .style(Style::new().fg(INK.into()).add_modifier(Modifier::ITALIC)),
        rows[0],
    );
    frame.render_widget(
        Table::new(
            COUNTRIES.iter().map(|(name, weight)| {
                Row::new([
                    Cell::from(*name),
                    Cell::from(Line::from(*weight).alignment(Alignment::Right)),
                ])
            }),
            [Constraint::Percentage(72), Constraint::Percentage(28)],
        ),
        rows[1],
    );
}

fn render_holdings(frame: &mut Frame, area: Rect) {
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(5)]).split(area);
    frame.render_widget(
        Paragraph::new("Top holdings")
            .alignment(Alignment::Center)
            .style(Style::new().fg(INK.into()).add_modifier(Modifier::ITALIC)),
        rows[0],
    );
    let columns = Layout::horizontal([
        Constraint::Percentage(50),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .split(rows[1]);
    render_holding_half(frame, columns[0], 0);
    render_holding_half(frame, columns[2], 5);
}

fn render_holding_half(frame: &mut Frame, area: Rect, offset: usize) {
    frame.render_widget(
        Table::new(
            HOLDINGS
                .iter()
                .skip(offset)
                .take(5)
                .enumerate()
                .map(|(index, (name, weight))| {
                    Row::new([
                        Cell::from(format!("{}", index + offset + 1)).style(MUTED),
                        Cell::from(*name),
                        Cell::from(Line::from(*weight).alignment(Alignment::Right)),
                    ])
                }),
            [
                Constraint::Length(3),
                Constraint::Min(16),
                Constraint::Length(6),
            ],
        )
        .column_spacing(1),
        area,
    );
}

fn render_movers(
    frame: &mut Frame,
    area: Rect,
    title: &'static str,
    values: &[(&'static str, &'static str)],
) {
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(5)]).split(area);
    frame.render_widget(
        Paragraph::new(title)
            .alignment(Alignment::Center)
            .style(Style::new().fg(INK.into()).add_modifier(Modifier::ITALIC)),
        rows[0],
    );
    frame.render_widget(
        Table::new(
            values.iter().map(|(name, change)| {
                Row::new([
                    Cell::from(*name),
                    Cell::from(Line::from(*change).alignment(Alignment::Right))
                        .style(theme::value(change)),
                ])
            }),
            [Constraint::Min(14), Constraint::Length(8)],
        )
        .column_spacing(1),
        rows[1],
    );
}

fn render_headlines(frame: &mut Frame, area: Rect) {
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(5)]).split(area);
    frame.render_widget(
        Paragraph::new("Market headlines")
            .alignment(Alignment::Center)
            .style(Style::new().fg(INK.into()).add_modifier(Modifier::ITALIC)),
        rows[0],
    );
    frame.render_widget(
        Paragraph::new(
            HEADLINES
                .iter()
                .map(|headline| {
                    Line::from(vec![
                        Span::styled(" • ", MUTED),
                        Span::styled(*headline, MUTED),
                    ])
                })
                .collect::<Vec<_>>(),
        ),
        rows[1],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyModifiers, MouseButton, MouseEventKind};
    use ratatui::{backend::TestBackend, Terminal};
    use std::{
        collections::HashSet,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Mutex,
        },
    };

    struct LiveQuery;

    fn live_snapshot() -> LiveOverviewSnapshot {
        LiveOverviewSnapshot {
            net_asset_value: "$48.00".to_owned(),
            ytd_return: "N/A".to_owned(),
            available_cash: "$0.00".to_owned(),
            sharpe: "N/A".to_owned(),
            portfolio_source: "CSV · actual-positions.csv".to_owned(),
            portfolio_as_of: "2026-08-26 12:00 UTC".to_owned(),
            holdings: vec![super::super::OverviewHolding {
                symbol: "USER".to_owned(),
                quantity: "4".to_owned(),
                market_value: "$48.00".to_owned(),
                pnl: "+20.00%".to_owned(),
                weight: "100.00%".to_owned(),
            }],
            headlines: vec![super::super::OverviewHeadline {
                time: "12:01".to_owned(),
                topic: "TOP".to_owned(),
                title: "Cached publisher headline".to_owned(),
                region: "US".to_owned(),
            }],
            news_status: "LIVE · 1 STORY".to_owned(),
            market_pulse: vec![super::super::OverviewMarketPulse {
                symbol: "SPY".to_owned(),
                last: "653.28".to_owned(),
                percent_change: "+0.64%".to_owned(),
                quality: "DELAYED".to_owned(),
                as_of: "2026-08-26 12:01 UTC".to_owned(),
                provider: "TEST".to_owned(),
            }],
            events: vec![super::super::OverviewEvent {
                time: "14:00".to_owned(),
                region: "US".to_owned(),
                importance: "HIGH".to_owned(),
                title: "FED DECISION".to_owned(),
                period: "AUG".to_owned(),
            }],
            source_health: vec![super::super::OverviewSourceHealth {
                source: "MARKETS".to_owned(),
                state: super::super::OverviewHealthState::Ready,
                detail: "TEST · DELAYED".to_owned(),
                as_of: "2026-08-26 12:01 UTC".to_owned(),
                command: "MARKETS".to_owned(),
            }],
            saved_work: vec![super::super::OverviewSavedWork {
                id: 9,
                label: "Apple research".to_owned(),
                command: "SEC AAPL US".to_owned(),
                kind: "COMMAND TILE".to_owned(),
            }],
            priorities: vec![super::super::OverviewPriority {
                id: "performance-missing".to_owned(),
                score: 45,
                title: "Performance history is unavailable".to_owned(),
                reason: "Point-in-time positions cannot establish returns".to_owned(),
                source: "PORTFOLIO".to_owned(),
                as_of: "2026-08-26 12:00 UTC".to_owned(),
                command: "PORT PERF".to_owned(),
            }],
        }
    }

    impl OverviewQuery for LiveQuery {
        fn load_overview(&self) -> OverviewSnapshot {
            OverviewSnapshot::Live(Box::new(live_snapshot()))
        }
    }

    struct MutableLiveQuery {
        snapshot: Mutex<LiveOverviewSnapshot>,
        refreshes: AtomicUsize,
    }

    impl MutableLiveQuery {
        fn new() -> Self {
            Self {
                snapshot: Mutex::new(live_snapshot()),
                refreshes: AtomicUsize::new(0),
            }
        }
    }

    impl OverviewQuery for MutableLiveQuery {
        fn load_overview(&self) -> OverviewSnapshot {
            OverviewSnapshot::Live(Box::new(self.snapshot.lock().unwrap().clone()))
        }

        fn request_refresh(&self) {
            self.refreshes.fetch_add(1, Ordering::Relaxed);
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
    fn stepped_series_preserves_horizontal_plateaus_and_vertical_moves() {
        assert_eq!(
            stepped_points(&[(0.0, 1.0), (2.0, 3.0), (5.0, 2.0)]),
            [(0.0, 1.0), (2.0, 1.0), (2.0, 3.0), (5.0, 3.0), (5.0, 2.0)]
        );
    }

    #[test]
    fn live_overview_renders_imported_and_cached_values_without_gallery_analytics() {
        let workspace = OverviewWorkspace::new(Arc::new(LiveQuery));
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
        assert!(rendered.contains("actual-positions.csv"));
        assert!(rendered.contains("Cached publisher headline"));
        assert!(rendered.contains("MISSION CONTROL"));
        assert!(rendered.contains("Performance history"));
        assert!(rendered.contains("FED DECISION"));
        assert!(!rendered.contains("Advantest"));
        assert!(!rendered.contains("S&P Futures"));
    }

    #[test]
    fn live_overview_rows_open_security_and_news() {
        let area = Rect::new(0, 0, 160, 48);
        let mut workspace = OverviewWorkspace::new(Arc::new(LiveQuery));
        let actions = workspace.actions(area);
        let holding = actions
            .iter()
            .find(|action| action.id.starts_with("holding:0:"))
            .unwrap();
        let headline = actions
            .iter()
            .find(|action| action.id.starts_with("headline:0:"))
            .unwrap();

        assert!(workspace.handle_mouse(click(holding.area.x, holding.area.y), area));
        assert!(workspace.handle_mouse(click(headline.area.x, headline.area.y), area));

        assert_eq!(
            workspace.poll_intents(),
            vec![
                AppIntent::DispatchCommand {
                    command: "SEC USER US".to_owned(),
                    origin: ID,
                },
                AppIntent::DispatchCommand {
                    command: "NEWS TOP".to_owned(),
                    origin: ID,
                },
            ]
        );
    }

    #[test]
    fn live_actions_share_geometry_and_revalidate_async_rows() {
        let area = Rect::new(0, 0, 120, 36);
        let query = Arc::new(MutableLiveQuery::new());
        let mut workspace = OverviewWorkspace::new(query.clone());
        let actions = workspace.actions(area);
        let ids = actions
            .iter()
            .map(|action| action.id.as_str())
            .collect::<HashSet<_>>();
        assert_eq!(ids.len(), actions.len());
        assert!(actions.iter().all(|action| {
            action.area.x >= area.x
                && action.area.y >= area.y
                && action.area.right() <= area.right()
                && action.area.bottom() <= area.bottom()
        }));

        let holding = actions
            .iter()
            .find(|action| action.id.starts_with("holding:0:"))
            .unwrap();
        assert!(!holding.preferred);
        assert!(
            actions
                .iter()
                .find(|action| action.id.starts_with("priority:0:"))
                .unwrap()
                .preferred
        );
        let priority = actions
            .iter()
            .find(|action| action.id.starts_with("priority:0:"))
            .unwrap()
            .clone();
        assert!(workspace.activate_action(&priority.id));
        assert_eq!(
            workspace.poll_intents(),
            vec![AppIntent::DispatchCommand {
                command: "PORT PERF".to_owned(),
                origin: ID,
            }]
        );
        query.snapshot.lock().unwrap().priorities[0].reason = "Replacement rationale".to_owned();
        assert!(!workspace.activate_action(&priority.id));

        let saved = workspace
            .actions(area)
            .into_iter()
            .find(|action| action.id.starts_with("saved:9:"))
            .unwrap();
        assert!(workspace.activate_action(&saved.id));
        assert_eq!(
            workspace.poll_intents(),
            vec![AppIntent::DispatchCommand {
                command: "SEC AAPL US".to_owned(),
                origin: ID,
            }]
        );
        assert!(workspace.activate_action(&holding.id));
        assert_eq!(
            workspace.poll_intents(),
            vec![AppIntent::DispatchCommand {
                command: "SEC USER US".to_owned(),
                origin: ID,
            }]
        );

        let headline = actions
            .iter()
            .find(|action| action.id.starts_with("headline:0:"))
            .unwrap()
            .clone();
        assert!(workspace.handle_mouse(click(headline.area.x, headline.area.y), area));
        assert_eq!(
            workspace.poll_intents(),
            vec![AppIntent::DispatchCommand {
                command: "NEWS TOP".to_owned(),
                origin: ID,
            }]
        );

        query.snapshot.lock().unwrap().headlines[0].title = "Replacement story".to_owned();
        assert!(!workspace.activate_action(&headline.id));

        let refresh = workspace
            .actions(area)
            .into_iter()
            .find(|action| action.id == "control:refresh")
            .unwrap();
        assert!(workspace.handle_mouse(click(refresh.area.x, refresh.area.y), area));
        assert_eq!(query.refreshes.load(Ordering::Relaxed), 1);
    }
}
