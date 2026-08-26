use std::{collections::BTreeSet, sync::Arc};

use crossterm::event::{KeyCode, KeyEvent, MouseEvent};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{List, ListItem, Paragraph, Wrap},
    Frame,
};

use crate::{
    app::{CommandInvocation, Workspace, WorkspaceDescriptor},
    ui::{
        components::terminal_block,
        is_primary_click,
        theme::{AMBER, BG, CYAN, GREEN, INK, MUTED, RED, YELLOW},
    },
};

use super::{
    ChatConnectionState, ChatEndpoint, ChatEvent, ChatGateway, ChatMessage, ChatMessageKind, ID,
    MAX_CHAT_MESSAGE_BYTES, MAX_CHAT_MESSAGES,
};

pub struct ChatWorkspace {
    gateway: Arc<dyn ChatGateway>,
    endpoint: ChatEndpoint,
    connection: ChatConnectionState,
    messages: Vec<ChatMessage>,
    participants: BTreeSet<String>,
    draft: String,
    composing: bool,
    status: String,
}

impl ChatWorkspace {
    pub fn new(gateway: Arc<dyn ChatGateway>) -> Self {
        let endpoint = gateway.endpoint();
        let connection = if endpoint.configured {
            ChatConnectionState::Connecting
        } else {
            ChatConnectionState::Disabled
        };
        let status = if endpoint.configured {
            format!("CONNECTING TO {}:{}", endpoint.server, endpoint.port)
        } else {
            "SET IRC_SERVER AND IRC_CHANNEL TO CONNECT".to_owned()
        };
        Self {
            gateway,
            endpoint,
            connection,
            messages: Vec::new(),
            participants: BTreeSet::new(),
            draft: String::new(),
            composing: false,
            status,
        }
    }

    fn poll_gateway(&mut self) {
        for event in self.gateway.drain_events() {
            match event {
                ChatEvent::State(state) => self.connection = state,
                ChatEvent::Message(message) => {
                    if !message.sender.is_empty() {
                        self.participants.insert(message.sender.clone());
                    }
                    self.messages.push(message);
                    if self.messages.len() > MAX_CHAT_MESSAGES {
                        let overflow = self.messages.len() - MAX_CHAT_MESSAGES;
                        self.messages.drain(..overflow);
                    }
                }
                ChatEvent::ParticipantJoined(nickname) => {
                    self.participants.insert(nickname);
                }
                ChatEvent::ParticipantLeft(nickname) => {
                    self.participants.remove(&nickname);
                }
                ChatEvent::Status(status) => self.status = status,
            }
        }
    }

