use std::fmt;

use super::{NewsEvent, NewsSnapshot, NewsWorkbench};

pub trait NewsFeed: Send + Sync {
    fn load_news(&self) -> NewsSnapshot;

    /// Adapters may override this with full text, calendars, and entitlement-
    /// aware results. The default keeps offline and test adapters useful.
    fn load_workbench(&self) -> NewsWorkbench {
        NewsWorkbench::from_snapshot(self.load_news())
    }

    /// Returns only provider-backed calendar events. The default is empty so
    /// consumers never mistake the deterministic News gallery calendar for a
    /// live external source.
    fn load_events(&self) -> Vec<NewsEvent> {
        Vec::new()
    }

    fn status(&self) -> String {
        "DETERMINISTIC DEMO FEED".to_owned()
    }

    fn request_refresh(&self) {}

    /// Requests an on-demand, transient article-body download. Returns true
    /// when the adapter accepted the request for background processing.
    fn request_article(&self, _story_id: &str, _url: &str) -> bool {
        false
    }
}

/// Opens a publisher-owned article outside the terminal.
///
/// Keeping this behind a news-owned port lets the workspace remain testable
/// and leaves URL validation and operating-system integration to adapters.
pub trait NewsArticleOpener: Send + Sync {
    fn open(&self, url: &str) -> Result<(), NewsArticleOpenError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NewsArticleOpenError {
    InvalidUrl,
    UnsupportedScheme(String),
    Launch(String),
}

impl fmt::Display for NewsArticleOpenError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUrl => write!(formatter, "publisher link is not a valid URL"),
            Self::UnsupportedScheme(scheme) => {
                write!(formatter, "publisher link uses unsupported {scheme} scheme")
            }
            Self::Launch(message) => write!(formatter, "could not open publisher link: {message}"),
        }
    }
}

impl std::error::Error for NewsArticleOpenError {}
