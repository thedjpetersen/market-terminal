use std::fmt;

use crate::foundation::InstrumentId;

pub const MAX_COMPARISONS: usize = 3;

/// Charting's stable read-model reference to an instrument.
///
/// The canonical identifier is deliberately separate from the display symbol so
/// provider symbols can change without changing a saved chart specification.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChartInstrument {
    pub canonical_id: InstrumentId,
    pub symbol: String,
}

impl ChartInstrument {
    pub fn new(canonical_id: impl Into<String>, symbol: impl Into<String>) -> Self {
        Self {
            canonical_id: InstrumentId::new(canonical_id),
            symbol: symbol.into(),
        }
    }

    pub fn from_terminal_subject(subject: &str) -> Self {
        let symbol = subject.trim().to_ascii_uppercase();
        let canonical_id = format!("terminal:{}", symbol.to_ascii_lowercase().replace(' ', ":"));
        Self::new(canonical_id, symbol)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChartPeriod {
    OneDay,
    OneMonth,
    SixMonths,
    YearToDate,
    OneYear,
    FiveYears,
}

impl ChartPeriod {
    pub const ALL: [Self; 6] = [
        Self::OneDay,
        Self::OneMonth,
        Self::SixMonths,
        Self::YearToDate,
        Self::OneYear,
        Self::FiveYears,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::OneDay => "1D",
            Self::OneMonth => "1M",
            Self::SixMonths => "6M",
            Self::YearToDate => "YTD",
            Self::OneYear => "1Y",
            Self::FiveYears => "5Y",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|period| period.label().eq_ignore_ascii_case(value))
    }

    pub fn next(self) -> Self {
        let index = Self::ALL
            .iter()
            .position(|candidate| *candidate == self)
            .unwrap_or(0);
        Self::ALL[(index + 1) % Self::ALL.len()]
    }

    pub fn previous(self) -> Self {
        let index = Self::ALL
            .iter()
            .position(|candidate| *candidate == self)
            .unwrap_or(0);
        Self::ALL[(index + Self::ALL.len() - 1) % Self::ALL.len()]
    }

    pub const fn sample_count(self) -> usize {
        match self {
            Self::OneDay => 78,
            Self::OneMonth => 22,
            Self::SixMonths => 126,
            Self::YearToDate => 168,
            Self::OneYear => 252,
            Self::FiveYears => 260,
        }
    }

    pub const fn sample_interval_seconds(self) -> i64 {
        match self {
            Self::OneDay => 300,
            Self::OneMonth | Self::SixMonths | Self::YearToDate | Self::OneYear => 86_400,
            Self::FiveYears => 604_800,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Normalization {
    Price,
    PercentChange,
}

impl Normalization {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Price => "PRICE",
            Self::PercentChange => "% CHANGE",
        }
    }

    pub const fn toggled(self) -> Self {
        match self {
            Self::Price => Self::PercentChange,
            Self::PercentChange => Self::Price,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Study {
    SimpleMovingAverage { window: usize },
    ExponentialMovingAverage { window: usize },
    RelativeStrengthIndex { period: usize },
    Volume,
}

impl Study {
    pub fn label(self) -> String {
        match self {
            Self::SimpleMovingAverage { window } => format!("SMA {window}"),
            Self::ExponentialMovingAverage { window } => format!("EMA {window}"),
            Self::RelativeStrengthIndex { period } => format!("RSI {period}"),
            Self::Volume => "VOLUME".to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChartSpecError {
    PrimaryCannotBeComparison,
    DuplicateComparison,
    ComparisonLimitReached,
    InvalidStudyWindow,
}

impl fmt::Display for ChartSpecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::PrimaryCannotBeComparison => "the primary instrument cannot compare with itself",
            Self::DuplicateComparison => "the comparison is already present",
            Self::ComparisonLimitReached => "the chart supports at most three comparisons",
            Self::InvalidStudyWindow => "indicator windows must be greater than one",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for ChartSpecError {}

/// A serializable-in-spirit, provider-independent chart specification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChartSpecification {
    pub primary: ChartInstrument,
    pub period: ChartPeriod,
    pub normalization: Normalization,
    pub comparisons: Vec<ChartInstrument>,
    pub studies: Vec<Study>,
}

impl ChartSpecification {
    pub fn new(primary: ChartInstrument) -> Self {
        Self {
            primary,
            period: ChartPeriod::OneYear,
            normalization: Normalization::Price,
            comparisons: Vec::new(),
            studies: vec![
                Study::SimpleMovingAverage { window: 20 },
                Study::SimpleMovingAverage { window: 100 },
                Study::RelativeStrengthIndex { period: 14 },
                Study::Volume,
            ],
        }
    }

    pub fn set_primary(&mut self, primary: ChartInstrument) {
        self.comparisons
            .retain(|comparison| comparison.canonical_id != primary.canonical_id);
        self.primary = primary;
    }

    pub fn add_comparison(&mut self, comparison: ChartInstrument) -> Result<(), ChartSpecError> {
        if comparison.canonical_id == self.primary.canonical_id {
            return Err(ChartSpecError::PrimaryCannotBeComparison);
        }
        if self
            .comparisons
            .iter()
            .any(|current| current.canonical_id == comparison.canonical_id)
        {
            return Err(ChartSpecError::DuplicateComparison);
        }
        if self.comparisons.len() >= MAX_COMPARISONS {
            return Err(ChartSpecError::ComparisonLimitReached);
        }
        self.comparisons.push(comparison);
        Ok(())
    }

    pub fn toggle_study(&mut self, study: Study) -> Result<(), ChartSpecError> {
        if matches!(
            study,
            Study::SimpleMovingAverage { window }
                | Study::ExponentialMovingAverage { window }
                if window < 2
        ) || matches!(study, Study::RelativeStrengthIndex { period } if period < 2)
        {
            return Err(ChartSpecError::InvalidStudyWindow);
        }
        if let Some(index) = self.studies.iter().position(|current| *current == study) {
            self.studies.remove(index);
        } else {
            self.studies.push(study);
        }
        Ok(())
    }

    pub fn has_study(&self, study: Study) -> bool {
        self.studies.contains(&study)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PriceBar {
    pub timestamp: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryQuality {
    Replayed,
    Delayed,
    Live,
    Derived,
}

impl HistoryQuality {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Replayed => "REPLAY",
            Self::Delayed => "DELAYED",
            Self::Live => "LIVE",
            Self::Derived => "DERIVED",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct HistorySeries {
    pub instrument: ChartInstrument,
    pub bars: Vec<PriceBar>,
    pub quality: HistoryQuality,
    pub source: String,
}

pub(crate) fn percent_change(values: &[f64]) -> Vec<f64> {
    let Some(base) = values.first().copied() else {
        return Vec::new();
    };
    if base.abs() < f64::EPSILON {
        return vec![0.0; values.len()];
    }
    values
        .iter()
        .map(|value| ((value / base) - 1.0) * 100.0)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instrument(symbol: &str) -> ChartInstrument {
        ChartInstrument::from_terminal_subject(symbol)
    }

    #[test]
    fn chart_spec_rejects_duplicate_self_and_excess_comparisons() {
        let mut spec = ChartSpecification::new(instrument("AAPL"));
        assert_eq!(
            spec.add_comparison(instrument("AAPL")),
            Err(ChartSpecError::PrimaryCannotBeComparison)
        );
        assert_eq!(spec.add_comparison(instrument("MSFT")), Ok(()));
        assert_eq!(
            spec.add_comparison(instrument("MSFT")),
            Err(ChartSpecError::DuplicateComparison)
        );
        assert_eq!(spec.add_comparison(instrument("SPY")), Ok(()));
        assert_eq!(spec.add_comparison(instrument("QQQ")), Ok(()));
        assert_eq!(
            spec.add_comparison(instrument("NVDA")),
            Err(ChartSpecError::ComparisonLimitReached)
        );
    }

    #[test]
    fn percent_change_normalizes_each_series_to_zero() {
        let normalized = percent_change(&[200.0, 210.0, 190.0]);
        assert!(normalized[0].abs() < 1e-10);
        assert!((normalized[1] - 5.0).abs() < 1e-10);
        assert!((normalized[2] + 5.0).abs() < 1e-10);
        assert_eq!(percent_change(&[0.0, 2.0]), vec![0.0, 0.0]);
    }

    #[test]
    fn period_navigation_wraps() {
        assert_eq!(ChartPeriod::FiveYears.next(), ChartPeriod::OneDay);
        assert_eq!(ChartPeriod::OneDay.previous(), ChartPeriod::FiveYears);
        assert_eq!(ChartPeriod::parse("ytd"), Some(ChartPeriod::YearToDate));
    }
}
