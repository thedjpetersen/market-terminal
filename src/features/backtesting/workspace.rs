use std::sync::{
    mpsc::{sync_channel, Receiver, SyncSender, TrySendError},
    Arc,
};

use chrono::{DateTime, Utc};
use crossterm::event::{KeyCode, KeyEvent, MouseEvent};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Cell, Paragraph, Row, Sparkline, Table, Wrap},
    Frame,
};

use crate::{
    app::{
        AppIntent, CommandInvocation, ViewRestoreReport, ViewValue, Workspace, WorkspaceAction,
        WorkspaceDescriptor, WorkspaceViewState,
    },
    ui::{
        components::terminal_block,
        is_primary_click, table_row_at,
        theme::{AMBER, BG, CYAN, GREEN, INK, MUTED, RED, YELLOW},
    },
};

use super::{
    run_backtest, BacktestArtifact, BacktestArtifactFileStore, BacktestArtifactStore,
    BacktestConfig, BacktestHistoryQuery, BacktestHistoryRequest, ID,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BacktestView {
    Summary,
    Trades,
}

impl BacktestView {
    const fn label(self) -> &'static str {
        match self {
            Self::Summary => "SUMMARY",
            Self::Trades => "TRADES",
        }
    }
}

struct RunRequest {
    generation: u64,
    config: BacktestConfig,
}

struct RunResult {
    generation: u64,
    result: Result<BacktestArtifact, String>,
}

#[derive(Clone, Copy)]
struct BacktestAreas {
    header: Rect,
    tabs: Rect,
    body: Rect,
    footer: Rect,
}

pub struct BacktestWorkspace {
    config: BacktestConfig,
    view: BacktestView,
    selected_trade: usize,
    status: String,
    artifact: Option<BacktestArtifact>,
    desired_generation: u64,
    pending_request: Option<RunRequest>,
    request_sender: SyncSender<RunRequest>,
    result_receiver: Receiver<RunResult>,
    pending_intents: Vec<AppIntent>,
    artifact_store: Option<Arc<dyn BacktestArtifactStore>>,
    artifact_files: Option<Arc<dyn BacktestArtifactFileStore>>,
}

impl BacktestWorkspace {
    pub fn new(query: Arc<dyn BacktestHistoryQuery>) -> Self {
        Self::configured(query, None, None)
    }

    pub fn persistent(
        query: Arc<dyn BacktestHistoryQuery>,
        artifact_store: Arc<dyn BacktestArtifactStore>,
        artifact_files: Arc<dyn BacktestArtifactFileStore>,
    ) -> Self {
        Self::configured(query, Some(artifact_store), Some(artifact_files))
    }

