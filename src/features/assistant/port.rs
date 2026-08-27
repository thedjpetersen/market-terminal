use std::sync::mpsc::Sender;

use super::domain::{AssistantRequest, AssistantResponse, AssistantStreamEvent};

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
            Self::NotConfigured => write!(formatter, "AI provider is not configured"),
            Self::Transport(message) => write!(formatter, "transport error: {message}"),
            Self::Provider(message) => write!(formatter, "provider error: {message}"),
            Self::InvalidResponse(message) => write!(formatter, "invalid response: {message}"),
        }
    }
}

impl std::error::Error for AssistantError {}

pub trait AssistantGateway: Send + Sync {
    fn complete(&self, request: AssistantRequest) -> Result<AssistantResponse, AssistantError>;

    fn complete_stream(
        &self,
        request: AssistantRequest,
        updates: Sender<AssistantStreamEvent>,
    ) -> Result<AssistantResponse, AssistantError> {
        let response = self.complete(request)?;
        if !response.content.is_empty() {
            let _ = updates.send(AssistantStreamEvent::TextDelta(response.content.clone()));
        }
        Ok(response)
    }

    fn model_label(&self) -> &str;
    fn is_configured(&self) -> bool;
    fn configuration_hint(&self) -> &str;
}
