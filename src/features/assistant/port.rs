use super::domain::{AssistantRequest, AssistantResponse};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssistantError {
    NotConfigured,
    Transport(String),
    Provider(String),
    InvalidResponse(String),
}

impl std::fmt::Display for AssistantError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotConfigured => write!(formatter, "OPENROUTER_API_KEY is not configured"),
            Self::Transport(message) => write!(formatter, "network error: {message}"),
            Self::Provider(message) => write!(formatter, "provider error: {message}"),
            Self::InvalidResponse(message) => write!(formatter, "invalid response: {message}"),
        }
    }
}

impl std::error::Error for AssistantError {}

pub trait AssistantGateway: Send + Sync {
    fn complete(&self, request: AssistantRequest) -> Result<AssistantResponse, AssistantError>;
    fn model_label(&self) -> &str;
    fn is_configured(&self) -> bool;
}
