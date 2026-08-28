mod controls;
mod domain;
mod port;
mod workspace;

pub use domain::{
    ArticleBodyState, EventImportance, Headline, NewsEvent, NewsFilter, NewsSnapshot, NewsStory,
    NewsWorkbench,
};
pub use port::{NewsArticleOpenError, NewsArticleOpener, NewsFeed};
pub use workspace::NewsWorkspace;

use crate::app::WorkspaceId;

pub const ID: WorkspaceId = WorkspaceId::new("news");
