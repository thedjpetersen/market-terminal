use std::{
    collections::{HashMap, HashSet},
    env,
    io::Read,
    sync::{mpsc, Arc, Mutex, RwLock},
    thread,
    time::Duration,
};

use chrono::{DateTime, Utc};
use dom_smoothie::{Config as ReadabilityConfig, Readability, TextMode};
use feed_rs::{model::Entry, parser};
use reqwest::{blocking::Client, Url};

use crate::{
    features::news::{
        analyze_news_sentiment, ArticleBodyState, Headline, NewsFeed, NewsFreshness,
        NewsProvenance, NewsSnapshot, NewsStory, NewsWorkbench,
    },
    foundation::InstrumentId,
};

const DEFAULT_REFRESH_SECONDS: u64 = 300;
const DEFAULT_TIMEOUT_SECONDS: u64 = 12;
const MAX_FEED_BYTES: u64 = 2 * 1024 * 1024;
const MAX_ARTICLE_BYTES: u64 = 5 * 1024 * 1024;
const MAX_ARTICLE_CHARS: usize = 120_000;
const MAX_STORIES: usize = 80;
const MAX_FEEDS: usize = 24;
const MAX_CATEGORIES: usize = 12;
const MAX_RELATED_SYMBOLS: usize = 16;
const COMMAND_QUEUE_CAPACITY: usize = 8;
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
        let mut seen = HashSet::new();
        let feeds = feeds
            .into_iter()
            .filter_map(|feed| canonical_http_url(&feed, None))
            .filter(|feed| seen.insert(feed.clone()))
            .take(MAX_FEEDS)
            .collect();
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
    commands: mpsc::SyncSender<WorkerCommand>,
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
        let (commands, receiver) = mpsc::sync_channel(COMMAND_QUEUE_CAPACITY);
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

    fn load_events(&self) -> Vec<crate::features::news::NewsEvent> {
        self.state
            .read()
            .expect("news state lock")
            .workbench
            .events
            .clone()
    }

    fn status(&self) -> String {
        self.state.read().expect("news state lock").status.clone()
    }

    fn request_refresh(&self) {
        let _ = self.commands.try_send(WorkerCommand::Refresh);
    }

    fn request_article(&self, story_id: &str, url: &str) -> bool {
        let requested_url = canonical_http_url(url, None);
        let accepted_url = {
            let mut state = self.state.write().expect("news state lock");
            let Some(story) = state
                .workbench
                .stories
                .iter_mut()
                .find(|story| story.id == story_id)
            else {
                return false;
            };
            let stored_url = story
                .url
                .as_deref()
                .and_then(|value| canonical_http_url(value, None));
            if requested_url.is_none() || requested_url != stored_url {
                return false;
            }
            if matches!(
                story.body_state,
                ArticleBodyState::FeedProvided
                    | ArticleBodyState::Loading
                    | ArticleBodyState::Downloaded
            ) {
                return true;
            }
            story.body_state = ArticleBodyState::Loading;
            stored_url.expect("validated stored article URL")
        };
        if self
            .commands
            .try_send(WorkerCommand::FetchArticle {
                story_id: story_id.to_owned(),
                url: accepted_url,
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
        let _ = self.commands.try_send(WorkerCommand::Stop);
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
    let retrieved_at = Utc::now();
    let outcomes = fetch_feeds_concurrently(client, feeds, retrieved_at);
    let previous = state.read().expect("news state lock").clone();
    *state.write().expect("news state lock") =
        assemble_refresh(&previous, feeds, outcomes, retrieved_at);
}

fn fetch_feeds_concurrently(
    client: &Client,
    feeds: &[String],
    retrieved_at: DateTime<Utc>,
) -> Vec<(String, Result<Vec<TimestampedStory>, String>)> {
    thread::scope(|scope| {
        let handles = feeds
            .iter()
            .map(|url| {
                let client = client.clone();
                let url = url.clone();
                let worker_url = url.clone();
                (
                    url,
                    scope.spawn(move || fetch_feed_at(&client, &worker_url, retrieved_at)),
                )
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .map(|(url, handle)| {
                let result = handle
                    .join()
                    .unwrap_or_else(|_| Err(source_error(&url, "FETCH WORKER PANICKED")));
                (url, result)
            })
            .collect()
    })
}

fn assemble_refresh(
    previous: &FeedState,
    feeds: &[String],
    outcomes: Vec<(String, Result<Vec<TimestampedStory>, String>)>,
    retrieved_at: DateTime<Utc>,
) -> FeedState {
    let mut stories = Vec::new();
    let mut failures = Vec::new();
    let mut failed_urls = HashSet::new();
    let mut successful = 0;
    for (url, result) in outcomes {
        match result {
            Ok(mut source_stories) => {
                successful += 1;
                stories.append(&mut source_stories);
            }
            Err(error) => {
                failed_urls.insert(url);
                failures.push(error);
            }
        }
    }

    let mut stale_sources = HashSet::new();
    for story in &previous.workbench.stories {
        if story
            .provenance
            .feed_urls
            .iter()
            .any(|url| failed_urls.contains(url))
        {
            let mut retained = story.clone();
            retained.provenance.freshness = NewsFreshness::StaleSource;
            for url in &retained.provenance.feed_urls {
                if failed_urls.contains(url) {
                    stale_sources.insert(url.clone());
                }
            }
            let timestamp = retained
                .provenance
                .published_at
                .as_deref()
                .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
                .map(|value| value.with_timezone(&Utc));
            stories.push((timestamp, retained));
        }
    }

    preserve_downloaded_articles(&mut stories, &previous.workbench.stories);
    let stories = merge_and_limit_stories(stories);
    let status = if failures.is_empty() {
        if stories.is_empty() {
            format!("LIVE · NO STORIES · {successful} SOURCES · F9 REFRESH")
        } else {
            format!(
                "LIVE · {} STORIES · {successful} SOURCES · AS OF {}Z · F9 REFRESH",
                stories.len(),
                retrieved_at.format("%H:%M")
            )
        }
    } else if stories.is_empty() {
        format!("LIVE FEED UNAVAILABLE · {}", compact_errors(&failures))
    } else {
        format!(
            "LIVE DEGRADED · {} STORIES · {successful}/{} SOURCES · {} STALE · {}",
            stories.len(),
            feeds.len(),
            stale_sources.len(),
            compact_errors(&failures)
        )
    };
    FeedState {
        workbench: NewsWorkbench {
            stories,
            events: Vec::new(),
        },
        status,
    }
}

fn preserve_downloaded_articles(stories: &mut [TimestampedStory], previous: &[NewsStory]) {
    let downloaded = previous
        .iter()
        .filter(|story| matches!(story.body_state, ArticleBodyState::Downloaded))
        .map(|story| (story_dedup_key(story), story))
        .collect::<HashMap<_, _>>();
    for (_, story) in stories {
        if let Some(previous) = downloaded.get(&story_dedup_key(story)) {
            story.body.clone_from(&previous.body);
            story.body_state = ArticleBodyState::Downloaded;
            story.byline.clone_from(&previous.byline);
            if previous.summary.len() > story.summary.len() {
                story.summary.clone_from(&previous.summary);
            }
            if story.provenance.language.is_none() {
                story
                    .provenance
                    .language
                    .clone_from(&previous.provenance.language);
            }
            refresh_sentiment(story);
        }
    }
}

fn merge_and_limit_stories(mut stories: Vec<TimestampedStory>) -> Vec<NewsStory> {
    stories.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| left.1.id.cmp(&right.1.id))
    });
    let mut merged = Vec::<NewsStory>::new();
    let mut keys = HashMap::<String, usize>::new();
    for (_, story) in stories {
        let candidates = story_dedup_keys(&story);
        let existing = candidates.iter().find_map(|key| keys.get(key).copied());
        if let Some(index) = existing {
            merge_story(&mut merged[index], story);
            for key in candidates {
                keys.insert(key, index);
            }
        } else if merged.len() < MAX_STORIES {
            let index = merged.len();
            for key in candidates {
                keys.insert(key, index);
            }
            merged.push(story);
        }
    }
    merged
}

fn merge_story(primary: &mut NewsStory, duplicate: NewsStory) {
    extend_unique(
        &mut primary.provenance.sources,
        duplicate.provenance.sources,
        8,
    );
    extend_unique(
        &mut primary.provenance.feed_urls,
        duplicate.provenance.feed_urls,
        8,
    );
    extend_unique(
        &mut primary.provenance.categories,
        duplicate.provenance.categories,
        MAX_CATEGORIES,
    );
    extend_unique(
        &mut primary.related_symbols,
        duplicate.related_symbols,
        MAX_RELATED_SYMBOLS,
    );
    for instrument in duplicate.instruments {
        if primary.instruments.len() >= MAX_RELATED_SYMBOLS {
            break;
        }
        if !primary.instruments.contains(&instrument) {
            primary.instruments.push(instrument);
        }
    }
    if duplicate.provenance.freshness == NewsFreshness::Fresh {
        primary.provenance.freshness = NewsFreshness::Fresh;
    }
    if primary.provenance.language.is_none() {
        primary.provenance.language = duplicate.provenance.language;
    }
    if primary.provenance.published_at.is_none() {
        primary.provenance.published_at = duplicate.provenance.published_at;
    }
    if duplicate.provenance.retrieved_at > primary.provenance.retrieved_at {
        primary.provenance.retrieved_at = duplicate.provenance.retrieved_at;
    }
    if duplicate.body.iter().map(String::len).sum::<usize>()
        > primary.body.iter().map(String::len).sum::<usize>()
    {
        primary.body = duplicate.body;
        primary.body_state = duplicate.body_state;
    }
    if duplicate.summary.len() > primary.summary.len() {
        primary.summary = duplicate.summary;
    }
    refresh_sentiment(primary);
}

fn refresh_sentiment(story: &mut NewsStory) {
    let observed_at = story
        .provenance
        .published_at
        .as_deref()
        .unwrap_or(&story.provenance.retrieved_at);
    story.sentiment = analyze_news_sentiment(
        &story.headline.title,
        &story.summary,
        &story.provenance.categories,
        observed_at,
    );
}

fn extend_unique(values: &mut Vec<String>, additions: Vec<String>, limit: usize) {
    for addition in additions {
        if values.len() >= limit {
            break;
        }
        if !values
            .iter()
            .any(|value| value.eq_ignore_ascii_case(&addition))
        {
            values.push(addition);
        }
    }
}

fn fetch_feed_at(
    client: &Client,
    url: &str,
    retrieved_at: DateTime<Utc>,
) -> Result<Vec<TimestampedStory>, String> {
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
    parse_feed_document_at(&bytes, url, retrieved_at)
}

fn parse_feed_document_at(
    bytes: &[u8],
    url: &str,
    retrieved_at: DateTime<Utc>,
) -> Result<Vec<TimestampedStory>, String> {
    let feed = parser::parse(bytes).map_err(|error| source_error(url, &error.to_string()))?;
    let source = feed
        .title
        .as_ref()
        .map(|title| clean_text(&title.content))
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| source_name(url));
    let language = feed
        .language
        .as_deref()
        .map(clean_text)
        .filter(|value| !value.is_empty());
    Ok(feed
        .entries
        .into_iter()
        .filter_map(|entry| {
            story_from_entry(entry, &source, url, language.as_deref(), retrieved_at)
        })
        .collect())
}

