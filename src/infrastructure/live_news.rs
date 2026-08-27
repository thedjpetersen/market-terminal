use std::{
    collections::HashSet,
    env,
    io::Read,
    sync::{mpsc, Arc, Mutex, RwLock},
    thread,
    time::Duration,
};

use chrono::{DateTime, Utc};
use dom_smoothie::{Config as ReadabilityConfig, Readability, TextMode};
use feed_rs::{model::Entry, parser};
use reqwest::blocking::Client;

use crate::{
    features::news::{
        ArticleBodyState, Headline, NewsFeed, NewsSnapshot, NewsStory, NewsWorkbench,
    },
    foundation::InstrumentId,
};

const DEFAULT_REFRESH_SECONDS: u64 = 300;
const DEFAULT_TIMEOUT_SECONDS: u64 = 12;
const MAX_FEED_BYTES: u64 = 2 * 1024 * 1024;
const MAX_ARTICLE_BYTES: u64 = 5 * 1024 * 1024;
const MAX_ARTICLE_CHARS: usize = 120_000;
const MAX_STORIES: usize = 80;
type TimestampedStory = (Option<DateTime<Utc>>, NewsStory);
const DEFAULT_FEEDS: [&str; 7] = [
    "https://seekingalpha.com/market_currents.xml",
    "https://seekingalpha.com/feed.xml",
    "https://feeds.bloomberg.com/markets/news.rss",
    "https://feeds.content.dowjones.io/public/rss/mw_topstories",
    "https://www.ft.com/markets?format=rss",
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
    FetchArticle { story_id: String, url: String },
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

    fn request_article(&self, story_id: &str, url: &str) -> bool {
        {
            let mut state = self.state.write().expect("news state lock");
            let Some(story) = state
                .workbench
                .stories
                .iter_mut()
                .find(|story| story.id == story_id)
            else {
                return false;
            };
            if matches!(
                story.body_state,
                ArticleBodyState::FeedProvided
                    | ArticleBodyState::Loading
                    | ArticleBodyState::Downloaded
            ) {
                return true;
            }
            story.body_state = ArticleBodyState::Loading;
        }
        if self
            .commands
            .send(WorkerCommand::FetchArticle {
                story_id: story_id.to_owned(),
                url: url.to_owned(),
            })
            .is_ok()
        {
            true
        } else {
            set_article_unavailable(&self.state, story_id, "news worker is unavailable");
            false
        }
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
    refresh_feeds(&client, &config.feeds, &state);
    loop {
        match commands.recv_timeout(config.refresh_interval) {
            Ok(WorkerCommand::Refresh) | Err(mpsc::RecvTimeoutError::Timeout) => {
                refresh_feeds(&client, &config.feeds, &state);
            }
            Ok(WorkerCommand::FetchArticle { story_id, url }) => {
                fetch_and_store_article(&client, &state, &story_id, &url);
            }
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

    stories.sort_by_key(|story| std::cmp::Reverse(story.0));
    let mut seen = HashSet::new();
    let mut stories = stories
        .into_iter()
        .filter_map(|(_, story)| seen.insert(story.id.clone()).then_some(story))
        .take(MAX_STORIES)
        .collect::<Vec<_>>();
    let downloaded = state
        .read()
        .expect("news state lock")
        .workbench
        .stories
        .iter()
        .filter(|story| matches!(story.body_state, ArticleBodyState::Downloaded))
        .map(|story| (story.id.clone(), story.body.clone()))
        .collect::<std::collections::HashMap<_, _>>();
    for story in &mut stories {
        if let Some(body) = downloaded.get(&story.id) {
            story.body.clone_from(body);
            story.body_state = ArticleBodyState::Downloaded;
        }
    }
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
    let feed_body = entry
        .content
        .as_ref()
        .and_then(|content| content.body.as_deref())
        .map(paragraphs_from_markup)
        .unwrap_or_default();
    let feed_body_chars = feed_body.iter().map(String::len).sum::<usize>();
    let body_is_substantial = feed_body.len() > 1 || feed_body_chars >= 500;
    let summary = entry
        .summary
        .as_ref()
        .map(|summary| clean_text(&summary.content))
        .or_else(|| feed_body.first().cloned())
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
    let related_symbols = extract_entry_symbols(&entry, &title);
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
            body: if body_is_substantial {
                feed_body
            } else {
                Vec::new()
            },
            body_state: if body_is_substantial {
                ArticleBodyState::FeedProvided
            } else {
                ArticleBodyState::ExcerptOnly
            },
            related_symbols,
            instruments,
            url,
        },
    ))
}

