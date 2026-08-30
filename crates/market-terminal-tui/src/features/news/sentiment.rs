const METHOD_VERSION: &str = "MT-LEXICON-1";
const INPUT_SCOPE: &str = "TITLE+SUMMARY+CATEGORIES";
const MAX_EVIDENCE: usize = 12;

const POSITIVE: [(&str, i16); 32] = [
    ("ADVANCE", 2),
    ("ADVANCED", 2),
    ("BEAT", 3),
    ("BEATS", 3),
    ("BREAKTHROUGH", 2),
    ("EXPAND", 1),
    ("EXPANDED", 1),
    ("GAIN", 2),
    ("GAINS", 2),
    ("GROWTH", 1),
    ("HIGHER", 2),
    ("IMPROVE", 2),
    ("IMPROVED", 2),
    ("OUTPERFORM", 3),
    ("OUTPERFORMS", 3),
    ("RAISE", 2),
    ("RAISED", 2),
    ("RALLY", 3),
    ("REBOUND", 2),
    ("RECORD", 1),
    ("RESILIENT", 1),
    ("RISE", 2),
    ("RISEN", 2),
    ("RISES", 2),
    ("STRONG", 2),
    ("STRONGER", 2),
    ("SURGE", 3),
    ("SURGES", 3),
    ("UPGRADE", 3),
    ("UPGRADED", 3),
    ("UPSIDE", 2),
    ("WIN", 2),
];

const NEGATIVE: [(&str, i16); 34] = [
    ("BANKRUPTCY", 4),
    ("CUT", 2),
    ("CUTS", 2),
    ("DECLINE", 2),
    ("DECLINED", 2),
    ("DEFAULT", 4),
    ("DOWNGRADE", 3),
    ("DOWNGRADED", 3),
    ("DOWNSIDE", 2),
    ("DROP", 2),
    ("DROPS", 2),
    ("FALL", 2),
    ("FELL", 2),
    ("FRAUD", 4),
    ("INVESTIGATION", 2),
    ("LAYOFF", 2),
    ("LAYOFFS", 2),
    ("LOSS", 2),
    ("LOSSES", 2),
    ("LOWER", 2),
    ("MISS", 3),
    ("MISSES", 3),
    ("PLUNGE", 4),
    ("PLUNGES", 4),
    ("RECALL", 2),
    ("SLUMP", 3),
    ("SLUMPS", 3),
    ("TUMBLE", 3),
    ("TUMBLES", 3),
    ("WEAK", 2),
    ("WEAKER", 2),
    ("WARNING", 2),
    ("WITHDRAW", 3),
    ("WITHDRAWS", 3),
];