fn story_from_entry(
    entry: Entry,
    source: &str,
    feed_url: &str,
    language: Option<&str>,
    retrieved_at: DateTime<Utc>,
) -> Option<(Option<DateTime<Utc>>, NewsStory)> {
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
        .and_then(|link| canonical_http_url(&link.href, Some(feed_url)));
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
    let category_labels = entry
        .categories
        .iter()
        .map(|category| clean_text(&category.term))
        .filter(|category| !category.is_empty())
        .take(MAX_CATEGORIES)
        .collect::<Vec<_>>();
    let context = format!("{title} {summary} {}", category_labels.join(" "));
    let topic = classify_topic(&context, source);
    let region = classify_region(&context, source);
    let related_symbols = extract_entry_symbols(&entry, &context);
    let instruments = related_symbols
        .iter()
        .map(|symbol| InstrumentId::new(format!("us:listed:{}", symbol.to_ascii_lowercase())))
        .collect();
    let id = stable_story_id(url.as_deref(), &entry.id, source, &title, published);
    let published_at = published.map(|value| value.to_rfc3339());
    let retrieved_at = retrieved_at.to_rfc3339();
    let sentiment = analyze_news_sentiment(
        &title,
        &summary,
        &category_labels,
        published_at.as_deref().unwrap_or(&retrieved_at),
    );
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
            sentiment,
            provenance: NewsProvenance {
                sources: vec![source.to_owned()],
                feed_urls: vec![feed_url.to_owned()],
                published_at,
                retrieved_at,
                categories: category_labels,
                language: language.map(ToOwned::to_owned),
                freshness: NewsFreshness::Fresh,
            },
        },
    ))
}