    fn configured(
        query: Arc<dyn BacktestHistoryQuery>,
        artifact_store: Option<Arc<dyn BacktestArtifactStore>>,
        artifact_files: Option<Arc<dyn BacktestArtifactFileStore>>,
    ) -> Self {
        let (request_sender, worker_receiver) = sync_channel::<RunRequest>(1);
        let (worker_sender, result_receiver) = sync_channel::<RunResult>(1);
        std::thread::Builder::new()
            .name("backtest-runner".to_owned())
            .spawn(move || {
                while let Ok(mut request) = worker_receiver.recv() {
                    while let Ok(newer) = worker_receiver.try_recv() {
                        request = newer;
                    }
                    let history_request = BacktestHistoryRequest {
                        instrument_id: request.config.instrument_id.clone(),
                        symbol: request.config.symbol.clone(),
                    };
                    let result = query
                        .load_history(&history_request)
                        .map_err(|error| error.to_string())
                        .and_then(|history| {
                            if history.instrument_id != request.config.instrument_id
                                || history.symbol != request.config.symbol
                            {
                                return Err(
                                    "history identity does not match the requested instrument"
                                        .to_owned(),
                                );
                            }
                            run_backtest(
                                &request.config,
                                &history.bars,
                                history.source,
                                history.quality,
                                history.input_version,
                            )
                            .map_err(|error| error.to_string())
                        });
                    if worker_sender
                        .send(RunResult {
                            generation: request.generation,
                            result,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .expect("backtest worker should start");

        let mut workspace = Self {
            config: BacktestConfig::moving_average_cross("terminal:aapl", "AAPL"),
            view: BacktestView::Summary,
            selected_trade: 0,
            status: "RESEARCH REPLAY · NEXT-BAR EXECUTION · NO LIVE ORDER PATH".to_owned(),
            artifact: None,
            desired_generation: 0,
            pending_request: None,
            request_sender,
            result_receiver,
            pending_intents: Vec::new(),
            artifact_store,
            artifact_files,
        };
        workspace.queue_run();
        workspace
    }

    fn save_artifact(&mut self) {
        let Some(artifact) = self.artifact.as_ref() else {
            self.status = "SAVE REQUIRES A COMPLETED BACKTEST".to_owned();
            return;
        };
        let Some(store) = &self.artifact_store else {
            self.status = "BACKTEST ARTIFACT PERSISTENCE IS DISABLED".to_owned();
            return;
        };
        self.status = match store.save_artifact(artifact) {
            Ok(true) => format!(
                "SAVED IMMUTABLE RUN {} · {}",
                artifact.run_digest, artifact.artifact_digest
            ),
            Ok(false) => format!("RUN {} ALREADY SAVED · IDENTICAL", artifact.run_digest),
            Err(error) => format!("BACKTEST SAVE FAILED · {error}"),
        };
    }

    fn list_artifacts(&mut self) {
        let Some(store) = &self.artifact_store else {
            self.status = "BACKTEST ARTIFACT PERSISTENCE IS DISABLED".to_owned();
            return;
        };
        self.status = match store.list_artifacts() {
            Ok(artifacts) if artifacts.is_empty() => "NO SAVED BACKTEST RUNS".to_owned(),
            Ok(artifacts) => format!(
                "SAVED RUNS · {}",
                artifacts
                    .iter()
                    .map(|artifact| format!("{} {}", artifact.symbol, artifact.run_digest))
                    .collect::<Vec<_>>()
                    .join(" · ")
            ),
            Err(error) => format!("BACKTEST LIST FAILED · {error}"),
        };
    }

    fn open_artifact(&mut self, run_digest: Option<&String>) {
        let Some(run_digest) = run_digest else {
            self.status = "BACKTEST OPEN REQUIRES A RUN DIGEST".to_owned();
            return;
        };
        let Some(store) = &self.artifact_store else {
            self.status = "BACKTEST ARTIFACT PERSISTENCE IS DISABLED".to_owned();
            return;
        };
        match store.load_artifact(run_digest) {
            Ok(artifact) => {
                self.config = artifact.config.clone();
                self.selected_trade = 0;
                self.status = format!(
                    "OPENED VERIFIED RUN {} · {}",
                    artifact.run_digest, artifact.artifact_digest
                );
                self.artifact = Some(artifact);
            }
            Err(error) => self.status = format!("BACKTEST OPEN FAILED · {error}"),
        }
    }

    fn delete_artifact(&mut self, run_digest: Option<&String>) {
        let Some(run_digest) = run_digest else {
            self.status = "BACKTEST DELETE REQUIRES A RUN DIGEST".to_owned();
            return;
        };
        let Some(store) = &self.artifact_store else {
            self.status = "BACKTEST ARTIFACT PERSISTENCE IS DISABLED".to_owned();
            return;
        };
        self.status = match store.delete_artifact(run_digest) {
            Ok(true) => format!("DELETED SAVED RUN {run_digest}"),
            Ok(false) => format!("SAVED RUN {run_digest} WAS NOT FOUND"),
            Err(error) => format!("BACKTEST DELETE FAILED · {error}"),
        };
    }

    fn export_artifact(&mut self, location: Option<&String>, overwrite: bool) {
        let Some(location) = location else {
            self.status = "BACKTEST EXPORT REQUIRES A JSON PATH".to_owned();
            return;
        };
        let Some(artifact) = self.artifact.as_ref() else {
            self.status = "EXPORT REQUIRES A COMPLETED BACKTEST".to_owned();
            return;
        };
        let Some(files) = &self.artifact_files else {
            self.status = "BACKTEST ARTIFACT EXPORT IS DISABLED".to_owned();
            return;
        };
        let document = match serde_json::to_string_pretty(artifact) {
            Ok(document) => format!("{document}\n"),
            Err(error) => {
                self.status = format!("BACKTEST EXPORT FAILED · {error}");
                return;
            }
        };
        self.status = match files.write_artifact(location, &document, overwrite) {
            Ok(()) => format!(
                "EXPORTED VERIFIED RUN {} · {}",
                artifact.run_digest, artifact.artifact_digest
            ),
            Err(error) => format!("BACKTEST EXPORT FAILED · {error}"),
        };
    }

    fn queue_run(&mut self) {
        self.desired_generation = self.desired_generation.wrapping_add(1);
        self.pending_request = Some(RunRequest {
            generation: self.desired_generation,
            config: self.config.clone(),
        });
        self.status = format!(
            "RUNNING {} · SMA {}/{} · {} BPS",
            self.config.symbol,
            self.config.fast_window,
            self.config.slow_window,
            self.config.execution_cost_bps
        );
        self.dispatch_pending();
    }

    fn dispatch_pending(&mut self) {
        let Some(request) = self.pending_request.take() else {
            return;
        };
        match self.request_sender.try_send(request) {
            Ok(()) => {}
            Err(TrySendError::Full(request)) => self.pending_request = Some(request),
            Err(TrySendError::Disconnected(_)) => {
                self.status = "BACKTEST WORKER STOPPED".to_owned();
            }
        }
    }

    fn poll_results(&mut self) {
        while let Ok(result) = self.result_receiver.try_recv() {
            if result.generation != self.desired_generation {
                continue;
            }
            match result.result {
                Ok(artifact) => {
                    self.status = format!(
                        "REPRODUCIBLE RUN · {} BARS · {} TRADES · {}",
                        artifact.bars,
                        artifact.trades.len(),
                        artifact.run_digest
                    );
                    self.selected_trade = self
                        .selected_trade
                        .min(artifact.trades.len().saturating_sub(1));
                    self.artifact = Some(artifact);
                }
                Err(error) => {
                    self.status = format!("RUN FAILED · {error} · LAST VALID RESULT RETAINED");
                }
            }
        }
        self.dispatch_pending();
    }

    fn move_trade(&mut self, delta: isize) {
        let count = self.artifact.as_ref().map_or(0, |run| run.trades.len());
        self.selected_trade = if count == 0 {
            0
        } else {
            self.selected_trade
                .saturating_add_signed(delta)
                .min(count - 1)
        };
    }

    fn open_chart(&mut self) -> bool {
        if self.config.symbol.trim().is_empty() {
            return false;
        }
        self.pending_intents.push(AppIntent::DispatchCommand {
            command: format!("CHART {}", self.config.symbol),
            origin: ID,
        });
        true
    }

    fn parse_command(&self, invocation: &CommandInvocation) -> Result<BacktestConfig, String> {
        let mut candidate = self.config.clone();
        if invocation.args.is_empty() {
            return Ok(candidate);
        }
        let keywords = ["FAST", "SLOW", "COST", "COMMISSION"];
        let option_start = invocation
            .args
            .iter()
            .position(|arg| {
                keywords
                    .iter()
                    .any(|keyword| arg.eq_ignore_ascii_case(keyword))
            })
            .unwrap_or(invocation.args.len());
        if option_start > 0 {
            let symbol = invocation.args[..option_start]
                .join(" ")
                .trim()
                .to_ascii_uppercase();
            if symbol.is_empty() || symbol.len() > 64 {
                return Err("symbol must contain 1-64 characters".to_owned());
            }
            candidate.symbol = symbol.clone();
            candidate.instrument_id =
                format!("terminal:{}", symbol.to_ascii_lowercase().replace(' ', ":"));
        }
        let mut index = option_start;
        while index < invocation.args.len() {
            let key = invocation.args[index].to_ascii_uppercase();
            let value = invocation
                .args
                .get(index + 1)
                .ok_or_else(|| format!("{key} requires a value"))?;
            match key.as_str() {
                "FAST" => {
                    candidate.fast_window = value
                        .parse::<usize>()
                        .map_err(|_| "FAST requires an integer".to_owned())?;
                }
                "SLOW" => {
                    candidate.slow_window = value
                        .parse::<usize>()
                        .map_err(|_| "SLOW requires an integer".to_owned())?;
                }
                "COST" => {
                    candidate.execution_cost_bps = value
                        .parse::<u32>()
                        .map_err(|_| "COST requires integer basis points".to_owned())?;
                }
                "COMMISSION" => {
                    let commission = value
                        .parse::<f64>()
                        .map_err(|_| "COMMISSION requires a decimal amount".to_owned())?;
                    if !commission.is_finite() || commission < 0.0 || commission > 10_000.0 {
                        return Err("COMMISSION must be between 0 and 10000".to_owned());
                    }
                    candidate.commission_micros = (commission * 1_000_000.0).round() as i64;
                }
                _ => return Err(format!("unknown BACKTEST option {key}")),
            }
            index += 2;
        }
        if candidate.fast_window < 2
            || candidate.fast_window >= candidate.slow_window
            || candidate.slow_window > 500
            || candidate.execution_cost_bps > 1_000
        {
            return Err("requires 2 <= FAST < SLOW <= 500 and COST <= 1000".to_owned());
        }
        Ok(candidate)
    }
}

impl Workspace for BacktestWorkspace {
    fn descriptor(&self) -> WorkspaceDescriptor {
        WorkspaceDescriptor {
            id: ID,
            label: "BACKTEST",
            hotkey: '\0',
            commands: &["BACKTEST", "BT"],
        }
    }

    fn handle_command(&mut self, invocation: &CommandInvocation) -> bool {
        if let Some(operation) = invocation.args.first() {
            match operation.to_ascii_uppercase().as_str() {
                "SAVE" => {
                    self.save_artifact();
                    return true;
                }
                "LIST" => {
                    self.list_artifacts();
                    return true;
                }
                "OPEN" | "LOAD" => {
                    self.open_artifact(invocation.args.get(1));
                    return true;
                }
                "DELETE" | "DROP" => {
                    self.delete_artifact(invocation.args.get(1));
                    return true;
                }
                "EXPORT" => {
                    self.export_artifact(invocation.args.get(1), false);
                    return true;
                }
                "EXPORT!" => {
                    self.export_artifact(invocation.args.get(1), true);
                    return true;
                }
                _ => {}
            }
        }
        match self.parse_command(invocation) {
            Ok(config) => {
                self.config = config;
                self.queue_run();
            }
            Err(error) => self.status = format!("BACKTEST COMMAND ERROR · {error}"),
        }
        true
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Tab => {
                self.view = match self.view {
                    BacktestView::Summary => BacktestView::Trades,
                    BacktestView::Trades => BacktestView::Summary,
                };
                true
            }
            KeyCode::Char('1') => {
                self.view = BacktestView::Summary;
                true
            }
            KeyCode::Char('2') => {
                self.view = BacktestView::Trades;
                true
            }
            KeyCode::Up | KeyCode::Char('k') if self.view == BacktestView::Trades => {
                self.move_trade(-1);
                true
            }
            KeyCode::Down | KeyCode::Char('j') if self.view == BacktestView::Trades => {
                self.move_trade(1);
                true
            }
            KeyCode::Char('r') | KeyCode::F(9) => {
                self.queue_run();
                true
            }
            KeyCode::Char('c') => self.open_chart(),
            _ => false,
        }
    }

    fn handle_mouse(&mut self, event: MouseEvent, area: Rect) -> bool {
        let areas = backtest_areas(area);
        if is_primary_click(event, areas.header) {
            self.queue_run();
            return true;
        }
        if is_primary_click(event, areas.tabs) {
            self.view = if event.column < areas.tabs.x.saturating_add(18) {
                BacktestView::Summary
            } else {
                BacktestView::Trades
            };
            return true;
        }
        if self.view == BacktestView::Trades {
            let count = self.artifact.as_ref().map_or(0, |run| run.trades.len());
            if let Some(index) = table_row_at(event, areas.body, count) {
                self.selected_trade = index;
                return true;
            }
        }
        if is_primary_click(event, areas.footer) {
            return self.open_chart();
        }
        Workspace::handle_mouse(self, event, area)
    }

    fn actions(&self, area: Rect) -> Vec<WorkspaceAction> {
        let areas = backtest_areas(area);
        let tab_width = areas.tabs.width / 2;
        let mut summary = WorkspaceAction::new(
            "view:summary",
            "Show backtest summary",
            Rect::new(areas.tabs.x, areas.tabs.y, tab_width, areas.tabs.height),
        );
        let mut trades = WorkspaceAction::new(
            "view:trades",
            "Show backtest trades",
            Rect::new(
                areas.tabs.x.saturating_add(tab_width),
                areas.tabs.y,
                areas.tabs.width.saturating_sub(tab_width),
                areas.tabs.height,
            ),
        );
        if self.view == BacktestView::Summary {
            summary = summary.preferred();
        } else {
            trades = trades.preferred();
        }
        vec![
            summary,
            trades,
            WorkspaceAction::new("run:refresh", "Rerun with identical inputs", areas.header),
            WorkspaceAction::new("open:chart", "Open input instrument chart", areas.footer),
        ]
    }

    fn activate_action(&mut self, id: &str) -> bool {
        match id {
            "view:summary" => self.view = BacktestView::Summary,
            "view:trades" => self.view = BacktestView::Trades,
            "run:refresh" => self.queue_run(),
            "open:chart" => return self.open_chart(),
            _ => return false,
        }
        true
    }

    fn poll_intents(&mut self) -> Vec<AppIntent> {
        self.poll_results();
        std::mem::take(&mut self.pending_intents)
    }

    fn capture_view(&self) -> WorkspaceViewState {
        WorkspaceViewState::new(ID.as_str())
            .with_field(
                "instrument_id",
                ViewValue::Text(self.config.instrument_id.clone()),
            )
            .with_field("symbol", ViewValue::Text(self.config.symbol.clone()))
            .with_field(
                "fast_window",
                ViewValue::Unsigned(self.config.fast_window as u64),
            )
            .with_field(
                "slow_window",
                ViewValue::Unsigned(self.config.slow_window as u64),
            )
            .with_field(
                "execution_cost_bps",
                ViewValue::Unsigned(u64::from(self.config.execution_cost_bps)),
            )
            .with_field(
                "commission_micros",
                ViewValue::Unsigned(self.config.commission_micros.max(0) as u64),
            )
            .with_field("view", ViewValue::Text(self.view.label().to_owned()))
            .with_field(
                "selected_trade",
                ViewValue::Unsigned(self.selected_trade as u64),
            )
    }

    fn restore_view(&mut self, state: &WorkspaceViewState) -> ViewRestoreReport {
        if !state.workspace.eq_ignore_ascii_case(ID.as_str()) {
            return ViewRestoreReport::warning(format!(
                "saved state belongs to {}, not {}",
                state.workspace,
                ID.as_str()
            ));
        }
        let mut report = ViewRestoreReport::default();
        let previous = self.config.clone();
        if let (Some(id), Some(symbol)) = (
            state
                .fields
                .get("instrument_id")
                .and_then(ViewValue::as_text),
            state.fields.get("symbol").and_then(ViewValue::as_text),
        ) {
            if id.trim().is_empty() || symbol.trim().is_empty() || symbol.len() > 64 {
                report.skipped_fields += 2;
            } else {
                self.config.instrument_id = id.to_owned();
                self.config.symbol = symbol.to_owned();
                report.restored_fields += 2;
            }
        }
        for (name, target) in [
            ("fast_window", &mut self.config.fast_window),
            ("slow_window", &mut self.config.slow_window),
        ] {
            if let Some(value) = state.fields.get(name).and_then(ViewValue::as_unsigned) {
                if let Ok(value) = usize::try_from(value) {
                    *target = value;
                    report.restored_fields += 1;
                } else {
                    report.skipped_fields += 1;
                }
            }
        }
        if let Some(value) = state
            .fields
            .get("execution_cost_bps")
            .and_then(ViewValue::as_unsigned)
        {
            if let Ok(value) = u32::try_from(value) {
                self.config.execution_cost_bps = value;
                report.restored_fields += 1;
            } else {
                report.skipped_fields += 1;
            }
        }
        if let Some(value) = state
            .fields
            .get("commission_micros")
            .and_then(ViewValue::as_unsigned)
        {
            if let Ok(value) = i64::try_from(value) {
                self.config.commission_micros = value;
                report.restored_fields += 1;
            } else {
                report.skipped_fields += 1;
            }
        }
        if let Some(view) = state.fields.get("view").and_then(ViewValue::as_text) {
            match view {
                "SUMMARY" => self.view = BacktestView::Summary,
                "TRADES" => self.view = BacktestView::Trades,
                _ => report.skipped_fields += 1,
            }
            if matches!(view, "SUMMARY" | "TRADES") {
                report.restored_fields += 1;
            }
        }
        if let Some(value) = state
            .fields
            .get("selected_trade")
            .and_then(ViewValue::as_unsigned)
        {
            self.selected_trade = usize::try_from(value).unwrap_or(usize::MAX).min(10_000);
            report.restored_fields += 1;
        }
        let valid = self.config.fast_window >= 2
            && self.config.fast_window < self.config.slow_window
            && self.config.slow_window <= 500
            && self.config.execution_cost_bps <= 1_000;
        if valid && self.config != previous {
            self.queue_run();
        } else if !valid {
            self.config = previous;
            report
                .warnings
                .push("ignored invalid backtest configuration".to_owned());
            report.skipped_fields += 1;
        }
        report
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let areas = backtest_areas(area);
        render_header(
            frame,
            areas.header,
            &self.config,
            self.artifact.as_ref(),
            &self.status,
        );
        render_tabs(frame, areas.tabs, self.view);
        match (&self.artifact, self.view) {
            (Some(run), BacktestView::Summary) => render_summary(frame, areas.body, run),
            (Some(run), BacktestView::Trades) => {
                render_trades(frame, areas.body, run, self.selected_trade)
            }
            (None, _) => frame.render_widget(
                Paragraph::new(vec![
                    Line::styled("BACKTEST RUN IS LOADING", AMBER),
                    Line::styled("The UI keeps work off the render/input thread.", MUTED),
                ])
                .block(terminal_block("BT", "REPRODUCIBLE RESEARCH")),
                areas.body,
            ),
        }
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" 1/2/TAB VIEW  ", CYAN),
                Span::styled("↑↓/JK TRADE  ", INK),
                Span::styled("R/F9 RERUN  ", YELLOW),
                Span::styled("C CHART  ", GREEN),
                Span::styled("RESEARCH ONLY", RED),
            ]))
            .style(Style::new().bg(BG.into())),
            areas.footer,
        );
    }
}

