use std::{
    collections::HashSet,
    env,
    io::Read,
    sync::{Arc, Mutex, RwLock, mpsc},
    thread,
    time::Duration,
};

use chrono::{DateTime, Utc};
use feed_rs::{model::Entry, parser};
use reqwest::blocking::Client;

use crate::{
    features::news::{Headline, NewsFeed, NewsSnapshot, NewsStory, NewsWorkbench},
    foundation::InstrumentId,
};

const DEFAULT_REFRESH_SECONDS: u64 = 300;
const DEFAULT_TIMEOUT_SECONDS: u64 = 12;
const MAX_FEED_BYTES: u64 = 2 * 1024 * 1024;
const MAX_STORIES: usize = 80;
type TimestampedStory = (Option<DateTime<Utc>>, NewsStory);
const DEFAULT_FEEDS: [&str; 3] = [
    "https://www.cnbc.com/id/100003114/device/rss/rss.html",
    "https://www.sec.gov/news/pressreleases.rss",
    "https://www.federalreserve.gov/feeds/press_all.xml",
];

#[derive(Debug, Clone)]
pub struct LiveNewsConfig {
    feeds: Vec<String>,
    refresh_interval: Duration,
    timeout: Duration,
}

impl LiveNewsConfig {
    pub fn from_env() -> Self {
        let feeds = env::var("MARKET_TERMINAL_NEWS_FEEDS")
            .ok()
            .map(|value| {
                value
                    .split(',')
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned)
                    .collect::<Vec<_>>()
            })
            .filter(|feeds| !feeds.is_empty())
            .unwrap_or_else(|| {
                DEFAULT_FEEDS
                    .iter()
                    .map(|feed| (*feed).to_owned())
                    .collect()
            });
        let refresh_seconds = env_u64("MARKET_TERMINAL_NEWS_REFRESH_SECS")
            .unwrap_or(DEFAULT_REFRESH_SECONDS)
            .clamp(60, 3_600);
        let timeout_seconds = env_u64("MARKET_TERMINAL_NEWS_TIMEOUT_SECS")
            .unwrap_or(DEFAULT_TIMEOUT_SECONDS)
            .clamp(3, 30);
        Self {
            feeds,
            refresh_interval: Duration::from_secs(refresh_seconds),
            timeout: Duration::from_secs(timeout_seconds),
        }
    }
}

#[derive(Debug, Clone)]
struct FeedState {
    workbench: NewsWorkbench,
    status: String,
}

enum WorkerCommand {
    Refresh,
    Stop,
}

pub struct LiveNewsFeed {
    state: Arc<RwLock<FeedState>>,
    commands: mpsc::Sender<WorkerCommand>,
    worker: Mutex<Option<thread::JoinHandle<()>>>,
}

impl LiveNewsFeed {
    pub fn from_env() -> Self {
        Self::new(LiveNewsConfig::from_env())
    }

    pub fn new(config: LiveNewsConfig) -> Self {
        let state = Arc::new(RwLock::new(FeedState {
            workbench: NewsWorkbench {
                stories: Vec::new(),
                events: Vec::new(),
            },
            status: "LIVE FEED · LOADING".to_owned(),
        }));
        let (commands, receiver) = mpsc::channel();
        let worker_state = state.clone();
        let worker = thread::Builder::new()
            .name("market-terminal-news".to_owned())
            .spawn(move || run_worker(config, worker_state, receiver))
            .ok();
        if worker.is_none() {
            state.write().expect("news state lock").status =
                "LIVE FEED ERROR · COULD NOT START WORKER".to_owned();
        }
        Self {
            state,
            commands,
            worker: Mutex::new(worker),
        }
    }
}

impl NewsFeed for LiveNewsFeed {
    fn load_news(&self) -> NewsSnapshot {
        NewsSnapshot {
            headlines: self
                .state
                .read()
                .expect("news state lock")
                .workbench
                .stories
                .iter()
                .map(|story| story.headline.clone())
                .collect(),
        }
    }

    fn load_workbench(&self) -> NewsWorkbench {
        self.state
            .read()
            .expect("news state lock")
            .workbench
            .clone()
    }

    fn status(&self) -> String {
        self.state.read().expect("news state lock").status.clone()
    }

    fn request_refresh(&self) {
        let _ = self.commands.send(WorkerCommand::Refresh);
    }
}

impl Drop for LiveNewsFeed {
    fn drop(&mut self) {
        let _ = self.commands.send(WorkerCommand::Stop);
        // Dropping the handle detaches a request that may currently be inside
        // a bounded network timeout; terminal shutdown must remain immediate.
        let _ = self.worker.lock().expect("news worker lock").take();
    }
}

