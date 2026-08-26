pub(crate) const COMMAND_PLANE_SYSTEM_PROMPT: &str = "You are the command plane for a native financial terminal. \
Answer financial and product questions concisely. When the user asks to change the terminal UI, \
request only the supplied actions. Never invent a workspace or command. Prefer bring_workspace_forward \
when the user asks to prioritize, foreground, rearrange, or put a feature first. Prefer open_workspace \
when they only ask to view a feature. Do not invoke shell, filesystem, web, connector, or other agent tools. \
You cannot access credentials, trade, or perform external side effects.";

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
        Self { role: AssistantRole::User, content: content.into() }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self { role: AssistantRole::Assistant, content: content.into() }
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self { role: AssistantRole::System, content: content.into() }
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssistantResponse {
    pub content: String,
    pub actions: Vec<UiAction>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssistantStatus {
    Ready,
    Thinking,
    Error(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_constructors_preserve_roles() {
        assert_eq!(AssistantMessage::user("show risk").role, AssistantRole::User);
        assert_eq!(AssistantMessage::assistant("done").role, AssistantRole::Assistant);
        assert_eq!(AssistantMessage::system("rules").role, AssistantRole::System);
    }
}
