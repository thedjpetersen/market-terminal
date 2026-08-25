use std::fmt;

use super::{ChatEndpoint, ChatEvent, ChatValidationError};

pub trait ChatGateway: Send + Sync {
    fn endpoint(&self) -> ChatEndpoint;
    fn drain_events(&self) -> Vec<ChatEvent>;
    fn send_message(&self, message: &str) -> Result<(), ChatGatewayError>;
    fn reconnect(&self) -> Result<(), ChatGatewayError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatGatewayError {
    Disabled,
    Busy,
    Disconnected,
    Validation(ChatValidationError),
}

impl fmt::Display for ChatGatewayError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Disabled => formatter.write_str("IRC is not configured"),
            Self::Busy => formatter.write_str("IRC outbound queue is full"),
            Self::Disconnected => formatter.write_str("IRC worker is disconnected"),
            Self::Validation(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ChatGatewayError {}

impl From<ChatValidationError> for ChatGatewayError {
    fn from(error: ChatValidationError) -> Self { Self::Validation(error) }
}