    fn submit(&mut self) {
        match self.gateway.send_message(&self.draft) {
            Ok(()) => {
                self.draft.clear();
                self.status = "MESSAGE QUEUED".to_owned();
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    fn append(&mut self, character: char) {
        if self.draft.len() + character.len_utf8() <= MAX_CHAT_MESSAGE_BYTES {
            self.draft.push(character);
        }
    }
}

impl Workspace for ChatWorkspace {
    fn descriptor(&self) -> WorkspaceDescriptor {
        WorkspaceDescriptor {
            id: ID,
            label: "CHAT",
            hotkey: 'h',
            commands: &["CHAT", "IRC"],
        }
    }

    fn is_favorite(&self) -> bool { true }

    fn handle_command(&mut self, invocation: &CommandInvocation) -> bool {
        if !invocation.args.is_empty() {
            self.draft = invocation.args.join(" ");
            let boundary = self
                .draft
                .char_indices()
                .map(|(index, _)| index)
                .take_while(|index| *index <= MAX_CHAT_MESSAGE_BYTES)
                .last()
                .unwrap_or_default();
            if self.draft.len() > MAX_CHAT_MESSAGE_BYTES {
                self.draft.truncate(boundary);
            }
            self.composing = true;
        }
        true
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        if self.composing {
            match key.code {
                KeyCode::Esc => self.composing = false,
                KeyCode::Enter => self.submit(),
                KeyCode::Backspace => {
                    self.draft.pop();
                }
                KeyCode::Char(character) => self.append(character),
                _ => {}
            }
            return true;
        }
        match key.code {
            KeyCode::Char('i') | KeyCode::Enter => {
                self.composing = true;
                true
            }
            KeyCode::Char('r') => {
                self.status = match self.gateway.reconnect() {
                    Ok(()) => "RECONNECT REQUESTED".to_owned(),
                    Err(error) => error.to_string(),
                };
                true
            }
            _ => false,
        }
    }

    fn handle_mouse(&mut self, event: MouseEvent, area: Rect) -> bool {
        let sections = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(3),
        ])
        .split(area);
        if is_primary_click(event, sections[2]) {
            self.composing = true;
            return true;
        }
        false
    }

    fn on_blur(&mut self) { self.composing = false; }

    fn poll_intents(&mut self) -> Vec<crate::app::AppIntent> {
        self.poll_gateway();
        Vec::new()
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let sections = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(3),
        ])
        .split(area);
        let connection_style = match self.connection {
            ChatConnectionState::Connected => GREEN,
            ChatConnectionState::Connecting | ChatConnectionState::Reconnecting => YELLOW,
            ChatConnectionState::Disabled | ChatConnectionState::Disconnected => RED,
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    format!(" {} ", self.endpoint.channel),
                    Style::new().bg(AMBER).fg(BG).bold(),
                ),
                Span::styled(format!(" {}  ", self.connection.label()), connection_style),
                Span::styled(
                    format!(
                        "{}:{} · {} · {}",
                        self.endpoint.server,
                        self.endpoint.port,
                        if self.endpoint.tls { "TLS" } else { "PLAINTEXT" },
                        self.endpoint.nickname,
                    ),
                    MUTED,
                ),
                Span::styled(format!("  {}", self.status), INK),
            ]))
            .block(terminal_block("IRC", "MARKET CHAT")),
            sections[0],
        );

        let columns = Layout::horizontal([
            Constraint::Length(20),
            Constraint::Min(40),
            Constraint::Length(24),
        ])
        .split(sections[1]);
        let channels = vec![
            ListItem::new(Line::styled(self.endpoint.channel.clone(), CYAN)),
            ListItem::new(Line::styled("", MUTED)),
            ListItem::new(Line::styled("IRC NETWORK", AMBER)),
            ListItem::new(Line::styled(self.endpoint.server.clone(), MUTED)),
        ];
        frame.render_widget(
            List::new(channels).block(terminal_block("CH", "CHANNELS")),
            columns[0],
        );

        let visible_height = usize::from(columns[1].height.saturating_sub(2));
        let start = self.messages.len().saturating_sub(visible_height);
        let lines = self.messages[start..]
            .iter()
            .map(|message| {
                let sender_style = if message.own { GREEN } else { CYAN };
                let body_style = match message.kind {
                    ChatMessageKind::System | ChatMessageKind::Notice => MUTED,
                    ChatMessageKind::Action => YELLOW,
                    ChatMessageKind::Message => INK,
                };
                Line::from(vec![
                    Span::styled(format!("{} ", message.time), MUTED),
                    Span::styled(format!("{:<13}", message.sender), sender_style),
                    Span::styled(message.body.clone(), body_style),
                ])
            })
            .collect::<Vec<_>>();
        frame.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .block(terminal_block("MSG", "CONVERSATION")),
            columns[1],
        );

        let participants = self
            .participants
            .iter()
            .map(|nickname| ListItem::new(Line::styled(format!("● {nickname}"), GREEN)))
            .collect::<Vec<_>>();
        frame.render_widget(
            List::new(participants).block(terminal_block("WHO", "PARTICIPANTS")),
            columns[2],
        );

        let prompt_style = if self.composing { CYAN } else { MUTED };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(if self.composing { " MESSAGE › " } else { " I/ENTER " }, AMBER),
                Span::styled(
                    if self.draft.is_empty() && !self.composing {
                        "COMPOSE · R RECONNECT"
                    } else {
                        self.draft.as_str()
                    },
                    prompt_style,
                ),
                Span::styled(if self.composing { "█" } else { "" }, CYAN),
            ]))
            .block(terminal_block("SEND", "IRC MESSAGE · ENTER SEND · ESC EXIT INPUT")),
            sections[2],
        );
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::features::chat::ChatGatewayError;

    struct StubGateway {
        sent: Mutex<Vec<String>>,
    }

    impl ChatGateway for StubGateway {
        fn endpoint(&self) -> ChatEndpoint { ChatEndpoint::offline() }
        fn drain_events(&self) -> Vec<ChatEvent> { Vec::new() }
        fn send_message(&self, message: &str) -> Result<(), ChatGatewayError> {
            self.sent.lock().expect("sent lock").push(message.to_owned());
            Ok(())
        }
        fn reconnect(&self) -> Result<(), ChatGatewayError> { Ok(()) }
    }

    #[test]
    fn composer_captures_keys_and_sends_without_shell_access() {
        let gateway = Arc::new(StubGateway { sent: Mutex::new(Vec::new()) });
        let mut workspace = ChatWorkspace::new(gateway.clone());
        assert!(workspace.handle_key(KeyEvent::new(
            KeyCode::Char('i'),
            crossterm::event::KeyModifiers::NONE,
        )));
        for character in "hello desk".chars() {
            workspace.handle_key(KeyEvent::new(
                KeyCode::Char(character),
                crossterm::event::KeyModifiers::NONE,
            ));
        }
        workspace.handle_key(KeyEvent::new(
            KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        ));
        assert_eq!(
            *gateway.sent.lock().expect("sent lock"),
            vec!["hello desk".to_owned()]
        );
    }
}
