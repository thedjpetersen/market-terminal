use crate::features::portfolio::{PortfolioActivityLedger, PortfolioSnapshot};

pub(crate) const COMMAND_PLANE_SYSTEM_PROMPT: &str = "You are the command plane for a native financial terminal. \
Answer financial and product questions concisely. Use the supplied Market Terminal tools when the answer \
depends on the current interface or portfolio, and use only those tools when the user asks to change the UI. \
Never invent a workspace, command, position, or account value. Prefer the supplied bring-forward action \
when the user asks to prioritize, foreground, rearrange, or put a feature first. Prefer the supplied \
open-workspace action when they only ask to view a feature. Do not invoke shell, filesystem, web, \
connector, MCP, or any other agent tools. Portfolio access is read-only: you cannot access credentials, \
submit trades, or perform external side effects.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssistantRole {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssistantMessage {
    pub role: AssistantRole,
    pub content: String,
}

impl AssistantMessage {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: AssistantRole::User,
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: AssistantRole::Assistant,
            content: content.into(),
        }
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: AssistantRole::System,
            content: content.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiAction {
    OpenWorkspace { target: String },
    BringForward { target: String },
    RunCommand { command: String },
    RestoreLayout,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssistantRequest {
    pub messages: Vec<AssistantMessage>,
    pub active_workspace: String,
    pub available_workspaces: Vec<String>,
    pub portfolio: PortfolioSnapshot,
    pub activity: PortfolioActivityLedger,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssistantResponse {
    pub content: String,
    pub actions: Vec<UiAction>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AssistantTokenUsage {
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_output_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssistantStreamEvent {
    TextDelta(String),
    TokenUsage(AssistantTokenUsage),
    ToolStarted(String),
    ToolFinished { name: String, success: bool },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssistantStatus {
    Ready,
    Loading,
    Streaming,
    Error(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_constructors_preserve_roles() {
        assert_eq!(
            AssistantMessage::user("show risk").role,
            AssistantRole::User
        );
        assert_eq!(
            AssistantMessage::assistant("done").role,
            AssistantRole::Assistant
        );
        assert_eq!(
            AssistantMessage::system("rules").role,
            AssistantRole::System
        );
    }
}
