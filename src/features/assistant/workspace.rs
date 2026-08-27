use std::{
    sync::{mpsc, Arc},
    thread,
    time::Instant,
};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
    Frame,
};

use crate::{
    app::{AppIntent, CommandInvocation, ShellContext, Workspace, WorkspaceDescriptor},
    features::portfolio::PortfolioRepository,
    ui::{
        components::terminal_block,
        is_primary_click,
        theme::{AMBER, BG, CYAN, GREEN, INK, MUTED, RED},
    },
};

use super::{
    domain::{
        AssistantMessage, AssistantRequest, AssistantResponse, AssistantRole, AssistantStatus,
        AssistantStreamEvent, AssistantTokenUsage, UiAction,
    },
    AssistantError, AssistantGateway, ID,
};

const MAX_INPUT_BYTES: usize = 4_096;
const MAX_RESPONSE_BYTES: usize = 65_536;
const SPINNER: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

struct PendingResponse {
    result: mpsc::Receiver<Result<AssistantResponse, AssistantError>>,
    updates: mpsc::Receiver<AssistantStreamEvent>,
}

pub struct AssistantWorkspace {
    gateway: Arc<dyn AssistantGateway>,
    portfolio: Arc<dyn PortfolioRepository>,
    model_label: String,
    available_workspaces: Vec<String>,
    active_workspace: String,
    messages: Vec<AssistantMessage>,
    input: String,
    composing: bool,
    status: AssistantStatus,
    pending: Option<PendingResponse>,
    started_at: Option<Instant>,
    streaming_text: String,
    token_usage: Option<AssistantTokenUsage>,
    tool_activity: Option<(String, Option<bool>)>,
}

impl AssistantWorkspace {
    pub fn new(
        gateway: Arc<dyn AssistantGateway>,
        portfolio: Arc<dyn PortfolioRepository>,
        available_workspaces: Vec<String>,
    ) -> Self {
        let model_label = gateway.model_label().to_owned();
        let status = if gateway.is_configured() {
            AssistantStatus::Ready
        } else {
            AssistantStatus::Error(gateway.configuration_hint().to_owned())
        };
        Self {
            gateway,
            portfolio,
            model_label,
            available_workspaces,
            active_workspace: "overview".to_owned(),
            messages: vec![AssistantMessage::system(
                "ASK FOR ANALYSIS OR TELL ME WHICH WORKSPACE TO OPEN OR BRING FORWARD.",
            )],
            input: String::new(),
            composing: false,
            status,
            pending: None,
            started_at: None,
            streaming_text: String::new(),
            token_usage: None,
            tool_activity: None,
        }
    }

    fn submit(&mut self) {
        let prompt = self.input.trim().to_owned();
        if prompt.is_empty() || self.pending.is_some() {
            return;
        }

        self.messages.push(AssistantMessage::user(prompt));
        self.input.clear();
        self.status = AssistantStatus::Loading;
        self.started_at = Some(Instant::now());
        self.streaming_text.clear();
        self.token_usage = None;
        self.tool_activity = None;

        let history_start = self.messages.len().saturating_sub(10);
        let messages = self.messages[history_start..].to_vec();
        let active_workspace = self.active_workspace.clone();
        let available_workspaces = self.available_workspaces.clone();
        let gateway = Arc::clone(&self.gateway);
        let portfolio = Arc::clone(&self.portfolio);
        let (result_sender, result) = mpsc::channel();
        let (updates, update_receiver) = mpsc::channel();
        thread::spawn(move || {
            let request = AssistantRequest {
                messages,
                active_workspace,
                available_workspaces,
                portfolio: portfolio.load_portfolio(),
            };
            let _ = result_sender.send(gateway.complete_stream(request, updates));
        });
        self.pending = Some(PendingResponse {
            result,
            updates: update_receiver,
        });
    }

    fn response_text(response: &AssistantResponse) -> String {
        if !response.content.trim().is_empty() {
            return bounded_text(response.content.trim(), MAX_RESPONSE_BYTES);
        }
        if response.actions.is_empty() {
            return "NO RESPONSE RETURNED".to_owned();
        }
        "UI UPDATED".to_owned()
    }

    fn intents(actions: Vec<UiAction>) -> Vec<AppIntent> {
        actions
            .into_iter()
            .map(|action| match action {
                UiAction::OpenWorkspace { target } => AppIntent::ActivateWorkspace { target },
                UiAction::BringForward { target } => AppIntent::BringWorkspaceForward { target },
                UiAction::RunCommand { command } => AppIntent::DispatchCommand {
                    command,
                    origin: ID,
                },
                UiAction::RestoreLayout => AppIntent::RestoreWorkspaceOrder,
            })
            .collect()
    }