const NEGATIONS: [&str; 7] = ["HARDLY", "NEVER", "NO", "NOT", "WITHOUT", "ISNT", "WASNT"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewsSentimentLabel {
    Positive,
    Negative,
    Mixed,
    Neutral,
    Unavailable,
}

impl NewsSentimentLabel {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Positive => "POSITIVE",
            Self::Negative => "NEGATIVE",
            Self::Mixed => "MIXED",
            Self::Neutral => "NEUTRAL",
            Self::Unavailable => "UNAVAILABLE",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SentimentPolarity {
    Positive,
    Negative,
}

impl SentimentPolarity {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Positive => "+",
            Self::Negative => "-",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewsSentimentEvidence {
    pub term: String,
    pub polarity: SentimentPolarity,
    pub weight: u8,
    pub negated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewsSentiment {
    pub label: NewsSentimentLabel,
    /// Signed tone score in basis points of the normalized [-1, 1] range.
    /// This is not a probability or expected-return estimate.
    pub score_bps: i16,
    /// Strength of lexical evidence, not predictive confidence.
    pub evidence_confidence_bps: u16,
    pub method_version: String,
    pub input_scope: String,
    pub observed_at: String,
    pub input_digest: String,
    pub evidence: Vec<NewsSentimentEvidence>,
    pub calibration: String,
    pub disclosure: String,
}

impl NewsSentiment {
    pub fn score_label(&self) -> String {
        format!("{:+.2}", f64::from(self.score_bps) / 100.0)
    }

    pub fn evidence_confidence_label(&self) -> String {
        format!("{:.2}%", f64::from(self.evidence_confidence_bps) / 100.0)
    }
}

pub fn analyze_news_sentiment(
    title: &str,
    summary: &str,
    categories: &[String],
    observed_at: &str,
) -> NewsSentiment {
    let input = format!("{title}\n{summary}\n{}", categories.join("\n"));
    let tokens = tokenize(&input);
    let mut evidence = Vec::new();
    let mut positive_weight = 0_i32;
    let mut negative_weight = 0_i32;

    for (index, token) in tokens.iter().enumerate() {
        let Some((base_polarity, weight)) = lexicon_entry(token) else {
            continue;
        };
        let negated = tokens[index.saturating_sub(3)..index]
            .iter()
            .rev()
            .take_while(|candidate| lexicon_entry(candidate).is_none())
            .any(|candidate| NEGATIONS.contains(&candidate.as_str()));
        let polarity = if negated {
            invert(base_polarity)
        } else {
            base_polarity
        };
        match polarity {
            SentimentPolarity::Positive => positive_weight += i32::from(weight),
            SentimentPolarity::Negative => negative_weight += i32::from(weight),
        }
        if evidence.len() < MAX_EVIDENCE {
            evidence.push(NewsSentimentEvidence {
                term: token.clone(),
                polarity,
                weight,
                negated,
            });
        }
    }

    let total_weight = positive_weight + negative_weight;
    let net_weight = positive_weight - negative_weight;
    let score_bps = if total_weight == 0 {
        0
    } else {
        (net_weight * 10_000 / (total_weight + 4)).clamp(-10_000, 10_000) as i16
    };
    let label = classify(score_bps, positive_weight, negative_weight);
    let evidence_confidence_bps =
        evidence_confidence(tokens.len(), total_weight, positive_weight, negative_weight);

    NewsSentiment {
        label,
        score_bps,
        evidence_confidence_bps,
        method_version: METHOD_VERSION.to_owned(),
        input_scope: INPUT_SCOPE.to_owned(),
        observed_at: observed_at.to_owned(),
        input_digest: format!("FNV1A64-{:016X}", fnv64(&input)),
        evidence,
        calibration: "UNCALIBRATED · NO OUTCOME PROBABILITY".to_owned(),
        disclosure: "LEXICAL TONE ONLY · NOT FACT, FORECAST, OR INVESTMENT SIGNAL".to_owned(),
    }
}

fn tokenize(value: &str) -> Vec<String> {
    value
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|token| token.len() > 1)
        .map(|token| token.to_ascii_uppercase())
        .take(2_048)
        .collect()
}

fn lexicon_entry(token: &str) -> Option<(SentimentPolarity, u8)> {
    POSITIVE
        .iter()
        .find(|(term, _)| *term == token)
        .map(|(_, weight)| (SentimentPolarity::Positive, *weight as u8))
        .or_else(|| {
            NEGATIVE
                .iter()
                .find(|(term, _)| *term == token)
                .map(|(_, weight)| (SentimentPolarity::Negative, *weight as u8))
        })
}

const fn invert(polarity: SentimentPolarity) -> SentimentPolarity {
    match polarity {
        SentimentPolarity::Positive => SentimentPolarity::Negative,
        SentimentPolarity::Negative => SentimentPolarity::Positive,
    }
}

fn classify(score_bps: i16, positive_weight: i32, negative_weight: i32) -> NewsSentimentLabel {
    if positive_weight + negative_weight == 0 {
        NewsSentimentLabel::Unavailable
    } else if positive_weight > 0 && negative_weight > 0 && score_bps.unsigned_abs() < 1_500 {
        NewsSentimentLabel::Mixed
    } else if score_bps >= 1_200 {
        NewsSentimentLabel::Positive
    } else if score_bps <= -1_200 {
        NewsSentimentLabel::Negative
    } else {
        NewsSentimentLabel::Neutral
    }
}

fn evidence_confidence(
    token_count: usize,
    total_weight: i32,
    positive_weight: i32,
    negative_weight: i32,
) -> u16 {
    if total_weight == 0 {
        return 0;
    }
    let coverage = (total_weight * 10_000 / token_count.max(1) as i32).min(2_000);
    let volume = (total_weight * 700).min(6_000);
    let conflict = positive_weight.min(negative_weight) * 350;
    (1_000 + coverage + volume - conflict).clamp(0, 8_500) as u16
}

fn fnv64(value: &str) -> u64 {
    value
        .bytes()
        .fold(14_695_981_039_346_656_037, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(1_099_511_628_211)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn analyze(title: &str, summary: &str) -> NewsSentiment {
        analyze_news_sentiment(title, summary, &[], "2026-08-29T12:00:00Z")
    }

    #[test]
    fn directional_examples_are_versioned_replayable_and_bounded() {
        let positive = analyze(
            "Shares surge after earnings beat",
            "Management raised guidance as growth improved.",
        );
        let repeated = analyze(
            "Shares surge after earnings beat",
            "Management raised guidance as growth improved.",
        );
        assert_eq!(positive, repeated);
        assert_eq!(positive.label, NewsSentimentLabel::Positive);
        assert!(positive.score_bps > 0);
        assert!(positive.evidence.len() <= MAX_EVIDENCE);
        assert_eq!(positive.method_version, METHOD_VERSION);
        assert!(positive.input_digest.starts_with("FNV1A64-"));

        let negative = analyze(
            "Shares plunge after earnings miss",
            "The company withdrew guidance and announced layoffs.",
        );
        assert_eq!(negative.label, NewsSentimentLabel::Negative);
        assert!(negative.score_bps < 0);
        assert_ne!(positive.input_digest, negative.input_digest);
    }

    #[test]
    fn negation_changes_contribution_instead_of_silently_dropping_it() {
        let sentiment = analyze("Outlook is not weak", "Demand improved.");
        let weak = sentiment
            .evidence
            .iter()
            .find(|evidence| evidence.term == "WEAK")
            .unwrap();
        assert!(weak.negated);
        assert_eq!(weak.polarity, SentimentPolarity::Positive);
        assert_eq!(sentiment.label, NewsSentimentLabel::Positive);
    }

    #[test]
    fn conflicting_evidence_is_mixed_and_not_presented_as_probability() {
        let sentiment = analyze("Strong growth meets weak demand", "");
        assert_eq!(sentiment.label, NewsSentimentLabel::Mixed);
        assert!(sentiment.calibration.contains("NO OUTCOME PROBABILITY"));
        assert!(sentiment.disclosure.contains("NOT FACT"));
    }

    #[test]
    fn absent_lexical_evidence_is_unavailable_not_neutral_fact() {
        let sentiment = analyze(
            "Company schedules annual meeting",
            "Agenda published Friday.",
        );
        assert_eq!(sentiment.label, NewsSentimentLabel::Unavailable);
        assert_eq!(sentiment.score_bps, 0);
        assert_eq!(sentiment.evidence_confidence_bps, 0);
        assert!(sentiment.evidence.is_empty());
    }
}