fn fetch_and_store_article(client: &Client, state: &RwLock<FeedState>, story_id: &str, url: &str) {
    match fetch_article_body(client, url) {
        Ok(body) => {
            let mut state = state.write().expect("news state lock");
            if let Some(story) = state
                .workbench
                .stories
                .iter_mut()
                .find(|story| story.id == story_id)
            {
                story.body = body;
                story.body_state = ArticleBodyState::Downloaded;
            }
        }
        Err(error) => set_article_unavailable(state, story_id, &error),
    }
}

fn fetch_article_body(client: &Client, url: &str) -> Result<Vec<String>, String> {
    let response = client
        .get(url)
        .header(
            reqwest::header::ACCEPT,
            "text/html,application/xhtml+xml;q=0.9",
        )
        .send()
        .map_err(|error| format!("article request failed: {error}"))?;
    if !response.status().is_success() {
        return Err(format!("publisher returned HTTP {}", response.status()));
    }
    if response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            !value.contains("text/html") && !value.contains("application/xhtml+xml")
        })
    {
        return Err("publisher did not return an HTML article".to_owned());
    }
    let document_url = response.url().to_string();
    let mut bytes = Vec::new();
    response
        .take(MAX_ARTICLE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("article download failed: {error}"))?;
    if bytes.len() as u64 > MAX_ARTICLE_BYTES {
        return Err("article exceeds the 5 MB reader limit".to_owned());
    }
    let html = String::from_utf8_lossy(&bytes).into_owned();
    extract_article_body(&html, &document_url)
}

fn extract_article_body(html: &str, document_url: &str) -> Result<Vec<String>, String> {
    let config = ReadabilityConfig {
        max_elements_to_parse: 30_000,
        text_mode: TextMode::Formatted,
        ..ReadabilityConfig::default()
    };
    let mut readability = Readability::new(html, Some(document_url), Some(config))
        .map_err(|error| format!("article parser rejected the page: {error}"))?;
    let article = readability
        .parse()
        .map_err(|error| format!("article text was not readable: {error}"))?;
    let body = paragraphs_from_text(&article.text_content, MAX_ARTICLE_CHARS);
    let length = body.iter().map(String::len).sum::<usize>();
    if body.is_empty() || length < 240 {
        return Err(
            "publisher exposed only a short preview; press O to read it on the web".to_owned(),
        );
    }
    Ok(body)
}

fn set_article_unavailable(state: &RwLock<FeedState>, story_id: &str, error: &str) {
    let mut state = state.write().expect("news state lock");
    if let Some(story) = state
        .workbench
        .stories
        .iter_mut()
        .find(|story| story.id == story_id)
    {
        story.body_state = ArticleBodyState::Unavailable(truncate(error, 180));
    }
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
    if source.contains("SEC")
        || source.contains("FEDERAL RESERVE")
        || source.contains("SEEKING ALPHA")
        || source.contains("MARKETWATCH")
    {
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

fn extract_entry_symbols(entry: &Entry, title: &str) -> Vec<String> {
    let mut symbols = extract_symbols(title);
    for category in &entry.categories {
        let candidate = category
            .term
            .trim()
            .trim_start_matches('$')
            .to_ascii_uppercase();
        let looks_like_symbol = (1..=5).contains(&candidate.len())
            && candidate
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '.');
        if looks_like_symbol
            && !matches!(
                candidate.as_str(),
                "AI" | "ETF" | "IPO" | "NEWS" | "STOCK" | "TECH"
            )
            && !symbols.contains(&candidate)
        {
            symbols.push(candidate);
        }
    }
    symbols
}

fn clean_text(value: &str) -> String {
    normalize_whitespace(&strip_markup(value, false))
}

fn paragraphs_from_markup(value: &str) -> Vec<String> {
    paragraphs_from_text(&strip_markup(value, true), MAX_ARTICLE_CHARS)
}

fn strip_markup(value: &str, preserve_blocks: bool) -> String {
    let mut output = String::new();
    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        if character != '<' {
            output.push(character);
            continue;
        }
        let mut tag = String::new();
        for tag_character in characters.by_ref() {
            if tag_character == '>' {
                break;
            }
            tag.push(tag_character);
        }
        let name = tag
            .trim()
            .trim_start_matches('/')
            .split_ascii_whitespace()
            .next()
            .unwrap_or_default()
            .trim_end_matches('/')
            .to_ascii_lowercase();
        if preserve_blocks
            && matches!(
                name.as_str(),
                "p" | "br" | "div" | "li" | "h1" | "h2" | "h3" | "h4" | "blockquote"
            )
        {
            output.push('\n');
        }
    }
    html_escape::decode_html_entities(&output).into_owned()
}

