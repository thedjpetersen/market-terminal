use std::{
    sync::{mpsc, Arc},
    thread,
};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
    Frame,
};

use crate::{
    app::{AppIntent, CommandInvocation, ShellContext, Workspace, WorkspaceDescriptor},
    ui::{
        components::terminal_block,
        theme::{AMBER, BG, CYAN, GREEN, INK, MUTED, RED},
    },
};

use super::{
    domain::{
        AssistantMessage, AssistantRequest, AssistantResponse, AssistantRole, AssistantStatus,
        UiAction,
    },
    AssistantError, AssistantGateway, ID,
};

type PendingResponse = mpsc::Receiver<Result<AssistantResponse, AssistantError>>;
const MAX_INPUT_BYTES: usize = 4_096;

pub struct AssistantWorkspace {
    gateway: Arc<dyn AssistantGateway>,
    model_label: String,
    available_workspaces: Vec<String>,
    active_workspace: String,
    messages: Vec<AssistantMessage>,
    input: String,
    status: AssistantStatus,
    pending: Option<PendingResponse>,
}

impl AssistantWorkspace {
    pub fn new(
        gateway: Arc<dyn AssistantGateway>,
        available_workspaces: Vec<String>,
    ) -> Self {
        let model_label = gateway.model_label().to_owned();
        let status = if gateway.is_configured() {
            AssistantStatus::Ready
        } else {
            AssistantStatus::Error("SET OPENROUTER_API_KEY TO ENABLE AI".to_owned())
        };
        Self {
            gateway,
            model_label,
            available_workspaces,
            active_workspace: "overview".to_owned(),
            messages: vec![AssistantMessage::system(
                "ASK FOR ANALYSIS OR TELL ME WHICH WORKSPACE TO OPEN OR BRING FORWARD.",
            )],
            input: String::new(),
            status,
            pending: None,
        }
    }

    fn submit(&mut self) {
        let prompt = self.input.trim().to_owned();
        if prompt.is_empty() || self.pending.is_some() {
            return;
        }

        self.messages.push(AssistantMessage::user(prompt));
        self.input.clear();
        self.status = AssistantStatus::Thinking;

        let history_start = self.messages.len().saturating_sub(10);
        let request = AssistantRequest {
            messages: self.messages[history_start..].to_vec(),
            active_workspace: self.active_workspace.clone(),
            available_workspaces: self.available_workspaces.clone(),
        };
        let gateway = Arc::clone(&self.gateway);
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let _ = sender.send(gateway.complete(request));
        });
        self.pending = Some(receiver);
    }

    fn response_text(response: &AssistantResponse) -> String {
        if !response.content.trim().is_empty() {
            return response.content.trim().to_owned();
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
                UiAction::BringForward { target } => {
                    AppIntent::BringWorkspaceForward { target }
                }
                UiAction::RunCommand { command } => {
                    AppIntent::DispatchCommand { command, origin: ID }
                }
                UiAction::RestoreLayout => AppIntent::RestoreWorkspaceOrder,
            })
            .collect()
    }

    fn render_status(&self) -> Line<'_> {
        let (label, style) = match &self.status {
            AssistantStatus::Ready => ("READY", Style::new().fg(GREEN)),
            AssistantStatus::Thinking => ("THINKING", Style::new().fg(AMBER).bold()),
            AssistantStatus::Error(_) => ("CONFIG", Style::new().fg(RED).bold()),
        };
        Line::from(vec![
            Span::styled(" AI COMMAND PLANE  ", Style::new().fg(AMBER).bold()),
            Span::styled(label, style),
            Span::styled(format!("  MODEL {}", self.model_label), MUTED),
        ])
    }
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

    fn is_favorite(&self) -> bool { true }

    fn handle_command(&mut self, invocation: &CommandInvocation) -> bool {
        if !invocation.args.is_empty() {
            self.input = invocation.args.join(" ");
            self.submit();
        }
        true
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Enter => self.submit(),
            KeyCode::Backspace => {
                self.input.pop();
            }
            KeyCode::Esc => self.input.clear(),
            KeyCode::Char('l') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.messages.truncate(1);
            }
            KeyCode::Char(character)
                if !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                    && self.input.len() < MAX_INPUT_BYTES =>
            {
                self.input.push(character);
            }
            _ => {}
        }
        true
    }

    fn poll_intents(&mut self) -> Vec<AppIntent> {
        let result = match self.pending.as_ref().map(PendingResponse::try_recv) {
            Some(Ok(result)) => Some(result),
            Some(Err(mpsc::TryRecvError::Disconnected)) => Some(Err(
                AssistantError::Transport("assistant worker disconnected".to_owned()),
            )),
            Some(Err(mpsc::TryRecvError::Empty)) | None => None,
        };

        let Some(result) = result else {
            return Vec::new();
        };
        self.pending = None;
        match result {
            Ok(response) => {
                self.status = AssistantStatus::Ready;
                self.messages.push(AssistantMessage::assistant(Self::response_text(&response)));
                Self::intents(response.actions)
            }
            Err(error) => {
                self.status = AssistantStatus::Error(error.to_string());
                self.messages.push(AssistantMessage::assistant(format!("ERROR: {error}")));
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
            Constraint::Length(2),
            Constraint::Min(8),
            Constraint::Length(4),
            Constraint::Length(3),
        ])
        .split(area);

        frame.render_widget(Paragraph::new(self.render_status()), rows[0]);

        let mut transcript = Vec::new();
        for message in &self.messages {
            let (label, style) = match message.role {
                AssistantRole::User => ("YOU", Style::new().fg(CYAN).bold()),
                AssistantRole::Assistant => ("AI", Style::new().fg(AMBER).bold()),
                AssistantRole::System => ("SYS", Style::new().fg(MUTED)),
            };
            transcript.push(Line::from(vec![
                Span::styled(format!("{label:<4}"), style),
                Span::styled(message.content.as_str(), INK),
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

        let cursor = if self.pending.is_some() { "…" } else { "█" };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" > ", Style::new().fg(CYAN).bold()),
                Span::styled(self.input.as_str(), INK),
                Span::styled(cursor, AMBER),
            ]))
            .style(Style::new().bg(BG))
            .block(terminal_block("ASK", "NATURAL-LANGUAGE COMMAND")),
            rows[2],
        );

        let help = match &self.status {
            AssistantStatus::Error(message) => format!(
                "{message}  ·  EXPORT OPENROUTER_API_KEY=...  ·  OPTIONAL OPENROUTER_MODEL=openrouter/auto"
            ),
            _ => "ENTER SENDS  ·  ESC CLEARS  ·  CTRL-L CLEARS HISTORY  ·  TRY: BRING PORTFOLIO FORWARD AND OPEN RISK".to_owned(),
        };
        frame.render_widget(
            Paragraph::new(help)
                .style(Style::new().fg(MUTED))
                .wrap(Wrap { trim: true }),
            rows[3],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_actions_map_only_to_whitelisted_app_intents() {
        assert_eq!(
            AssistantWorkspace::intents(vec![
                UiAction::OpenWorkspace { target: "portfolio".to_owned() },
                UiAction::RestoreLayout,
            ]),
            vec![
                AppIntent::ActivateWorkspace { target: "portfolio".to_owned() },
                AppIntent::RestoreWorkspaceOrder,
            ]
        );
    }
}