    fn apply_stream_event(&mut self, event: AssistantStreamEvent) {
        match event {
            AssistantStreamEvent::TextDelta(delta) => {
                self.status = AssistantStatus::Streaming;
                let remaining = MAX_RESPONSE_BYTES.saturating_sub(self.streaming_text.len());
                self.streaming_text
                    .push_str(&bounded_text(&delta, remaining));
            }
            AssistantStreamEvent::TokenUsage(usage) => {
                self.token_usage = Some(usage);
            }
            AssistantStreamEvent::ToolStarted(name) => {
                self.status = AssistantStatus::Streaming;
                self.tool_activity = Some((name, None));
            }
            AssistantStreamEvent::ToolFinished { name, success } => {
                self.tool_activity = Some((name, Some(success)));
            }
        }
    }

    fn spinner(&self) -> &'static str {
        let frame = self
            .started_at
            .map_or(0, |started| (started.elapsed().as_millis() / 100) as usize);
        SPINNER[frame % SPINNER.len()]
    }

    fn render_status(&self) -> Vec<Line<'_>> {
        let (label, style) = match &self.status {
            AssistantStatus::Ready => ("READY", Style::new().fg(GREEN.into())),
            AssistantStatus::Loading => ("LOADING", Style::new().fg(AMBER.into()).bold()),
            AssistantStatus::Streaming => ("STREAMING", Style::new().fg(AMBER.into()).bold()),
            AssistantStatus::Error(_) => ("CONFIG", Style::new().fg(RED.into()).bold()),
        };
        let running = matches!(
            self.status,
            AssistantStatus::Loading | AssistantStatus::Streaming
        );
        let mut primary = vec![Span::styled(
            " AI COMMAND PLANE  ",
            Style::new().fg(AMBER.into()).bold(),
        )];
        if running {
            primary.push(Span::styled(
                format!("{} ", self.spinner()),
                Style::new().fg(AMBER.into()).bold(),
            ));
        }
        primary.push(Span::styled(label, style));
        if let Some(started) = self.started_at.filter(|_| running) {
            primary.push(Span::styled(
                format!("  {:.1}s", started.elapsed().as_secs_f32()),
                MUTED,
            ));
        }

        let usage = self.token_usage.unwrap_or_default();
        let secondary = vec![Span::styled(
            format!(
                " TOKENS  IN {}  ·  STREAMED {}  ·  TOTAL {}",
                usage.input_tokens, usage.output_tokens, usage.total_tokens
            ),
            MUTED,
        )];
        let mut tertiary = Vec::new();
        if let Some((tool, outcome)) = &self.tool_activity {
            let suffix = match outcome {
                None => " RUNNING",
                Some(true) => "",
                Some(false) => " FAILED",
            };
            tertiary.push(Span::styled(
                format!("  ·  TOOL {tool}{suffix}"),
                if *outcome == Some(false) {
                    Style::new().fg(RED.into())
                } else {
                    Style::new().fg(CYAN.into())
                },
            ));
        }
        tertiary.push(Span::styled(
            format!("  ·  MODEL {}", self.model_label),
            MUTED,
        ));
        vec![
            Line::from(primary),
            Line::from(secondary),
            Line::from(tertiary),
        ]
    }
}

fn bounded_text(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_owned();
    }
    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

impl Workspace for AssistantWorkspace {
    fn descriptor(&self) -> WorkspaceDescriptor {
        WorkspaceDescriptor {
            id: ID,
            label: "AI",
            hotkey: 'a',
            commands: &["AI", "ASK", "COPILOT"],
        }
    }

    fn is_favorite(&self) -> bool {
        true
    }

    fn handle_command(&mut self, invocation: &CommandInvocation) -> bool {
        if !invocation.args.is_empty() {
            self.input = invocation.args.join(" ");
            self.submit();
        }
        true
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        if self.composing {
            match key.code {
                KeyCode::Enter => self.submit(),
                KeyCode::Backspace => {
                    self.input.pop();
                }
                KeyCode::Esc => {
                    self.composing = false;
                    return false;
                }
                KeyCode::Char('l') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.messages.truncate(1);
                }
                KeyCode::Char(character)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                        && self.input.len() + character.len_utf8() <= MAX_INPUT_BYTES =>
                {
                    self.input.push(character);
                }
                _ => {}
            }
            return true;
        }

