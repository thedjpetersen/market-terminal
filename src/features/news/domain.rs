use crate::foundation::InstrumentId;

use super::{analyze_news_sentiment, NewsSentiment};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Headline {
    pub time: String,
    pub topic: String,
    pub title: String,
    pub region: String,
}

impl Headline {
    pub fn story_id(&self) -> String {
        format!("{}:{}:{}", self.time, self.topic, self.title)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewsSnapshot {
    pub headlines: Vec<Headline>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewsStory {
    pub id: String,
    pub headline: Headline,
    pub byline: String,
    pub summary: String,
    pub body: Vec<String>,
    pub body_state: ArticleBodyState,
    pub related_symbols: Vec<String>,
    pub instruments: Vec<InstrumentId>,
    pub url: Option<String>,
    pub provenance: NewsProvenance,
    pub sentiment: NewsSentiment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewsFreshness {
    Fresh,
    StaleSource,
}

impl NewsFreshness {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Fresh => "FRESH",
            Self::StaleSource => "STALE SOURCE",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewsProvenance {
    pub sources: Vec<String>,
    pub feed_urls: Vec<String>,
    pub published_at: Option<String>,
    pub retrieved_at: String,
    pub categories: Vec<String>,
    pub language: Option<String>,
    pub freshness: NewsFreshness,
}

impl NewsProvenance {
    pub fn deterministic(topic: &str) -> Self {
        Self {
            sources: vec!["DETERMINISTIC REPLAY".to_owned()],
            feed_urls: Vec::new(),
            published_at: None,
            retrieved_at: "REPLAY".to_owned(),
            categories: vec![topic.to_owned()],
            language: None,
            freshness: NewsFreshness::Fresh,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ArticleBodyState {
    #[default]
    ExcerptOnly,
    FeedProvided,
    Loading,
    Downloaded,
    Unavailable(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventImportance {
    High,
    Medium,
    Low,
}

impl EventImportance {
    pub const fn label(self) -> &'static str {
        match self {
            Self::High => "HIGH",
            Self::Medium => "MED",
            Self::Low => "LOW",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewsEvent {
    pub time: String,
    pub region: String,
    pub importance: EventImportance,
    pub event: String,
    pub period: String,
    pub survey: String,
    pub prior: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewsFilter {
    pub region: Option<String>,
    pub topic: Option<String>,
    pub symbol: Option<String>,
    pub unread_only: bool,
    pub bookmarked_only: bool,
}

impl NewsFilter {
    pub fn matches(&self, story: &NewsStory, is_read: bool, is_bookmarked: bool) -> bool {
        if self.unread_only && is_read {
            return false;
        }
        if self.bookmarked_only && !is_bookmarked {
            return false;
        }
        if self
            .region
            .as_ref()
            .is_some_and(|region| !story.headline.region.eq_ignore_ascii_case(region))
        {
            return false;
        }
        if self
            .topic
            .as_ref()
            .is_some_and(|topic| !story.headline.topic.eq_ignore_ascii_case(topic))
        {
            return false;
        }
        if self.symbol.as_ref().is_some_and(|symbol| {
            !story
                .related_symbols
                .iter()
                .any(|candidate| candidate.eq_ignore_ascii_case(symbol))
        }) {
            return false;
        }
        true
    }

    pub fn is_active(&self) -> bool {
        self.region.is_some()
            || self.topic.is_some()
            || self.symbol.is_some()
            || self.unread_only
            || self.bookmarked_only
    }
}

#[derive(Debug, Clone)]
pub struct NewsWorkbench {
    pub stories: Vec<NewsStory>,
    pub events: Vec<NewsEvent>,
}

const MARKET_BODY: &[&str] = &[
    "Global equities advanced as resilient earnings and a measured rates outlook supported risk appetite.",
    "Breadth improved through the session, with gains extending beyond the largest technology companies.",
    "Investors now turn to inflation data and central-bank guidance for confirmation of the policy path.",
];
const POLICY_BODY: &[&str] = &[
    "Policy makers said recent data are moving in the right direction, while emphasizing that decisions remain data dependent.",
    "Rates markets brought forward the expected timing of the next move as the front end of the curve rallied.",
    "The next employment and inflation releases remain the key catalysts for consensus forecasts.",
];
const COMMODITY_BODY: &[&str] = &[
    "Commodity markets repriced the demand outlook while traders assessed inventories and incoming industrial data.",
    "The move carried into currencies and shares of producers, producing a broad cross-asset response.",
    "Positioning remains light ahead of the next supply update.",
];

impl NewsWorkbench {
    pub fn from_snapshot(snapshot: NewsSnapshot) -> Self {
        let stories = snapshot.headlines.into_iter().map(|headline| {
            let (byline, summary, body, related_symbols): (_, _, _, &[&str]) =
                match headline.topic.as_str() {
                    "FED" | "ECO" | "POL" => (
                        "ECONOMICS DESK",
                        "Policy expectations shift as investors digest the latest official guidance.",
                        POLICY_BODY,
                        &["SPY", "TLT", "DXY"],
                    ),
                    "CMD" => (
                        "COMMODITIES DESK",
                        "Raw materials move as supply signals meet a changing demand outlook.",
                        COMMODITY_BODY,
                        &["CL1", "XLE", "FCX"],
                    ),
                    "TEC" => (
                        "TECHNOLOGY DESK",
                        "Semiconductors lead as a stronger forecast reinforces AI infrastructure demand.",
                        MARKET_BODY,
                        &["NVDA", "MU", "AVGO"],
                    ),
                    _ => (
                        "MARKETS DESK",
                        "Risk assets advance in broad trade as earnings support the growth outlook.",
                        MARKET_BODY,
                        &["SPY", "AAPL", "MSFT"],
                    ),
                };
            let related_symbols = related_symbols.iter().map(|symbol| (*symbol).to_owned()).collect::<Vec<_>>();
            let instruments = related_symbols.iter().map(|symbol| {
                InstrumentId::new(format!("us:listed:{}", symbol.to_ascii_lowercase()))
            }).collect();
            let sentiment = analyze_news_sentiment(
                &headline.title,
                summary,
                std::slice::from_ref(&headline.topic),
                "REPLAY",
            );
            NewsStory {
                id: headline.story_id(),
                provenance: NewsProvenance::deterministic(&headline.topic),
                sentiment,
                headline,
                byline: byline.to_owned(),
                summary: summary.to_owned(),
                body: body.iter().map(|paragraph| (*paragraph).to_owned()).collect(),
                body_state: ArticleBodyState::FeedProvided,
                related_symbols,
                instruments,
                url: None,
            }
        }).collect();
        Self {
            stories,
            events: demo_events(),
        }
    }
}

fn demo_events() -> Vec<NewsEvent> {
    [
        (
            "08:30",
            "US",
            EventImportance::High,
            "CORE PCE PRICE INDEX",
            "JUL",
            "+0.2%",
            "+0.3%",
        ),
        (
            "08:30",
            "US",
            EventImportance::Medium,
            "INITIAL JOBLESS CLAIMS",
            "AUG 22",
            "232K",
            "228K",
        ),
        (
            "10:00",
            "US",
            EventImportance::High,
            "CONSUMER CONFIDENCE",
            "AUG",
            "98.2",
            "97.4",
        ),
        (
            "11:00",
            "EU",
            EventImportance::Medium,
            "ECB 1Y INFLATION EXPECTATION",
            "JUL",
            "2.6%",
            "2.7%",
        ),
        (
            "14:00",
            "US",
            EventImportance::Low,
            "FED BEIGE BOOK",
            "AUG",
            "—",
            "—",
        ),
        (
            "22:00",
            "AS",
            EventImportance::High,
            "CHINA MANUFACTURING PMI",
            "AUG",
            "49.8",
            "49.5",
        ),
    ]
    .into_iter()
    .map(
        |(time, region, importance, event, period, survey, prior)| NewsEvent {
            time: time.to_owned(),
            region: region.to_owned(),
            importance,
            event: event.to_owned(),
            period: period.to_owned(),
            survey: survey.to_owned(),
            prior: prior.to_owned(),
        },
    )
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headlines() -> Vec<Headline> {
        vec![
            Headline {
                time: "16:00".into(),
                topic: "TOP".into(),
                title: "Stocks gain".into(),
                region: "US".into(),
            },
            Headline {
                time: "14:00".into(),
                topic: "TEC".into(),
                title: "Chip rally".into(),
                region: "AS".into(),
            },
        ]
    }

    #[test]
    fn filters_compose_across_region_topic_and_symbol() {
        let workbench = NewsWorkbench::from_snapshot(NewsSnapshot {
            headlines: headlines(),
        });
        let filter = NewsFilter {
            region: Some("AS".into()),
            topic: Some("TEC".into()),
            symbol: Some("NVDA".into()),
            ..NewsFilter::default()
        };
        assert!(!filter.matches(&workbench.stories[0], false, false));
        assert!(filter.matches(&workbench.stories[1], false, false));
    }

    #[test]
    fn stories_carry_provider_neutral_instrument_links() {
        let workbench = NewsWorkbench::from_snapshot(NewsSnapshot {
            headlines: headlines(),
        });
        assert_eq!(
            workbench.stories[1].instruments[0].as_str(),
            "us:listed:nvda"
        );
    }
}