fn backtest_areas(area: Rect) -> BacktestAreas {
    let rows = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Min(8),
        Constraint::Length(2),
    ])
    .split(area);
    BacktestAreas {
        header: rows[0],
        tabs: rows[1],
        body: rows[2],
        footer: rows[3],
    }
}

fn render_header(
    frame: &mut Frame,
    area: Rect,
    config: &BacktestConfig,
    artifact: Option<&BacktestArtifact>,
    status: &str,
) {
    let digest = artifact.map_or("PENDING", |run| run.run_digest.as_str());
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!(" {}  ", config.symbol),
                Style::new().bg(AMBER.into()).fg(BG.into()).bold(),
            ),
            Span::styled(
                format!("SMA {}/{}  ", config.fast_window, config.slow_window),
                CYAN,
            ),
            Span::styled(
                format!(
                    "{} BPS + {:.2} COMMISSION  ",
                    config.execution_cost_bps,
                    config.commission_micros as f64 / 1_000_000.0
                ),
                INK,
            ),
            Span::styled(digest, GREEN),
            Span::styled(format!("  ·  {status}"), MUTED),
        ]))
        .block(terminal_block("BT", "LOOK-AHEAD-SAFE RESEARCH REPLAY"))
        .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_tabs(frame: &mut Frame, area: Rect, active: BacktestView) {
    let line = [BacktestView::Summary, BacktestView::Trades]
        .into_iter()
        .flat_map(|view| {
            let style = if view == active {
                Style::new().bg(CYAN.into()).fg(BG.into()).bold()
            } else {
                Style::new().fg(MUTED.into())
            };
            [
                Span::styled(format!("  {}  ", view.label()), style),
                Span::raw("  "),
            ]
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(Line::from(line)).block(terminal_block("VIEW", "RUN ARTIFACT")),
        area,
    );
}

fn render_summary(frame: &mut Frame, area: Rect, run: &BacktestArtifact) {
    let columns = if area.width >= 100 {
        Layout::horizontal([Constraint::Percentage(66), Constraint::Percentage(34)]).split(area)
    } else {
        Layout::horizontal([Constraint::Percentage(100), Constraint::Length(0)]).split(area)
    };
    let minimum = run
        .equity
        .iter()
        .map(|point| point.equity_micros)
        .min()
        .unwrap_or(0);
    let curve = run
        .equity
        .iter()
        .map(|point| u64::try_from(point.equity_micros - minimum).unwrap_or_default())
        .collect::<Vec<_>>();
    frame.render_widget(
        Sparkline::default()
            .data(&curve)
            .style(if run.total_return_bps >= 0 {
                GREEN
            } else {
                RED
            })
            .block(terminal_block("EQUITY", "MARK-TO-MARKET CURVE")),
        columns[0],
    );
    if columns[1].width > 0 {
        let lines = vec![
            metric_line("RETURN", format_bps(run.total_return_bps)),
            metric_line(
                "MAX DRAWDOWN",
                format!("-{:.2}%", run.max_drawdown_bps as f64 / 100.0),
            ),
            metric_line(
                "TURNOVER",
                format!("{:.2}%", run.turnover_bps as f64 / 100.0),
            ),
            metric_line("TRADES", run.trades.len().to_string()),
            metric_line("OPEN SHARES", run.open_quantity.to_string()),
            metric_line("BARS", run.bars.to_string()),
            metric_line("INPUT", run.input_version.clone()),
            metric_line("DATA HASH", run.data_digest.clone()),
            metric_line("CONFIG HASH", run.config_digest.clone()),
            metric_line("SOURCE", format!("{} · {}", run.source, run.quality)),
            Line::styled(run.methodology.clone(), AMBER),
        ];
        frame.render_widget(
            Paragraph::new(lines)
                .block(terminal_block("RUN", "RECONCILIATION"))
                .wrap(Wrap { trim: true }),
            columns[1],
        );
    }
}

fn render_trades(frame: &mut Frame, area: Rect, run: &BacktestArtifact, selected: usize) {
    let rows = run.trades.iter().enumerate().map(|(index, trade)| {
        let style = if index == selected {
            Style::new().bg(CYAN.into()).fg(BG.into()).bold()
        } else if trade.side == super::TradeSide::Buy {
            Style::new().fg(GREEN.into())
        } else {
            Style::new().fg(RED.into())
        };
        Row::new(vec![
            Cell::from((index + 1).to_string()),
            Cell::from(trade.side.label()),
            Cell::from(format_timestamp(trade.signal_timestamp)),
            Cell::from(format_timestamp(trade.execution_timestamp)),
            Cell::from(trade.quantity.to_string()),
            Cell::from(format_price(trade.reference_price_micros)),
            Cell::from(format_price(trade.execution_price_micros)),
        ])
        .style(style)
    });
    frame.render_widget(
        Table::new(
            rows,
            [
                Constraint::Length(4),
                Constraint::Length(6),
                Constraint::Length(12),
                Constraint::Length(12),
                Constraint::Length(10),
                Constraint::Length(12),
                Constraint::Length(12),
            ],
        )
        .header(
            Row::new([
                "#",
                "SIDE",
                "SIGNAL",
                "NEXT OPEN",
                "QTY",
                "REFERENCE",
                "FILL",
            ])
            .style(AMBER)
            .bottom_margin(1),
        )
        .block(terminal_block("TRADES", "SIGNAL-TO-EXECUTION AUDIT"))
        .column_spacing(1),
        area,
    );
}

fn metric_line(label: &str, value: String) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label:<14}"), MUTED),
        Span::styled(value, INK),
    ])
}