fn fetch_and_store_article(client: &Client, state: &RwLock<FeedState>, story_id: &str, url: &str) {
    match fetch_article(client, url) {
        Ok(article) => {
            let mut state = state.write().expect("news state lock");
            if let Some(story) = state
                .workbench
                .stories
                .iter_mut()
                .find(|story| story.id == story_id)
            {
                story.body = article.body;
                story.body_state = ArticleBodyState::Downloaded;
                if let Some(byline) = article.byline {
                    story.byline = byline;
                }
                if story.summary.starts_with("Open the source link") {
                    if let Some(excerpt) = article.excerpt {
                        story.summary = excerpt;
                    }
                }
                if story.provenance.published_at.is_none() {
                    story.provenance.published_at = article.published_at;
                }
                if story.provenance.language.is_none() {
                    story.provenance.language = article.language;
                }
                if let Some(site_name) = article.site_name {
                    extend_unique(&mut story.provenance.sources, vec![site_name], 8);
                }
                refresh_sentiment(story);
            }
        }
        Err(error) => set_article_unavailable(state, story_id, &error),
    }
}

struct DownloadedArticle {
    body: Vec<String>,
    byline: Option<String>,
    excerpt: Option<String>,
    site_name: Option<String>,
    published_at: Option<String>,
    language: Option<String>,
}

