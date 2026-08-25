mod domain;
mod port;
mod workspace;

pub use domain::{
    EventImportance, Headline, NewsEvent, NewsFilter, NewsSnapshot, NewsStory, NewsWorkbench,
};
pub use port::NewsQuery;
pub use workspace::NewsWorkspace;

use crate::app::WorkspaceId;

pub const ID: WorkspaceId = WorkspaceId::new("news");
