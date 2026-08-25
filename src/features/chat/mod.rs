mod domain;
mod port;
mod workspace;

pub use domain::{
    validate_chat_message, ChatConnectionState, ChatEndpoint, ChatEvent, ChatMessage,
    ChatMessageKind, ChatValidationError, MAX_CHAT_EVENTS_PER_POLL, MAX_CHAT_MESSAGE_BYTES,
    MAX_CHAT_MESSAGES,
};
pub use port::{ChatGateway, ChatGatewayError};
pub use workspace::ChatWorkspace;

use crate::app::WorkspaceId;

pub const ID: WorkspaceId = WorkspaceId::new("chat");
