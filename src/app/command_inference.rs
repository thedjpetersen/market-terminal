#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandInferenceRequest {
    pub input: String,
    pub active_workspace: String,
    pub available_workspaces: Vec<String>,
    pub available_commands: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandInferenceError {
    NotConfigured(String),
    Provider(String),
    InvalidResponse(String),
}

impl std::fmt::Display for CommandInferenceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotConfigured(message)
            | Self::Provider(message)
            | Self::InvalidResponse(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for CommandInferenceError {}

/// A model-selected command inferred only after exact command resolution fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InferredCommand {
    command: String,
    model: String,
}

impl InferredCommand {
    pub fn new(command: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            model: model.into(),
        }
    }

    pub fn command(&self) -> &str {
        &self.command
    }

    pub fn model(&self) -> &str {
        &self.model
    }
}

/// Uses an AI provider to infer a command from otherwise unmatched command-bar
/// text.
///
/// The shell calls this port from a background worker. Exact command resolution
/// always runs before this port is consulted, and the returned command is still
/// validated against the registry before dispatch.
pub trait CommandInference: Send + Sync {
    fn infer(
        &self,
        request: CommandInferenceRequest,
    ) -> Result<InferredCommand, CommandInferenceError>;
}