fn format_price(micros: i64) -> String {
    format!("{:.4}", micros as f64 / 1_000_000.0)
}
fn format_bps(bps: i32) -> String {
    format!("{:+.2}%", bps as f64 / 100.0)
}
fn format_timestamp(timestamp: i64) -> String {
    DateTime::<Utc>::from_timestamp(timestamp, 0)
        .map(|value| value.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| timestamp.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Workspace;
    use crate::features::backtesting::{
        BacktestArtifactError, BacktestArtifactSummary, BacktestBar, BacktestHistoryError,
        BacktestHistorySnapshot,
    };
    use ratatui::{backend::TestBackend, Terminal};
    use std::{collections::HashMap, sync::Mutex, thread, time::Duration};

    struct Fixture;
    impl BacktestHistoryQuery for Fixture {
        fn load_history(
            &self,
            request: &BacktestHistoryRequest,
        ) -> Result<BacktestHistorySnapshot, BacktestHistoryError> {
            let bars = (0..140)
                .map(|index| {
                    let close = 100_000_000
                        + i64::from(index) * 100_000
                        + ((index / 35) % 2) as i64 * 5_000_000;
                    BacktestBar {
                        timestamp: 1_700_000_000 + i64::from(index) * 86_400,
                        open_micros: close,
                        high_micros: close + 500_000,
                        low_micros: close - 500_000,
                        close_micros: close,
                        volume: 1_000_000,
                    }
                })
                .collect();
            Ok(BacktestHistorySnapshot {
                instrument_id: request.instrument_id.clone(),
                symbol: request.symbol.clone(),
                bars,
                source: "FIXTURE".to_owned(),
                quality: "REPLAY".to_owned(),
                input_version: "FIXTURE-V1".to_owned(),
            })
        }
    }

    #[derive(Default)]
    struct MemoryArtifacts(Mutex<HashMap<String, BacktestArtifact>>);

    impl BacktestArtifactStore for MemoryArtifacts {
        fn save_artifact(
            &self,
            artifact: &BacktestArtifact,
        ) -> Result<bool, BacktestArtifactError> {
            let mut artifacts = self.0.lock().unwrap();
            match artifacts.get(&artifact.run_digest) {
                Some(existing) if existing == artifact => Ok(false),
                Some(_) => Err(BacktestArtifactError::ImmutableConflict(
                    artifact.run_digest.clone(),
                )),
                None => {
                    artifacts.insert(artifact.run_digest.clone(), artifact.clone());
                    Ok(true)
                }
            }
        }

        fn load_artifact(
            &self,
            run_digest: &str,
        ) -> Result<BacktestArtifact, BacktestArtifactError> {
            self.0
                .lock()
                .unwrap()
                .get(run_digest)
                .cloned()
                .ok_or_else(|| BacktestArtifactError::NotFound(run_digest.to_owned()))
        }

        fn list_artifacts(&self) -> Result<Vec<BacktestArtifactSummary>, BacktestArtifactError> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .values()
                .map(BacktestArtifactSummary::from)
                .collect())
        }

        fn delete_artifact(&self, run_digest: &str) -> Result<bool, BacktestArtifactError> {
            Ok(self.0.lock().unwrap().remove(run_digest).is_some())
        }
    }

    #[derive(Default)]
    struct MemoryFiles(Mutex<Vec<(String, String, bool)>>);

    impl BacktestArtifactFileStore for MemoryFiles {
        fn write_artifact(
            &self,
            location: &str,
            document: &str,
            overwrite: bool,
        ) -> Result<(), BacktestArtifactError> {
            self.0
                .lock()
                .unwrap()
                .push((location.to_owned(), document.to_owned(), overwrite));
            Ok(())
        }
    }

    fn ready_workspace() -> BacktestWorkspace {
        let mut workspace = BacktestWorkspace::new(Arc::new(Fixture));
        for _ in 0..100 {
            workspace.poll_intents();
            if workspace.artifact.is_some() {
                return workspace;
            }
            thread::sleep(Duration::from_millis(1));
        }
        panic!("fixture backtest did not complete")
    }

    fn ready_persistent_workspace(
        artifacts: Arc<MemoryArtifacts>,
        files: Arc<MemoryFiles>,
    ) -> BacktestWorkspace {
        let mut workspace = BacktestWorkspace::persistent(Arc::new(Fixture), artifacts, files);
        for _ in 0..100 {
            workspace.poll_intents();
            if workspace.artifact.is_some() {
                return workspace;
            }
            thread::sleep(Duration::from_millis(1));
        }
        panic!("fixture backtest did not complete")
    }

    #[test]
    fn command_updates_typed_config_and_runs() {
        let mut workspace = ready_workspace();
        workspace.handle_command(&CommandInvocation {
            function: "BACKTEST".to_owned(),
            args: vec![
                "MSFT".to_owned(),
                "FAST".to_owned(),
                "10".to_owned(),
                "SLOW".to_owned(),
                "50".to_owned(),
                "COST".to_owned(),
                "5".to_owned(),
            ],
        });
        assert_eq!(workspace.config.symbol, "MSFT");
        assert_eq!(workspace.config.fast_window, 10);
        assert!(workspace.status.starts_with("RUNNING MSFT"));
    }

    #[test]
    fn invalid_command_preserves_last_valid_artifact() {
        let mut workspace = ready_workspace();
        let digest = workspace.artifact.as_ref().unwrap().run_digest.clone();
        let config = workspace.config.clone();
        workspace.handle_command(&CommandInvocation {
            function: "BACKTEST".to_owned(),
            args: vec![
                "AAPL".to_owned(),
                "FAST".to_owned(),
                "100".to_owned(),
                "SLOW".to_owned(),
                "20".to_owned(),
            ],
        });
        assert_eq!(workspace.artifact.as_ref().unwrap().run_digest, digest);
        assert_eq!(workspace.config, config);
        assert!(workspace.status.contains("COMMAND ERROR"));
    }

    #[test]
    fn commands_save_reopen_list_export_and_delete_verified_artifacts() {
        let artifacts = Arc::new(MemoryArtifacts::default());
        let files = Arc::new(MemoryFiles::default());
        let mut workspace = ready_persistent_workspace(artifacts.clone(), files.clone());
        let expected = workspace.artifact.clone().unwrap();

        workspace.handle_command(&CommandInvocation {
            function: "BACKTEST".to_owned(),
            args: vec!["SAVE".to_owned()],
        });
        assert!(workspace.status.contains("SAVED IMMUTABLE RUN"));
        workspace.handle_command(&CommandInvocation {
            function: "BACKTEST".to_owned(),
            args: vec!["SAVE".to_owned()],
        });
        assert!(workspace.status.contains("ALREADY SAVED · IDENTICAL"));

        workspace.artifact = None;
        workspace.handle_command(&CommandInvocation {
            function: "BACKTEST".to_owned(),
            args: vec!["OPEN".to_owned(), expected.run_digest.clone()],
        });
        assert_eq!(workspace.artifact.as_ref(), Some(&expected));
        assert!(workspace.status.contains("OPENED VERIFIED RUN"));

        workspace.handle_command(&CommandInvocation {
            function: "BACKTEST".to_owned(),
            args: vec!["EXPORT".to_owned(), "run.json".to_owned()],
        });
        let exports = files.0.lock().unwrap();
        assert_eq!(exports.len(), 1);
        assert_eq!(exports[0].0, "run.json");
        assert!(!exports[0].2);
        let decoded: BacktestArtifact = serde_json::from_str(&exports[0].1).unwrap();
        assert_eq!(decoded, expected);
        drop(exports);

        workspace.handle_command(&CommandInvocation {
            function: "BACKTEST".to_owned(),
            args: vec!["LIST".to_owned()],
        });
        assert!(workspace.status.contains(&expected.run_digest));
        workspace.handle_command(&CommandInvocation {
            function: "BACKTEST".to_owned(),
            args: vec!["DELETE".to_owned(), expected.run_digest.clone()],
        });
        assert!(artifacts.0.lock().unwrap().is_empty());
    }

    #[test]
    fn saved_view_round_trips_research_configuration() {
        let workspace = ready_workspace();
        let state = workspace.capture_view();
        let mut restored = ready_workspace();
        let report = restored.restore_view(&state);
        assert!(report.restored_fields >= 7);
        assert_eq!(restored.config, workspace.config);
    }

    #[test]
    fn renders_summary_and_trade_audit_at_three_sizes() {
        let mut workspace = ready_workspace();
        for (width, height) in [(80, 24), (120, 36), (160, 48)] {
            let backend = TestBackend::new(width, height);
            let mut terminal = Terminal::new(backend).unwrap();
            terminal
                .draw(|frame| workspace.render(frame, frame.area()))
                .unwrap();
            let text = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>();
            assert!(text.contains("LOOK-AHEAD-SAFE"));
        }
        workspace.view = BacktestView::Trades;
        let backend = TestBackend::new(120, 36);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| workspace.render(frame, frame.area()))
            .unwrap();
    }
}
