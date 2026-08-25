mod domain;
mod port;
mod workspace;

pub use domain::{Headline, NewsSnapshot};
pub use port::NewsQuery;
pub use workspace::NewsWorkspace;

use crate::app::WorkspaceId;

pub const ID: WorkspaceId = WorkspaceId::new("news");