fn normalize_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn paragraphs_from_text(value: &str, limit: usize) -> Vec<String> {
    let mut remaining = limit;
    value
        .lines()
        .filter_map(|line| {
            let paragraph = normalize_whitespace(line);
            if paragraph.is_empty() || remaining == 0 {
                return None;
            }
            let paragraph = truncate(&paragraph, remaining);
            remaining = remaining.saturating_sub(paragraph.chars().count());
            Some(paragraph)
        })
        .collect()
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
        assert_eq!(story.body_state, ArticleBodyState::ExcerptOnly);
    }

    #[test]
    fn keeps_feed_provided_article_content_and_category_symbols() {
        let rss = br#"<?xml version="1.0"?>
<rss xmlns:content="http://purl.org/rss/1.0/modules/content/" version="2.0">
<channel><title>Investment Ideas</title><item><guid>idea-1</guid>
<title>A durable compounder</title><link>https://example.com/idea-1</link>
<category>AAPL</category><description>Our short investment thesis.</description>
<content:encoded><![CDATA[
<p>Apple has built an unusually durable installed base with recurring services demand.</p>
<p>The balance sheet supports continued investment, repurchases, and product development over a full cycle.</p>
]]></content:encoded></item></channel></rss>"#;

        let stories = parse_feed_document(rss, "https://example.com/feed").unwrap();
        let story = &stories[0].1;

        assert_eq!(story.related_symbols, ["AAPL"]);
        assert_eq!(story.body_state, ArticleBodyState::FeedProvided);
        assert_eq!(story.body.len(), 2);
    }

    #[test]
    fn extracts_readable_article_paragraphs_for_the_terminal_reader() {
        let html = r#"
<!doctype html><html><head><title>Markets reset expectations</title></head><body>
<nav>Home Markets Subscribe Sign in</nav>
<article>
<h1>Markets reset expectations</h1>
<p>Investors adjusted rate expectations after a broad set of economic releases changed the outlook for the coming quarters. The response moved through equities, rates, currencies, and commodities.</p>
<p>Market breadth improved while long-duration government bonds recovered from their intraday lows. Analysts said the cross-asset response reflected positioning as much as the incoming data.</p>
<p>The next inflation report and central-bank meeting are now the main events on the calendar. Traders will watch both the headline figures and the underlying distribution for confirmation.</p>
</article><footer>Privacy Terms Careers</footer></body></html>"#;

        let body = extract_article_body(html, "https://example.com/article").unwrap();

        assert!(body
            .join(" ")
            .contains("Investors adjusted rate expectations"));
        assert!(!body.join(" ").contains("Privacy Terms Careers"));
    }

    #[test]
    fn strips_markup_and_bounds_feed_text() {
        assert_eq!(
            clean_text("<b>Hello</b>   world &amp; markets &#x2014; today"),
            "Hello world & markets — today"
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
        let results = DEFAULT_FEEDS
            .iter()
            .map(|url| (*url, fetch_feed(&client, url)))
            .collect::<Vec<_>>();
        let successful = results
            .iter()
            .filter(|(_, result)| result.as_ref().is_ok_and(|stories| !stories.is_empty()))
            .count();
        let count = results
            .iter()
            .filter_map(|(_, result)| result.as_ref().ok())
            .map(Vec::len)
            .sum::<usize>();

        assert!(count > 0, "default live feeds returned no stories");
        assert!(
            successful >= 4,
            "fewer than four default publishers returned stories: {results:?}"
        );
        assert!(
            results[0]
                .1
                .as_ref()
                .is_ok_and(|stories| !stories.is_empty()),
            "Seeking Alpha All News did not return stories: {:?}",
            results[0].1
        );
    }

    #[test]
    #[ignore = "requires live network access"]
    fn live_reader_extracts_a_current_publisher_article() {
        let client = Client::builder()
            .user_agent("market-terminal/0.1 (live integration test)")
            .timeout(Duration::from_secs(15))
            .build()
            .unwrap();
        let candidates = DEFAULT_FEEDS[2..5]
            .iter()
            .filter_map(|url| fetch_feed(&client, url).ok())
            .flat_map(|stories| stories.into_iter().take(3))
            .filter_map(|(_, story)| story.url)
            .collect::<Vec<_>>();
        let readable = candidates.iter().find_map(|url| {
            fetch_article_body(&client, url)
                .ok()
                .map(|body| (url, body))
        });

        let Some((url, body)) = readable else {
            panic!("no current Bloomberg, MarketWatch, or FT article was readable: {candidates:?}");
        };
        assert!(
            body.iter().map(String::len).sum::<usize>() >= 240,
            "reader returned a short article from {url}"
        );
    }
}
