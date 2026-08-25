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