        match key.code {
            KeyCode::Enter | KeyCode::Char('i') => {
                self.composing = true;
                true
            }
            _ => false,
        }
    }

    fn handle_mouse(&mut self, event: MouseEvent, area: Rect) -> bool {
        let rows = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(4),
            Constraint::Length(3),
        ])
        .split(area);
        if is_primary_click(event, rows[2]) {
            self.composing = true;
            return true;
        }
        false
    }

    fn on_blur(&mut self) {
        self.composing = false;
    }

    fn on_focus(&mut self) {
        self.composing = true;
    }

    fn poll_intents(&mut self) -> Vec<AppIntent> {
        let updates = self
            .pending
            .as_ref()
            .map(|pending| pending.updates.try_iter().collect::<Vec<_>>())
            .unwrap_or_default();
        for update in updates {
            self.apply_stream_event(update);
        }

        let result = match self
            .pending
            .as_ref()
            .map(|pending| pending.result.try_recv())
        {
            Some(Ok(result)) => Some(result),
            Some(Err(mpsc::TryRecvError::Disconnected)) => Some(Err(AssistantError::Transport(
                "assistant worker disconnected".to_owned(),
            ))),
            Some(Err(mpsc::TryRecvError::Empty)) | None => None,
        };

        let Some(result) = result else {
            return Vec::new();
        };
        self.pending = None;
        self.started_at = None;
        match result {
            Ok(response) => {
                self.status = AssistantStatus::Ready;
                self.messages
                    .push(AssistantMessage::assistant(Self::response_text(&response)));
                self.streaming_text.clear();
                Self::intents(response.actions)
            }
            Err(error) => {
                self.status = AssistantStatus::Error(error.to_string());
                self.messages
                    .push(AssistantMessage::assistant(format!("ERROR: {error}")));
                self.streaming_text.clear();
                Vec::new()
            }
        }
    }

    fn update_shell_context(&mut self, context: &ShellContext) {
        self.active_workspace = context.active_workspace.as_str().to_owned();
        self.available_workspaces = context
            .workspace_order
            .iter()
            .map(|id| id.as_str().to_owned())
            .collect();
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let rows = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(4),
            Constraint::Length(3),
        ])
        .split(area);

        frame.render_widget(Paragraph::new(self.render_status()), rows[0]);

        let mut transcript = Vec::new();
        for message in &self.messages {
            let (label, style) = match message.role {
                AssistantRole::User => ("YOU", Style::new().fg(CYAN.into()).bold()),
                AssistantRole::Assistant => ("AI", Style::new().fg(AMBER.into()).bold()),
                AssistantRole::System => ("SYS", Style::new().fg(MUTED.into())),
            };
            transcript.push(Line::from(vec![
                Span::styled(format!("{label:<4}"), style),
                Span::styled(message.content.as_str(), INK),
            ]));
            transcript.push(Line::default());
        }
        if !self.streaming_text.is_empty() {
            transcript.push(Line::from(vec![
                Span::styled("AI  ", Style::new().fg(AMBER.into()).bold()),
                Span::styled(self.streaming_text.as_str(), INK),
                Span::styled("▌", AMBER),
            ]));
            transcript.push(Line::default());
        }
        let visible_height = rows[1].height.saturating_sub(2) as usize;
        let scroll = transcript.len().saturating_sub(visible_height) as u16;
        frame.render_widget(
            Paragraph::new(transcript)
                .wrap(Wrap { trim: false })
                .scroll((scroll, 0))
                .block(terminal_block("AI", "CONVERSATION")),
            rows[1],
        );

        let cursor = if self.pending.is_some() {
            "…"
        } else if self.composing {
            "█"
        } else {
            ""
        };
        let input = if self.composing || !self.input.is_empty() {
            self.input.as_str()
        } else {
            "PRESS I OR ENTER TO COMPOSE"
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" > ", Style::new().fg(CYAN.into()).bold()),
                Span::styled(input, if self.composing { INK } else { MUTED }),
                Span::styled(cursor, AMBER),
            ]))
            .style(Style::new().bg(BG.into()))
            .block(terminal_block("ASK", "NATURAL-LANGUAGE COMMAND")),
            rows[2],
        );

        let help = match &self.status {
            AssistantStatus::Error(message) => message.clone(),
            _ if self.composing => "ENTER SENDS  ·  ESC CLOSES DRAWER  ·  CTRL-L CLEARS HISTORY  ·  TRY: BRING PORTFOLIO FORWARD AND OPEN RISK".to_owned(),
            _ => "TYPE TO COMPOSE  ·  ESC CLOSES DRAWER  ·  CLICK OUTSIDE TO RETURN".to_owned(),
        };
        frame.render_widget(
            Paragraph::new(help)
                .style(Style::new().fg(MUTED.into()))
                .wrap(Wrap { trim: true }),
            rows[3],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use ratatui::{backend::TestBackend, Terminal};

    struct TestGateway;

    impl AssistantGateway for TestGateway {
        fn complete(
            &self,
            _request: AssistantRequest,
        ) -> Result<AssistantResponse, AssistantError> {
            Ok(AssistantResponse {
                content: "ok".to_owned(),
                actions: Vec::new(),
                model: Some("test".to_owned()),
            })
        }

        fn model_label(&self) -> &str {
            "TEST"
        }

        fn is_configured(&self) -> bool {
            true
        }

        fn configuration_hint(&self) -> &str {
            ""
        }
    }

    struct TestPortfolio;

    impl PortfolioRepository for TestPortfolio {
        fn load_portfolio(&self) -> crate::features::portfolio::PortfolioSnapshot {
            let usd = crate::foundation::Currency::new("USD").unwrap();
            let mut snapshot = crate::features::portfolio::PortfolioSnapshot::empty("TEST");
            snapshot
                .positions
                .push(crate::features::portfolio::Position {
                    instrument_id: crate::foundation::InstrumentId::new("us:xnas:aapl"),
                    account_id: crate::features::portfolio::PortfolioAccountId::new("ACCOUNT 1"),
                    symbol: "AAPL".to_owned(),
                    currency: usd,
                    quantity: crate::features::portfolio::PositionQuantity::from_scaled_units(
                        10_000_000,
                    ),
                    average_cost: Some(crate::foundation::Money::from_minor_units(15_000, usd)),
                    market_value: Some(crate::foundation::Money::from_minor_units(200_000, usd)),
                    unrealized_return_bps: Some(3_333),
                    weight_bps: Some(2_500),
                    cash: false,
                });
            snapshot
        }
    }

    struct CapturingGateway {
        seen: mpsc::SyncSender<crate::features::portfolio::PortfolioSnapshot>,
    }

    impl AssistantGateway for CapturingGateway {
        fn complete(&self, request: AssistantRequest) -> Result<AssistantResponse, AssistantError> {
            self.seen.send(request.portfolio).unwrap();
            Ok(AssistantResponse {
                content: "ok".to_owned(),
                actions: Vec::new(),
                model: Some("test".to_owned()),
            })
        }

        fn model_label(&self) -> &str {
            "TEST"
        }

        fn is_configured(&self) -> bool {
            true
        }

        fn configuration_hint(&self) -> &str {
            ""
        }
    }

    #[test]
    fn ui_actions_map_only_to_whitelisted_app_intents() {
        assert_eq!(
            AssistantWorkspace::intents(vec![
                UiAction::OpenWorkspace {
                    target: "portfolio".to_owned()
                },
                UiAction::RestoreLayout,
            ]),
            vec![
                AppIntent::ActivateWorkspace {
                    target: "portfolio".to_owned()
                },
                AppIntent::RestoreWorkspaceOrder,
            ]
        );
    }

    #[test]
    fn streaming_status_renders_a_spinner_live_tokens_and_tool_activity() {
        let mut workspace = AssistantWorkspace::new(
            Arc::new(TestGateway),
            Arc::new(TestPortfolio),
            vec!["overview".to_owned()],
        );
        workspace.status = AssistantStatus::Loading;
        workspace.started_at = Some(Instant::now());
        workspace.apply_stream_event(AssistantStreamEvent::TokenUsage(AssistantTokenUsage {
            input_tokens: 120,
            output_tokens: 17,
            total_tokens: 137,
            ..AssistantTokenUsage::default()
        }));
        workspace.apply_stream_event(AssistantStreamEvent::ToolStarted(
            "portfolio_get_positions".to_owned(),
        ));
        workspace.apply_stream_event(AssistantStreamEvent::TextDelta(
            "Your largest position is AAPL.".to_owned(),
        ));

        let mut terminal = Terminal::new(TestBackend::new(52, 30)).unwrap();
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

        assert!(rendered.contains("STREAMING"));
        assert!(rendered.contains("STREAMED 17"));
        assert!(rendered.contains("TOOL portfolio_get_positions RUNNING"));
        assert!(rendered.contains("Your largest position is AAPL."));
    }

    #[test]
    fn each_request_loads_the_same_portfolio_snapshot_as_the_portfolio_panel() {
        let (seen, received) = mpsc::sync_channel(1);
        let mut workspace = AssistantWorkspace::new(
            Arc::new(CapturingGateway { seen }),
            Arc::new(TestPortfolio),
            vec!["overview".to_owned()],
        );
        workspace.input = "analyze my positions".to_owned();
        workspace.submit();

        let portfolio = received
            .recv_timeout(std::time::Duration::from_secs(1))
            .expect("assistant request should receive the portfolio snapshot");
        assert_eq!(portfolio.positions[0].symbol, "AAPL");
        assert_eq!(portfolio.source, "TEST");
    }
}
