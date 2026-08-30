pub mod domain;
mod port;
mod workspace;

pub use port::{AssistantContextQuery, AssistantError, AssistantGateway};
pub use workspace::AssistantWorkspace;

use crate::app::WorkspaceId;

pub const ID: WorkspaceId = WorkspaceId::new("assistant");
