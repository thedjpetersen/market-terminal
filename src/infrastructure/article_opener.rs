use std::process::Command;

use crate::features::{
    news::{NewsArticleOpenError, NewsArticleOpener},
    security::{SecurityDocumentOpenError, SecurityDocumentOpener},
};

#[derive(Debug, Default)]
pub struct SystemNewsArticleOpener;

impl NewsArticleOpener for SystemNewsArticleOpener {
    fn open(&self, url: &str) -> Result<(), NewsArticleOpenError> {
        let parsed = reqwest::Url::parse(url).map_err(|_| NewsArticleOpenError::InvalidUrl)?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(NewsArticleOpenError::UnsupportedScheme(
                parsed.scheme().to_owned(),
            ));
        }
        if parsed.host_str().is_none() {
            return Err(NewsArticleOpenError::InvalidUrl);
        }

        launch(url).map_err(|error| NewsArticleOpenError::Launch(error.to_string()))?;
        Ok(())
    }
}

impl SecurityDocumentOpener for SystemNewsArticleOpener {
    fn open(&self, url: &str) -> Result<(), SecurityDocumentOpenError> {
        let parsed = reqwest::Url::parse(url).map_err(|_| SecurityDocumentOpenError::InvalidUrl)?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(SecurityDocumentOpenError::UnsupportedScheme(
                parsed.scheme().to_owned(),
            ));
        }
        if parsed.host_str().is_none() {
            return Err(SecurityDocumentOpenError::InvalidUrl);
        }
        launch(url).map_err(|error| SecurityDocumentOpenError::Launch(error.to_string()))
    }
}

#[cfg(target_os = "macos")]
fn launch(url: &str) -> std::io::Result<()> {
    Command::new("open").arg(url).spawn().map(|_| ())
}

#[cfg(target_os = "linux")]
fn launch(url: &str) -> std::io::Result<()> {
    Command::new("xdg-open").arg(url).spawn().map(|_| ())
}

#[cfg(target_os = "windows")]
fn launch(url: &str) -> std::io::Result<()> {
    Command::new("cmd")
        .args(["/C", "start", "", url])
        .spawn()
        .map(|_| ())
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn launch(_url: &str) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "opening a browser is not supported on this platform",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_web_urls_before_launching() {
        let opener = SystemNewsArticleOpener;
        assert_eq!(
            NewsArticleOpener::open(&opener, "javascript:alert(1)"),
            Err(NewsArticleOpenError::UnsupportedScheme(
                "javascript".to_owned()
            ))
        );
        assert_eq!(
            NewsArticleOpener::open(&opener, "file:///tmp/story.html"),
            Err(NewsArticleOpenError::UnsupportedScheme("file".to_owned()))
        );
        assert_eq!(
            NewsArticleOpener::open(&opener, "not a url"),
            Err(NewsArticleOpenError::InvalidUrl)
        );
        assert_eq!(
            SecurityDocumentOpener::open(&opener, "file:///tmp/filing.html"),
            Err(SecurityDocumentOpenError::UnsupportedScheme(
                "file".to_owned()
            ))
        );
    }
}
