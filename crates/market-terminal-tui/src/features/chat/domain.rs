use std::fmt;

pub const MAX_CHAT_MESSAGES: usize = 500;
pub const MAX_CHAT_MESSAGE_BYTES: usize = 400;
pub const MAX_CHAT_EVENTS_PER_POLL: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatEndpoint {
    pub server: String,
    pub port: u16,
    pub tls: bool,
    pub nickname: String,
    pub channel: String,
    pub configured: bool,
}

impl ChatEndpoint {
    pub fn offline() -> Self {
        Self {
            server: "NOT CONFIGURED".to_owned(),
            port: 0,
            tls: true,
            nickname: "market-terminal".to_owned(),
            channel: "#market-terminal".to_owned(),
            configured: false,
        }
    }

    pub fn validate(&self) -> Result<(), ChatValidationError> {
        if !self.configured {
            return Ok(());
        }
        if self.server.trim().is_empty() || self.server.len() > 253 {
            return Err(ChatValidationError::InvalidServer);
        }
        if self.port == 0 {
            return Err(ChatValidationError::InvalidPort);
        }
        if self.nickname.is_empty()
            || self.nickname.len() > 32
            || !self.nickname.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '[' | ']')
            })
        {
            return Err(ChatValidationError::InvalidNickname);
        }
        if self.channel.len() < 2
            || self.channel.len() > 64
            || !matches!(self.channel.as_bytes().first().copied(), Some(b'#' | b'&'))
            || self.channel.chars().any(char::is_whitespace)
        {
            return Err(ChatValidationError::InvalidChannel);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatConnectionState {
    Disabled,
    Connecting,
    Connected,
    Reconnecting,
    Disconnected,
}

impl ChatConnectionState {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Disabled => "OFFLINE",
            Self::Connecting => "CONNECTING",
            Self::Connected => "CONNECTED",
            Self::Reconnecting => "RECONNECTING",
            Self::Disconnected => "DISCONNECTED",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatMessageKind {
    Message,
    Action,
    Notice,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    pub sequence: u64,
    pub time: String,
    pub sender: String,
    pub target: String,
    pub body: String,
    pub kind: ChatMessageKind,
    pub own: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatEvent {
    State(ChatConnectionState),
    Message(ChatMessage),
    ParticipantJoined(String),
    ParticipantLeft(String),
    Status(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatValidationError {
    InvalidServer,
    InvalidPort,
    InvalidNickname,
    InvalidChannel,
    EmptyMessage,
    MessageTooLong,
}

impl fmt::Display for ChatValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidServer => "IRC server is invalid",
            Self::InvalidPort => "IRC port is invalid",
            Self::InvalidNickname => "IRC nickname is invalid",
            Self::InvalidChannel => "IRC channel is invalid",
            Self::EmptyMessage => "chat message cannot be empty",
            Self::MessageTooLong => "chat message exceeds 400 bytes",
        })
    }
}

impl std::error::Error for ChatValidationError {}

pub fn validate_chat_message(message: &str) -> Result<&str, ChatValidationError> {
    let message = message.trim();
    if message.is_empty() {
        return Err(ChatValidationError::EmptyMessage);
    }
    if message.len() > MAX_CHAT_MESSAGE_BYTES
        || message
            .chars()
            .any(|character| matches!(character, '\r' | '\n' | '\0'))
    {
        return Err(ChatValidationError::MessageTooLong);
    }
    Ok(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_and_message_inputs_are_bounded() {
        let endpoint = ChatEndpoint {
            server: "irc.libera.chat".to_owned(),
            port: 6697,
            tls: true,
            nickname: "market-terminal".to_owned(),
            channel: "#markets".to_owned(),
            configured: true,
        };
        assert_eq!(endpoint.validate(), Ok(()));
        assert_eq!(validate_chat_message(" hello "), Ok("hello"));
        assert_eq!(
            validate_chat_message(&"x".repeat(MAX_CHAT_MESSAGE_BYTES + 1)),
            Err(ChatValidationError::MessageTooLong)
        );
    }

    #[test]
    fn unsafe_irc_line_breaks_are_rejected() {
        assert_eq!(
            validate_chat_message("hello\r\nJOIN #other"),
            Err(ChatValidationError::MessageTooLong)
        );
    }
}