fn fetch_article(client: &Client, url: &str) -> Result<DownloadedArticle, String> {
    let url = canonical_http_url(url, None)
        .ok_or_else(|| "article link is not an HTTP(S) URL".to_owned())?;
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
    extract_article(&html, &document_url)
}

fn extract_article(html: &str, document_url: &str) -> Result<DownloadedArticle, String> {
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
    Ok(DownloadedArticle {
        body,
        byline: article
            .byline
            .as_deref()
            .map(clean_text)
            .filter(|value| !value.is_empty())
            .map(|value| truncate(&value, 240)),
        excerpt: article
            .excerpt
            .as_deref()
            .map(clean_text)
            .filter(|value| !value.is_empty())
            .map(|value| truncate(&value, 900)),
        site_name: article
            .site_name
            .as_deref()
            .map(clean_text)
            .filter(|value| !value.is_empty())
            .map(|value| truncate(&value, 120)),
        published_at: article
            .published_time
            .as_deref()
            .map(clean_text)
            .filter(|value| !value.is_empty())
            .map(|value| truncate(&value, 80)),
        language: article
            .lang
            .as_deref()
            .map(clean_text)
            .filter(|value| !value.is_empty())
            .map(|value| truncate(&value, 24)),
    })
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

fn classify_topic(context: &str, source: &str) -> String {
    let text = format!("{context} {source}").to_ascii_uppercase();
    if contains_any(
        &text,
        &[
            "SECURITIES AND EXCHANGE COMMISSION",
            " SEC ",
            "REGULATOR",
            "ANTITRUST",
        ],
    ) {
        "REG"
    } else if contains_any(
        &text,
        &[
            "FEDERAL RESERVE",
            "INTEREST RATE",
            "FOMC",
            "CENTRAL BANK",
            "ECB ",
        ],
    ) {
        "FED"
    } else if contains_any(
        &text,
        &[
            "INFLATION",
            "CPI",
            "PCE",
            "PAYROLL",
            "JOBS",
            "UNEMPLOYMENT",
            "GDP",
            "ECONOMY",
            "ECONOMIC",
        ],
    ) {
        "ECO"
    } else if contains_any(
        &text,
        &["OIL", "GOLD", "COPPER", "COMMODIT", "NATURAL GAS", "OPEC"],
    ) {
        "CMD"
    } else if contains_any(&text, &["BITCOIN", "CRYPTO", "ETHEREUM", "BLOCKCHAIN"]) {
        "CRY"
    } else if contains_any(&text, &["FOREX", "CURRENCY", "DOLLAR", "EURO ", "YEN "]) {
        "FX"
    } else if contains_any(
        &text,
        &[
            "TECH",
            "CHIP",
            "SEMICONDUCTOR",
            "ARTIFICIAL INTELLIGENCE",
            " AI ",
        ],
    ) {
        "TEC"
    } else if contains_any(
        &text,
        &[
            "EARNINGS",
            "STOCK",
            "EQUIT",
            "SHARES",
            "MARKET",
            "IPO",
            "MERGER",
            "ACQUISITION",
        ],
    ) {
        "EQU"
    } else {
        "TOP"
    }
    .to_owned()
}

fn classify_region(context: &str, source: &str) -> String {
    let text = format!("{context} {source}").to_ascii_uppercase();
    if contains_any(
        &text,
        &[
            "CHINA",
            "CHINESE",
            "JAPAN",
            "JAPANESE",
            "INDIA",
            "ASIA",
            "HONG KONG",
            "SOUTH KOREA",
        ],
    ) {
        "AS".to_owned()
    } else if contains_any(
        &text,
        &[
            "EUROPE",
            "EUROPEAN",
            "EUROZONE",
            "ECB",
            "GERMANY",
            "FRANCE",
            "BRITAIN",
            "UNITED KINGDOM",
            " UK ",
        ],
    ) {
        "EU".to_owned()
    } else if contains_any(
        &text,
        &[
            "SEC",
            "FEDERAL RESERVE",
            "SEEKING ALPHA",
            "MARKETWATCH",
            "UNITED STATES",
            " U.S. ",
            " US ",
        ],
    ) {
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
    let mut symbols = SYMBOLS
        .into_iter()
        .filter(|symbol| tokens.contains(*symbol))
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    for token in title.split_whitespace() {
        let candidate = token
            .trim_matches(|character: char| {
                !character.is_ascii_alphanumeric() && character != '$' && character != '.'
            })
            .strip_prefix('$')
            .unwrap_or_default()
            .to_ascii_uppercase();
        if valid_symbol_candidate(&candidate) && !symbols.contains(&candidate) {
            symbols.push(candidate);
        }
    }
    const COMPANY_ALIASES: [(&str, &str); 22] = [
        ("APPLE", "AAPL"),
        ("MICROSOFT", "MSFT"),
        ("NVIDIA", "NVDA"),
        ("AMAZON", "AMZN"),
        ("ALPHABET", "GOOGL"),
        ("GOOGLE", "GOOGL"),
        ("META PLATFORMS", "META"),
        ("TESLA", "TSLA"),
        ("BROADCOM", "AVGO"),
        ("ADVANCED MICRO DEVICES", "AMD"),
        ("INTEL", "INTC"),
        ("ORACLE", "ORCL"),
        ("SALESFORCE", "CRM"),
        ("NETFLIX", "NFLX"),
        ("UBER", "UBER"),
        ("JPMORGAN", "JPM"),
        ("GOLDMAN SACHS", "GS"),
        ("BANK OF AMERICA", "BAC"),
        ("COINBASE", "COIN"),
        ("PALANTIR", "PLTR"),
        ("MICRON", "MU"),
        ("BERKSHIRE HATHAWAY", "BRK.B"),
    ];
    let uppercase = title.to_ascii_uppercase();
    for (name, symbol) in COMPANY_ALIASES {
        if uppercase.contains(name) && !symbols.iter().any(|value| value == symbol) {
            symbols.push(symbol.to_owned());
        }
    }
    symbols.truncate(MAX_RELATED_SYMBOLS);
    symbols
}

fn extract_entry_symbols(entry: &Entry, title: &str) -> Vec<String> {
    let mut symbols = extract_symbols(title);
    for category in &entry.categories {
        let candidate = category
            .term
            .trim()
            .trim_start_matches('$')
            .to_ascii_uppercase();
        if valid_symbol_candidate(&candidate) && !symbols.contains(&candidate) {
            symbols.push(candidate);
        }
    }
    symbols.truncate(MAX_RELATED_SYMBOLS);
    symbols
}

fn valid_symbol_candidate(candidate: &str) -> bool {
    (1..=6).contains(&candidate.len())
        && candidate
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '.')
        && !matches!(
            candidate,
            "A" | "I"
                | "AI"
                | "CEO"
                | "CFO"
                | "CPI"
                | "ETF"
                | "EU"
                | "FED"
                | "GDP"
                | "IPO"
                | "NEWS"
                | "SEC"
                | "STOCK"
                | "TECH"
                | "UK"
                | "US"
                | "USA"
        )
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
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

fn canonical_http_url(value: &str, base: Option<&str>) -> Option<String> {
    let mut url = Url::parse(value)
        .ok()
        .or_else(|| Url::parse(base?).ok()?.join(value).ok())?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return None;
    }
    url.set_fragment(None);
    let retained = url
        .query_pairs()
        .filter(|(key, _)| !is_tracking_parameter(key))
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    url.set_query(None);
    if !retained.is_empty() {
        url.query_pairs_mut().extend_pairs(retained);
    }
    Some(url.to_string())
}

fn is_tracking_parameter(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.starts_with("utm_")
        || matches!(
            key.as_str(),
            "fbclid" | "gclid" | "mc_cid" | "mc_eid" | "ocid" | "ref_src" | "ref_url"
        )
}

fn stable_story_id(
    url: Option<&str>,
    entry_id: &str,
    source: &str,
    title: &str,
    published: Option<DateTime<Utc>>,
) -> String {
    if let Some(url) = url {
        return url.to_owned();
    }
    if !entry_id.trim().is_empty() {
        return truncate(entry_id.trim(), 512);
    }
    format!(
        "NEWS-{:016X}",
        fnv64(&format!(
            "{}|{}|{}",
            source.to_ascii_lowercase(),
            normalized_title(title),
            published
                .map(|value| value.format("%Y-%m-%d").to_string())
                .unwrap_or_default()
        ))
    )
}

fn story_dedup_key(story: &NewsStory) -> String {
    story_dedup_keys(story)
        .into_iter()
        .next()
        .unwrap_or_else(|| format!("id:{}", story.id))
}

fn story_dedup_keys(story: &NewsStory) -> Vec<String> {
    let mut keys = Vec::new();
    if let Some(url) = story
        .url
        .as_deref()
        .and_then(|value| canonical_http_url(value, None))
    {
        keys.push(format!("url:{url}"));
    }
    let day = story
        .provenance
        .published_at
        .as_deref()
        .and_then(|value| value.get(..10))
        .unwrap_or("undated");
    keys.push(format!(
        "title:{}:{day}",
        normalized_title(&story.headline.title)
    ));
    keys
}

fn normalized_title(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn fnv64(value: &str) -> u64 {
    value
        .bytes()
        .fold(14_695_981_039_346_656_037, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(1_099_511_628_211)
        })
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

        let stories = parse_feed_document_at(rss, "https://example.com/feed", Utc::now()).unwrap();
        let story = &stories[0].1;

        assert_eq!(story.headline.time, "20:15");
        assert_eq!(story.headline.topic, "TEC");
        assert_eq!(story.byline, "Market Wire");
        assert_eq!(story.related_symbols, ["NVDA"]);
        assert_eq!(
            story.summary,
            "Chip demand remained strong & guidance increased."
        );
        assert_eq!(story.url.as_deref(), Some("https://example.com/story-1"));
        assert_eq!(story.body_state, ArticleBodyState::ExcerptOnly);
        assert_eq!(story.provenance.sources, ["Market Wire"]);
        assert_eq!(
            story.provenance.published_at.as_deref(),
            Some("2026-08-25T20:15:00+00:00")
        );
        assert_eq!(story.provenance.freshness, NewsFreshness::Fresh);
        assert_eq!(story.sentiment.label.label(), "POSITIVE");
        assert!(story.sentiment.score_bps > 0);
        assert_eq!(story.sentiment.method_version, "MT-LEXICON-1");
        assert!(story.sentiment.calibration.contains("UNCALIBRATED"));
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

        let stories = parse_feed_document_at(rss, "https://example.com/feed", Utc::now()).unwrap();
        let story = &stories[0].1;

        assert_eq!(story.related_symbols, ["AAPL"]);
        assert_eq!(story.body_state, ArticleBodyState::FeedProvided);
        assert_eq!(story.body.len(), 2);
    }

    #[test]
    fn extracts_readable_article_paragraphs_for_the_terminal_reader() {
        let html = r#"
<!doctype html><html lang="en"><head><title>Markets reset expectations</title>
<meta name="author" content="Ada Markets">
<meta property="og:site_name" content="Market Wire">
<meta property="article:published_time" content="2026-08-29T09:30:00Z">
</head><body>
<nav>Home Markets Subscribe Sign in</nav>
<article>
<h1>Markets reset expectations</h1>
<p>Investors adjusted rate expectations after a broad set of economic releases changed the outlook for the coming quarters. The response moved through equities, rates, currencies, and commodities.</p>
<p>Market breadth improved while long-duration government bonds recovered from their intraday lows. Analysts said the cross-asset response reflected positioning as much as the incoming data.</p>
<p>The next inflation report and central-bank meeting are now the main events on the calendar. Traders will watch both the headline figures and the underlying distribution for confirmation.</p>
</article><footer>Privacy Terms Careers</footer></body></html>"#;

        let article = extract_article(html, "https://example.com/article").unwrap();

        assert!(article
            .body
            .join(" ")
            .contains("Investors adjusted rate expectations"));
        assert!(!article.body.join(" ").contains("Privacy Terms Careers"));
        assert_eq!(article.byline.as_deref(), Some("Ada Markets"));
        assert_eq!(article.site_name.as_deref(), Some("Market Wire"));
        assert_eq!(article.language.as_deref(), Some("en"));
    }

    #[test]
    fn canonical_urls_remove_tracking_and_resolve_relative_links() {
        assert_eq!(
            canonical_http_url(
                "/story?id=7&utm_source=rss&gclid=abc#section",
                Some("https://Example.com/feed.xml")
            )
            .as_deref(),
            Some("https://example.com/story?id=7")
        );
        assert!(canonical_http_url("javascript:alert(1)", None).is_none());
    }

    #[test]
    fn duplicate_syndicated_stories_merge_source_and_symbol_evidence() {
        let retrieved_at = DateTime::parse_from_rfc3339("2026-08-29T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let first = br#"<?xml version="1.0"?><rss version="2.0"><channel>
<title>Wire One</title><item><guid>one</guid><title>Apple expands services business</title>
<link>https://one.example/story?utm_source=rss</link><category>AAPL</category>
<pubDate>Sat, 29 Aug 2026 09:00:00 GMT</pubDate><description>First report.</description>
</item></channel></rss>"#;
        let second = br#"<?xml version="1.0"?><rss version="2.0"><channel>
<title>Wire Two</title><item><guid>two</guid><title>Apple expands services business</title>
<link>https://two.example/syndicated</link><category>Technology</category>
<pubDate>Sat, 29 Aug 2026 09:00:00 GMT</pubDate><description>A longer syndicated report with more context.</description>
</item></channel></rss>"#;
        let stories = merge_and_limit_stories(
            parse_feed_document_at(first, "https://one.example/feed", retrieved_at)
                .unwrap()
                .into_iter()
                .chain(
                    parse_feed_document_at(second, "https://two.example/feed", retrieved_at)
                        .unwrap(),
                )
                .collect(),
        );
        assert_eq!(stories.len(), 1);
        assert_eq!(stories[0].provenance.sources, ["Wire One", "Wire Two"]);
        assert_eq!(stories[0].related_symbols, ["AAPL"]);
        assert!(stories[0].summary.contains("longer syndicated report"));
        assert_eq!(
            stories[0].sentiment.input_digest,
            analyze_news_sentiment(
                &stories[0].headline.title,
                &stories[0].summary,
                &stories[0].provenance.categories,
                stories[0].provenance.published_at.as_deref().unwrap()
            )
            .input_digest
        );
    }

    #[test]
    fn failed_sources_keep_last_known_stories_with_stale_provenance() {
        let retrieved_at = DateTime::parse_from_rfc3339("2026-08-29T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let rss = br#"<?xml version="1.0"?><rss version="2.0"><channel>
<title>Resilient Wire</title><item><guid>story</guid><title>Markets open higher</title>
<link>https://example.com/story</link><pubDate>Sat, 29 Aug 2026 09:00:00 GMT</pubDate>
<description>Opening report.</description></item></channel></rss>"#;
        let feed_url = "https://example.com/feed".to_owned();
        let stories = parse_feed_document_at(rss, &feed_url, retrieved_at).unwrap();
        let previous = FeedState {
            workbench: NewsWorkbench {
                stories: stories.into_iter().map(|(_, story)| story).collect(),
                events: Vec::new(),
            },
            status: "LIVE".to_owned(),
        };
        let refreshed = assemble_refresh(
            &previous,
            std::slice::from_ref(&feed_url),
            vec![(feed_url.clone(), Err("example.com · timeout".to_owned()))],
            retrieved_at,
        );
        assert_eq!(refreshed.workbench.stories.len(), 1);
        assert_eq!(
            refreshed.workbench.stories[0].provenance.freshness,
            NewsFreshness::StaleSource
        );
        assert!(refreshed.status.contains("1 STALE"));
    }

    #[test]
    fn feed_requests_run_in_parallel_instead_of_accumulating_timeouts() {
        use std::{
            io::Write,
            net::TcpListener,
            time::{Duration, Instant},
        };

        fn delayed_feed() -> (String, thread::JoinHandle<()>) {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let worker = thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                thread::sleep(Duration::from_millis(500));
                let body = r#"<?xml version="1.0"?><rss version="2.0"><channel><title>Wire</title><item><guid>x</guid><title>Market update</title></item></channel></rss>"#;
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/rss+xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            });
            (format!("http://{address}/feed"), worker)
        }

        let (first, first_worker) = delayed_feed();
        let (second, second_worker) = delayed_feed();
        let client = Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap();
        let started = Instant::now();
        let outcomes = fetch_feeds_concurrently(&client, &[first, second], Utc::now());
        let elapsed = started.elapsed();
        first_worker.join().unwrap();
        second_worker.join().unwrap();
        assert!(outcomes.iter().all(|(_, result)| result.is_ok()));
        assert!(
            elapsed < Duration::from_millis(850),
            "parallel refresh took {elapsed:?}"
        );
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
            .map(|url| (*url, fetch_feed_at(&client, url, Utc::now())))
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
            .filter_map(|url| fetch_feed_at(&client, url, Utc::now()).ok())
            .flat_map(|stories| stories.into_iter().take(3))
            .filter_map(|(_, story)| story.url)
            .collect::<Vec<_>>();
        let readable = candidates.iter().find_map(|url| {
            fetch_article(&client, url)
                .ok()
                .map(|article| (url, article.body))
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
