mod controls;
mod domain;
mod port;
mod sentiment;
mod workspace;

pub use domain::{
    ArticleBodyState, EventImportance, Headline, NewsEvent, NewsFilter, NewsFreshness,
    NewsProvenance, NewsSnapshot, NewsStory, NewsWorkbench,
};
pub use port::{NewsArticleOpenError, NewsArticleOpener, NewsFeed};
pub use sentiment::{
    analyze_news_sentiment, NewsSentiment, NewsSentimentEvidence, NewsSentimentLabel,
    SentimentPolarity,
};
pub use workspace::NewsWorkspace;

use crate::app::WorkspaceId;

pub const ID: WorkspaceId = WorkspaceId::new("news");