fn run_worker(
    config: LiveNewsConfig,
    state: Arc<RwLock<FeedState>>,
    commands: mpsc::Receiver<WorkerCommand>,
) {
    let client = match Client::builder()
        .user_agent("market-terminal/0.1 (local desktop market reader)")
        .timeout(config.timeout)
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            state.write().expect("news state lock").status =
                format!("LIVE FEED ERROR · HTTP CLIENT · {error}");
            return;
        }
    };
    loop {
        refresh_feeds(&client, &config.feeds, &state);
        match commands.recv_timeout(config.refresh_interval) {
            Ok(WorkerCommand::Refresh) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Ok(WorkerCommand::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

fn refresh_feeds(client: &Client, feeds: &[String], state: &RwLock<FeedState>) {
    let mut stories = Vec::new();
    let mut failures = Vec::new();
    for url in feeds {
        match fetch_feed(client, url) {
            Ok(mut feed_stories) => stories.append(&mut feed_stories),
            Err(error) => failures.push(error),
        }
    }
    if stories.is_empty() {
        let mut state = state.write().expect("news state lock");
        state.status = if failures.is_empty() {
            "LIVE FEED · NO STORIES RETURNED".to_owned()
        } else {
            format!("LIVE FEED UNAVAILABLE · {}", compact_errors(&failures))
        };
        return;
    }

    stories.sort_by(|left, right| right.0.cmp(&left.0));
    let mut seen = HashSet::new();
    let stories = stories
        .into_iter()
        .filter_map(|(_, story)| seen.insert(story.id.clone()).then_some(story))
        .take(MAX_STORIES)
        .collect::<Vec<_>>();
    let successful = feeds.len().saturating_sub(failures.len());
    let status = if failures.is_empty() {
        format!(
            "LIVE · {} STORIES · {successful} SOURCES · F9 REFRESH",
            stories.len()
        )
    } else {
        format!(
            "LIVE DEGRADED · {} STORIES · {successful}/{} SOURCES · {}",
            stories.len(),
            feeds.len(),
            compact_errors(&failures)
        )
    };
    *state.write().expect("news state lock") = FeedState {
        workbench: NewsWorkbench {
            stories,
            events: Vec::new(),
        },
        status,
    };
}

fn fetch_feed(client: &Client, url: &str) -> Result<Vec<TimestampedStory>, String> {
    let response = client
        .get(url)
        .send()
        .map_err(|error| source_error(url, &error.to_string()))?;
    if !response.status().is_success() {
        return Err(source_error(url, &format!("HTTP {}", response.status())));
    }
    let mut bytes = Vec::new();
    response
        .take(MAX_FEED_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| source_error(url, &error.to_string()))?;
    if bytes.len() as u64 > MAX_FEED_BYTES {
        return Err(source_error(url, "RESPONSE EXCEEDS 2 MB"));
    }
    parse_feed_document(&bytes, url)
}

fn parse_feed_document(bytes: &[u8], url: &str) -> Result<Vec<TimestampedStory>, String> {
    let feed = parser::parse(bytes).map_err(|error| source_error(url, &error.to_string()))?;
    let source = feed
        .title
        .as_ref()
        .map(|title| clean_text(&title.content))
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| source_name(url));
    Ok(feed
        .entries
        .into_iter()
        .filter_map(|entry| story_from_entry(entry, &source))
        .collect())
}

fn story_from_entry(entry: Entry, source: &str) -> Option<(Option<DateTime<Utc>>, NewsStory)> {
    let title = entry
        .title
        .as_ref()
        .map(|title| clean_text(&title.content))?;
    if title.is_empty() {
        return None;
    }
    let published = entry.published.or(entry.updated);
    let time = published
        .map(|timestamp| timestamp.format("%H:%M").to_string())
        .unwrap_or_else(|| "--:--".to_owned());
    let summary = entry
        .summary
        .as_ref()
        .map(|summary| clean_text(&summary.content))
        .or_else(|| {
            entry
                .content
                .as_ref()
                .and_then(|content| content.body.as_ref().map(|body| clean_text(body)))
        })
        .filter(|summary| !summary.is_empty())
        .unwrap_or_else(|| "Open the source link for the full report.".to_owned());
    let summary = truncate(&summary, 900);
    let url = entry
        .links
        .iter()
        .find(|link| link.rel.as_deref().is_none_or(|rel| rel == "alternate"))
        .or_else(|| entry.links.first())
        .map(|link| link.href.clone());
    let byline = if entry.authors.is_empty() {
        source.to_owned()
    } else {
        entry
            .authors
            .iter()
            .map(|author| author.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let topic = classify_topic(&title, source);
    let region = classify_region(source);
    let related_symbols = extract_symbols(&title);
    let instruments = related_symbols
        .iter()
        .map(|symbol| InstrumentId::new(format!("us:listed:{}", symbol.to_ascii_lowercase())))
        .collect();
    let id = if entry.id.trim().is_empty() {
        url.clone().unwrap_or_else(|| format!("{source}:{title}"))
    } else {
        entry.id
    };
    Some((
        published,
        NewsStory {
            id,
            headline: Headline {
                time,
                topic,
                title,
                region,
            },
            byline,
            summary,
            body: Vec::new(),
            related_symbols,
            instruments,
            url,
        },
    ))
}

fn classify_topic(title: &str, source: &str) -> String {
    let text = format!("{title} {source}").to_ascii_uppercase();
    if text.contains("FEDERAL RESERVE") || text.contains("INTEREST RATE") || text.contains("FOMC") {
        "FED"
    } else if text.contains("SECURITIES AND EXCHANGE COMMISSION") || text.contains("SEC ") {
        "REG"
    } else if text.contains("EARNINGS") || text.contains("STOCK") || text.contains("MARKET") {
        "EQU"
    } else if text.contains("OIL") || text.contains("GOLD") || text.contains("COMMODIT") {
        "CMD"
    } else if text.contains("TECH") || text.contains("CHIP") || text.contains("AI ") {
        "TEC"
    } else {
        "TOP"
    }
    .to_owned()
}

fn classify_region(source: &str) -> String {
    let source = source.to_ascii_uppercase();
    if source.contains("SEC") || source.contains("FEDERAL RESERVE") || source.contains("CNBC") {
        "US".to_owned()
    } else {
        "GL".to_owned()
    }
}

fn extract_symbols(title: &str) -> Vec<String> {
    const SYMBOLS: [&str; 30] = [
        "AAPL", "AMD", "AMZN", "AVGO", "BAC", "BRK", "COIN", "CRM", "CVX", "DIS", "GOOG", "GOOGL",
        "GS", "INTC", "JPM", "META", "MSFT", "MU", "NFLX", "NVDA", "ORCL", "PLTR", "QQQ", "SMH",
        "SPY", "TSLA", "TLT", "UBER", "VTI", "XLE",
    ];
    let tokens = title
        .split(|character: char| !character.is_ascii_alphanumeric() && character != '$')
        .map(|token| token.trim_start_matches('$').to_ascii_uppercase())
        .collect::<HashSet<_>>();
    SYMBOLS
        .into_iter()
        .filter(|symbol| tokens.contains(*symbol))
        .map(ToOwned::to_owned)
        .collect()
}

fn clean_text(value: &str) -> String {
    let mut output = String::new();
    let mut in_tag = false;
    for character in value.chars() {
        match character {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                output.push(' ');
            }
            _ if !in_tag => output.push(character),
            _ => {}
        }
    }
    output
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn truncate(value: &str, limit: usize) -> String {
    let mut characters = value.chars();
    let shortened = characters.by_ref().take(limit).collect::<String>();
    if characters.next().is_some() {
        format!("{shortened}…")
    } else {
        shortened
    }
}

fn source_name(url: &str) -> String {
    url.split("//")
        .nth(1)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or("LIVE RSS")
        .to_owned()
}

fn source_error(url: &str, error: &str) -> String {
    format!("{} · {error}", source_name(url))
}

fn compact_errors(errors: &[String]) -> String {
    truncate(&errors.join(" | "), 220)
}

fn env_u64(name: &str) -> Option<u64> {
    env::var(name).ok()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_rss_shape_into_owned_provider_story() {
        let rss = br#"<?xml version="1.0"?>
<rss version="2.0"><channel><title>Market Wire</title>
<item><guid>story-1</guid><title>NVDA shares rise after earnings</title>
<link>https://example.com/story-1</link><pubDate>Tue, 25 Aug 2026 20:15:00 GMT</pubDate>
<description><![CDATA[<p>Chip demand remained strong &amp; guidance increased.</p>]]></description>
</item></channel></rss>"#;

        let stories = parse_feed_document(rss, "https://example.com/feed").unwrap();
        let story = &stories[0].1;

        assert_eq!(story.headline.time, "20:15");
        assert_eq!(story.headline.topic, "EQU");
        assert_eq!(story.byline, "Market Wire");
        assert_eq!(story.related_symbols, ["NVDA"]);
        assert_eq!(
            story.summary,
            "Chip demand remained strong & guidance increased."
        );
        assert_eq!(story.url.as_deref(), Some("https://example.com/story-1"));
    }

    #[test]
    fn strips_markup_and_bounds_feed_text() {
        assert_eq!(
            clean_text("<b>Hello</b>   world &amp; markets"),
            "Hello world & markets"
        );
        assert_eq!(truncate("abcdef", 3), "abc…");
    }

    #[test]
    #[ignore = "requires live network access"]
    fn live_default_feeds_return_current_stories() {
        let client = Client::builder()
            .user_agent("market-terminal/0.1 (live integration test)")
            .timeout(Duration::from_secs(15))
            .build()
            .unwrap();
        let count = DEFAULT_FEEDS
            .iter()
            .filter_map(|url| fetch_feed(&client, url).ok())
            .map(|stories| stories.len())
            .sum::<usize>();

        assert!(count > 0, "default live feeds returned no stories");
    }
}
