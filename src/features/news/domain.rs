use crate::foundation::InstrumentId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Headline {
    pub time: &'static str,
    pub topic: &'static str,
    pub title: &'static str,
    pub region: &'static str,
}

impl Headline {
    pub fn story_id(self) -> String {
        format!("{}:{}:{}", self.time, self.topic, self.title)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct NewsSnapshot {
    pub headlines: &'static [Headline],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewsStory {
    pub id: String,
    pub headline: Headline,
    pub byline: &'static str,
    pub summary: &'static str,
    pub body: &'static [&'static str],
    pub related_symbols: &'static [&'static str],
    pub instruments: Vec<InstrumentId>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NewsEvent {
    pub time: &'static str,
    pub region: &'static str,
    pub importance: EventImportance,
    pub event: &'static str,
    pub period: &'static str,
    pub survey: &'static str,
    pub prior: &'static str,
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
        if self.region.as_ref().is_some_and(|region| {
            !story.headline.region.eq_ignore_ascii_case(region)
        }) {
            return false;
        }
        if self.topic.as_ref().is_some_and(|topic| {
            !story.headline.topic.eq_ignore_ascii_case(topic)
        }) {
            return false;
        }
        if self.symbol.as_ref().is_some_and(|symbol| {
            !story.related_symbols.iter().any(|candidate| candidate.eq_ignore_ascii_case(symbol))
        }) {
            return false;
        }
        true
    }

    pub fn is_active(&self) -> bool {
        self.region.is_some() || self.topic.is_some() || self.symbol.is_some()
            || self.unread_only || self.bookmarked_only
    }
}

#[derive(Debug, Clone)]
pub struct NewsWorkbench {
    pub stories: Vec<NewsStory>,
    pub events: &'static [NewsEvent],
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

const EVENTS: [NewsEvent; 6] = [
    NewsEvent { time: "08:30", region: "US", importance: EventImportance::High, event: "CORE PCE PRICE INDEX", period: "JUL", survey: "+0.2%", prior: "+0.3%" },
    NewsEvent { time: "08:30", region: "US", importance: EventImportance::Medium, event: "INITIAL JOBLESS CLAIMS", period: "AUG 22", survey: "232K", prior: "228K" },
    NewsEvent { time: "10:00", region: "US", importance: EventImportance::High, event: "CONSUMER CONFIDENCE", period: "AUG", survey: "98.2", prior: "97.4" },
    NewsEvent { time: "11:00", region: "EU", importance: EventImportance::Medium, event: "ECB 1Y INFLATION EXPECTATION", period: "JUL", survey: "2.6%", prior: "2.7%" },
    NewsEvent { time: "14:00", region: "US", importance: EventImportance::Low, event: "FED BEIGE BOOK", period: "AUG", survey: "—", prior: "—" },
    NewsEvent { time: "22:00", region: "AS", importance: EventImportance::High, event: "CHINA MANUFACTURING PMI", period: "AUG", survey: "49.8", prior: "49.5" },
];

impl NewsWorkbench {
    pub fn from_snapshot(snapshot: NewsSnapshot) -> Self {
        let stories = snapshot.headlines.iter().copied().map(|headline| {
            let (byline, summary, body, related_symbols): (_, _, _, &'static [&'static str]) =
                match headline.topic {
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
            let instruments = related_symbols.iter().map(|symbol| {
                InstrumentId::new(format!("us:listed:{}", symbol.to_ascii_lowercase()))
            }).collect();
            NewsStory {
                id: headline.story_id(), headline, byline, summary, body, related_symbols,
                instruments,
            }
        }).collect();
        Self { stories, events: &EVENTS }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADLINES: [Headline; 2] = [
        Headline { time: "16:00", topic: "TOP", title: "Stocks gain", region: "US" },
        Headline { time: "14:00", topic: "TEC", title: "Chip rally", region: "AS" },
    ];

    #[test]
    fn filters_compose_across_region_topic_and_symbol() {
        let workbench = NewsWorkbench::from_snapshot(NewsSnapshot { headlines: &HEADLINES });
        let filter = NewsFilter {
            region: Some("AS".into()), topic: Some("TEC".into()),
            symbol: Some("NVDA".into()), ..NewsFilter::default()
        };
        assert!(!filter.matches(&workbench.stories[0], false, false));
        assert!(filter.matches(&workbench.stories[1], false, false));
    }

    #[test]
    fn stories_carry_provider_neutral_instrument_links() {
        let workbench = NewsWorkbench::from_snapshot(NewsSnapshot { headlines: &HEADLINES });
        assert_eq!(workbench.stories[1].instruments[0].as_str(), "us:listed:nvda");
    }
}
