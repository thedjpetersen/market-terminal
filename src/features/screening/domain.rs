use std::{cmp::Ordering, collections::BTreeSet, fmt};

use serde::{Deserialize, Serialize};

use crate::foundation::InstrumentId;

pub const MAX_UNIVERSE_MEMBERS: usize = 2_000;
pub const MAX_SCREEN_CLAUSES: usize = 8;
pub const MAX_SCREEN_RESULTS: usize = 200;
pub const MAX_SAVED_SCREENS: usize = 64;
const MAX_ID_BYTES: usize = 64;
const MAX_LABEL_BYTES: usize = 96;
const MAX_MEMBER_TEXT_BYTES: usize = 160;
const MAX_METADATA_BYTES: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScreenField {
    Last,
    ChangePercent,
    Volume,
    SpreadBps,
    DayRangePercent,
}

impl ScreenField {
    pub const fn key(self) -> &'static str {
        match self {
            Self::Last => "last",
            Self::ChangePercent => "change_pct",
            Self::Volume => "volume",
            Self::SpreadBps => "spread_bps",
            Self::DayRangePercent => "day_range_pct",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Last => "LAST",
            Self::ChangePercent => "% CHG",
            Self::Volume => "VOLUME",
            Self::SpreadBps => "SPREAD BP",
            Self::DayRangePercent => "DAY RANGE %",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "last" | "price" | "px_last" => Some(Self::Last),
            "change" | "change_pct" | "pct_change" | "%chg" => Some(Self::ChangePercent),
            "volume" | "vol" => Some(Self::Volume),
            "spread" | "spread_bps" | "spread_bp" => Some(Self::SpreadBps),
            "range" | "day_range" | "day_range_pct" => Some(Self::DayRangePercent),
            _ => None,
        }
    }

    pub const fn value(self, member: &UniverseMember) -> Option<f64> {
        match self {
            Self::Last => member.last,
            Self::ChangePercent => member.change_percent,
            Self::Volume => member.volume,
            Self::SpreadBps => member.spread_bps,
            Self::DayRangePercent => member.day_range_percent,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Comparison {
    GreaterThan,
    GreaterThanOrEqual,
    LessThan,
    LessThanOrEqual,
    Equal,
}

impl Comparison {
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::GreaterThan => ">",
            Self::GreaterThanOrEqual => ">=",
            Self::LessThan => "<",
            Self::LessThanOrEqual => "<=",
            Self::Equal => "=",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            ">" => Some(Self::GreaterThan),
            ">=" => Some(Self::GreaterThanOrEqual),
            "<" => Some(Self::LessThan),
            "<=" => Some(Self::LessThanOrEqual),
            "=" | "==" => Some(Self::Equal),
            _ => None,
        }
    }

    fn matches(self, actual: f64, expected: f64) -> bool {
        match self {
            Self::GreaterThan => actual > expected,
            Self::GreaterThanOrEqual => actual >= expected,
            Self::LessThan => actual < expected,
            Self::LessThanOrEqual => actual <= expected,
            Self::Equal => (actual - expected).abs() <= f64::EPSILON,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScreenClause {
    pub field: ScreenField,
    pub comparison: Comparison,
    pub value: f64,
}

impl ScreenClause {
    pub fn new(
        field: ScreenField,
        comparison: Comparison,
        value: f64,
    ) -> Result<Self, ScreenError> {
        if !value.is_finite() {
            return Err(ScreenError::InvalidThreshold);
        }
        Ok(Self {
            field,
            comparison,
            value,
        })
    }

    pub fn label(&self) -> String {
        format!(
            "{} {} {}",
            self.field.label(),
            self.comparison.symbol(),
            format_value(self.field, self.value)
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScreenSortDirection {
    Ascending,
    Descending,
}

impl ScreenSortDirection {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ascending => "ASC",
            Self::Descending => "DESC",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_uppercase().as_str() {
            "ASC" | "ASCENDING" => Some(Self::Ascending),
            "DESC" | "DESCENDING" => Some(Self::Descending),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScreenDefinition {
    pub id: String,
    pub label: String,
    pub universe_id: String,
    pub clauses: Vec<ScreenClause>,
    pub sort_field: ScreenField,
    pub sort_direction: ScreenSortDirection,
    pub limit: usize,
    #[serde(default)]
    pub built_in: bool,
}

impl ScreenDefinition {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: impl Into<String>,
        label: impl Into<String>,
        universe_id: impl Into<String>,
        clauses: Vec<ScreenClause>,
        sort_field: ScreenField,
        sort_direction: ScreenSortDirection,
        limit: usize,
        built_in: bool,
    ) -> Result<Self, ScreenError> {
        let definition = Self {
            id: id.into(),
            label: label.into(),
            universe_id: universe_id.into(),
            clauses,
            sort_field,
            sort_direction,
            limit,
            built_in,
        };
        definition.validate()?;
        Ok(definition)
    }

    pub fn validate(&self) -> Result<(), ScreenError> {
        validate_id(&self.id, "screen ID")?;
        validate_id(&self.universe_id, "universe ID")?;
        if self.label.trim().is_empty() || self.label.len() > MAX_LABEL_BYTES {
            return Err(ScreenError::InvalidLabel);
        }
        if self.clauses.is_empty() || self.clauses.len() > MAX_SCREEN_CLAUSES {
            return Err(ScreenError::InvalidClauseCount);
        }
        if self.clauses.iter().any(|clause| !clause.value.is_finite()) {
            return Err(ScreenError::InvalidThreshold);
        }
        if !(1..=MAX_SCREEN_RESULTS).contains(&self.limit) {
            return Err(ScreenError::InvalidLimit);
        }
        Ok(())
    }

    pub fn expression(&self) -> String {
        self.clauses
            .iter()
            .map(ScreenClause::label)
            .collect::<Vec<_>>()
            .join(" AND ")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct UniverseMember {
    pub instrument_id: InstrumentId,
    pub symbol: String,
    pub description: String,
    pub currency: String,
    pub last: Option<f64>,
    pub change_percent: Option<f64>,
    pub volume: Option<f64>,
    pub spread_bps: Option<f64>,
    pub day_range_percent: Option<f64>,
    pub quality: String,
    pub provider: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UniverseSnapshot {
    pub id: String,
    pub label: String,
    pub version: u64,
    pub as_of: String,
    pub source: String,
    pub members: Vec<UniverseMember>,
}

impl UniverseSnapshot {
    pub fn new(
        id: impl Into<String>,
        label: impl Into<String>,
        version: u64,
        as_of: impl Into<String>,
        source: impl Into<String>,
        members: Vec<UniverseMember>,
    ) -> Result<Self, ScreenError> {
        let snapshot = Self {
            id: id.into(),
            label: label.into(),
            version,
            as_of: as_of.into(),
            source: source.into(),
            members,
        };
        validate_id(&snapshot.id, "universe ID")?;
        if snapshot.label.trim().is_empty() || snapshot.label.len() > MAX_LABEL_BYTES {
            return Err(ScreenError::InvalidLabel);
        }
        if !valid_bounded_text(&snapshot.as_of, MAX_METADATA_BYTES, false)
            || !valid_bounded_text(&snapshot.source, MAX_METADATA_BYTES, false)
        {
            return Err(ScreenError::MissingMetadata);
        }
        if snapshot.members.len() > MAX_UNIVERSE_MEMBERS {
            return Err(ScreenError::UniverseTooLarge);
        }
        let mut identities = BTreeSet::new();
        if snapshot.members.iter().any(|member| {
            !valid_member(member) || !identities.insert(member.instrument_id.as_str().to_owned())
        }) {
            return Err(ScreenError::InvalidUniverseMember);
        }
        Ok(snapshot)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClauseEvidence {
    pub clause: ScreenClause,
    pub actual: Option<f64>,
    pub passed: bool,
}

impl ClauseEvidence {
    pub fn label(&self) -> String {
        match self.actual {
            Some(actual) => format!(
                "{} · ACTUAL {} · {}",
                self.clause.label(),
                format_value(self.clause.field, actual),
                if self.passed { "PASS" } else { "FAIL" }
            ),
            None => format!("{} · MISSING · FAIL", self.clause.label()),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScreenResultRow {
    pub rank: usize,
    pub member: UniverseMember,
    pub sort_value: f64,
    pub evidence: Vec<ClauseEvidence>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScreenExclusion {
    pub instrument_id: InstrumentId,
    pub symbol: String,
    pub evidence: Vec<ClauseEvidence>,
    pub missing_sort_value: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScreenEvaluation {
    pub definition: ScreenDefinition,
    pub universe: UniverseSnapshot,
    pub rows: Vec<ScreenResultRow>,
    pub exclusions: Vec<ScreenExclusion>,
    pub coverage_count: usize,
    pub truncated_count: usize,
}

impl ScreenEvaluation {
    pub fn input_count(&self) -> usize {
        self.universe.members.len()
    }

    pub fn coverage_percent(&self) -> f64 {
        if self.input_count() == 0 {
            0.0
        } else {
            self.coverage_count as f64 * 100.0 / self.input_count() as f64
        }
    }
}

pub fn evaluate_screen(
    definition: &ScreenDefinition,
    universe: UniverseSnapshot,
) -> Result<ScreenEvaluation, ScreenError> {
    definition.validate()?;
    if universe.id != definition.universe_id {
        return Err(ScreenError::WrongUniverse {
            expected: definition.universe_id.clone(),
            actual: universe.id,
        });
    }

    let mut accepted = Vec::new();
    let mut exclusions = Vec::new();
    let mut coverage_count = 0;
    for member in &universe.members {
        let evidence = definition
            .clauses
            .iter()
            .map(|clause| {
                let actual = clause.field.value(member);
                ClauseEvidence {
                    clause: clause.clone(),
                    actual,
                    passed: actual
                        .is_some_and(|actual| clause.comparison.matches(actual, clause.value)),
                }
            })
            .collect::<Vec<_>>();
        let sort_value = definition.sort_field.value(member);
        let complete = evidence.iter().all(|item| item.actual.is_some()) && sort_value.is_some();
        coverage_count += usize::from(complete);
        if let Some(sort_value) = sort_value.filter(|_| evidence.iter().all(|item| item.passed)) {
            accepted.push((member.clone(), sort_value, evidence));
        } else {
            exclusions.push(ScreenExclusion {
                instrument_id: member.instrument_id.clone(),
                symbol: member.symbol.clone(),
                evidence,
                missing_sort_value: sort_value.is_none(),
            });
        }
    }

    accepted.sort_by(|left, right| {
        let values = left.1.partial_cmp(&right.1).unwrap_or(Ordering::Equal);
        let values = match definition.sort_direction {
            ScreenSortDirection::Ascending => values,
            ScreenSortDirection::Descending => values.reverse(),
        };
        values.then_with(|| left.0.instrument_id.cmp(&right.0.instrument_id))
    });
    let truncated_count = accepted.len().saturating_sub(definition.limit);
    accepted.truncate(definition.limit);
    let rows = accepted
        .into_iter()
        .enumerate()
        .map(|(index, (member, sort_value, evidence))| ScreenResultRow {
            rank: index + 1,
            member,
            sort_value,
            evidence,
        })
        .collect();

    Ok(ScreenEvaluation {
        definition: definition.clone(),
        universe,
        rows,
        exclusions,
        coverage_count,
        truncated_count,
    })
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScreenCatalogState {
    pub schema_version: u16,
    pub revision: u64,
    pub screens: Vec<ScreenDefinition>,
}

impl ScreenCatalogState {
    pub const SCHEMA_VERSION: u16 = 1;

    pub fn new(revision: u64, screens: Vec<ScreenDefinition>) -> Result<Self, ScreenError> {
        let state = Self {
            schema_version: Self::SCHEMA_VERSION,
            revision,
            screens,
        };
        state.validate()?;
        Ok(state)
    }

    pub fn validate(&self) -> Result<(), ScreenError> {
        if self.schema_version != Self::SCHEMA_VERSION {
            return Err(ScreenError::UnsupportedSchema(self.schema_version));
        }
        if self.screens.len() > MAX_SAVED_SCREENS {
            return Err(ScreenError::TooManySavedScreens);
        }
        let builtins = builtin_screen_definitions()
            .into_iter()
            .map(|definition| definition.id)
            .collect::<BTreeSet<_>>();
        let mut ids = BTreeSet::new();
        for screen in &self.screens {
            screen.validate()?;
            if screen.built_in || builtins.contains(&screen.id) || !ids.insert(screen.id.clone()) {
                return Err(ScreenError::ProtectedOrDuplicateId(screen.id.clone()));
            }
        }
        Ok(())
    }
}

pub fn builtin_screen_definitions() -> Vec<ScreenDefinition> {
    vec![
        ScreenDefinition::new(
            "momentum",
            "QUALITY MOMENTUM",
            "core",
            vec![
                ScreenClause::new(
                    ScreenField::ChangePercent,
                    Comparison::GreaterThanOrEqual,
                    0.5,
                )
                .unwrap(),
                ScreenClause::new(
                    ScreenField::Volume,
                    Comparison::GreaterThanOrEqual,
                    10_000_000.0,
                )
                .unwrap(),
            ],
            ScreenField::ChangePercent,
            ScreenSortDirection::Descending,
            25,
            true,
        )
        .unwrap(),
        ScreenDefinition::new(
            "liquidity",
            "LIQUID LEADERS",
            "core",
            vec![ScreenClause::new(
                ScreenField::Volume,
                Comparison::GreaterThanOrEqual,
                20_000_000.0,
            )
            .unwrap()],
            ScreenField::Volume,
            ScreenSortDirection::Descending,
            25,
            true,
        )
        .unwrap(),
        ScreenDefinition::new(
            "tight-spread",
            "TIGHT SPREAD LIQUIDITY",
            "core",
            vec![
                ScreenClause::new(ScreenField::SpreadBps, Comparison::LessThanOrEqual, 5.0)
                    .unwrap(),
                ScreenClause::new(
                    ScreenField::Volume,
                    Comparison::GreaterThanOrEqual,
                    10_000_000.0,
                )
                .unwrap(),
            ],
            ScreenField::SpreadBps,
            ScreenSortDirection::Ascending,
            25,
            true,
        )
        .unwrap(),
    ]
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScreenError {
    InvalidId(&'static str),
    InvalidLabel,
    InvalidClauseCount,
    InvalidThreshold,
    InvalidLimit,
    MissingMetadata,
    UniverseTooLarge,
    InvalidUniverseMember,
    WrongUniverse { expected: String, actual: String },
    UnsupportedSchema(u16),
    TooManySavedScreens,
    ProtectedOrDuplicateId(String),
}

impl fmt::Display for ScreenError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidId(field) => write!(formatter, "{field} is invalid"),
            Self::InvalidLabel => formatter.write_str("screen label is invalid"),
            Self::InvalidClauseCount => {
                write!(formatter, "screen requires 1-{MAX_SCREEN_CLAUSES} clauses")
            }
            Self::InvalidThreshold => formatter.write_str("screen threshold must be finite"),
            Self::InvalidLimit => write!(formatter, "screen limit must be 1-{MAX_SCREEN_RESULTS}"),
            Self::MissingMetadata => formatter.write_str("universe metadata is incomplete"),
            Self::UniverseTooLarge => {
                write!(formatter, "universe exceeds {MAX_UNIVERSE_MEMBERS} members")
            }
            Self::InvalidUniverseMember => {
                formatter.write_str("universe members require unique identities and symbols")
            }
            Self::WrongUniverse { expected, actual } => write!(
                formatter,
                "screen expects universe {expected}, received {actual}"
            ),
            Self::UnsupportedSchema(version) => {
                write!(formatter, "unsupported screen catalog schema {version}")
            }
            Self::TooManySavedScreens => write!(
                formatter,
                "saved screen catalog exceeds {MAX_SAVED_SCREENS} definitions"
            ),
            Self::ProtectedOrDuplicateId(id) => {
                write!(formatter, "screen ID {id} is protected or duplicated")
            }
        }
    }
}

impl std::error::Error for ScreenError {}

fn validate_id(value: &str, field: &'static str) -> Result<(), ScreenError> {
    if value.is_empty()
        || value.len() > MAX_ID_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(ScreenError::InvalidId(field));
    }
    Ok(())
}

fn valid_member(member: &UniverseMember) -> bool {
    valid_bounded_text(&member.symbol, MAX_LABEL_BYTES, false)
        && valid_bounded_text(&member.description, MAX_MEMBER_TEXT_BYTES, true)
        && valid_bounded_text(&member.currency, 16, false)
        && valid_bounded_text(&member.quality, MAX_LABEL_BYTES, false)
        && valid_bounded_text(&member.provider, MAX_MEMBER_TEXT_BYTES, false)
        && [
            member.last,
            member.change_percent,
            member.volume,
            member.spread_bps,
            member.day_range_percent,
        ]
        .into_iter()
        .flatten()
        .all(f64::is_finite)
        && member.last.is_none_or(|value| value >= 0.0)
        && member.volume.is_none_or(|value| value >= 0.0)
        && member.spread_bps.is_none_or(|value| value >= 0.0)
        && member.day_range_percent.is_none_or(|value| value >= 0.0)
}

fn valid_bounded_text(value: &str, maximum: usize, allow_empty: bool) -> bool {
    value.len() <= maximum
        && (allow_empty || !value.trim().is_empty())
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

pub fn format_value(field: ScreenField, value: f64) -> String {
    match field {
        ScreenField::Last => format!("{value:.2}"),
        ScreenField::ChangePercent | ScreenField::DayRangePercent => format!("{value:.2}%"),
        ScreenField::Volume if value.abs() >= 1_000_000.0 => format!("{:.2}M", value / 1_000_000.0),
        ScreenField::Volume => format!("{value:.0}"),
        ScreenField::SpreadBps => format!("{value:.2}BP"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(id: &str, symbol: &str, change: Option<f64>, volume: Option<f64>) -> UniverseMember {
        UniverseMember {
            instrument_id: InstrumentId::new(id),
            symbol: symbol.to_owned(),
            description: symbol.to_owned(),
            currency: "USD".to_owned(),
            last: Some(100.0),
            change_percent: change,
            volume,
            spread_bps: Some(2.0),
            day_range_percent: None,
            quality: "REALTIME".to_owned(),
            provider: "fixture".to_owned(),
        }
    }

    fn try_universe(members: Vec<UniverseMember>) -> Result<UniverseSnapshot, ScreenError> {
        UniverseSnapshot::new(
            "core",
            "CORE",
            7,
            "2026-08-29T10:00:00Z",
            "FIXTURE",
            members,
        )
    }

    fn universe(members: Vec<UniverseMember>) -> UniverseSnapshot {
        try_universe(members).unwrap()
    }

    #[test]
    fn screen_is_point_in_time_stable_and_ties_break_by_identity() {
        let definition = builtin_screen_definitions().remove(0);
        let snapshot = universe(vec![
            member("us:z", "Z", Some(2.0), Some(30_000_000.0)),
            member("us:a", "A", Some(2.0), Some(30_000_000.0)),
            member("us:x", "X", Some(0.2), Some(50_000_000.0)),
        ]);

        let first = evaluate_screen(&definition, snapshot.clone()).unwrap();
        let second = evaluate_screen(&definition, snapshot).unwrap();

        assert_eq!(first, second);
        assert_eq!(
            first
                .rows
                .iter()
                .map(|row| row.member.symbol.as_str())
                .collect::<Vec<_>>(),
            ["A", "Z"]
        );
        assert_eq!(first.exclusions[0].symbol, "X");
        assert!(first
            .rows
            .iter()
            .all(|row| row.evidence.iter().all(|evidence| evidence.passed)));
    }

    #[test]
    fn missing_values_fail_closed_and_reduce_coverage() {
        let definition = builtin_screen_definitions().remove(0);
        let result = evaluate_screen(
            &definition,
            universe(vec![member("us:a", "A", Some(1.0), None)]),
        )
        .unwrap();

        assert!(result.rows.is_empty());
        assert_eq!(result.coverage_count, 0);
        assert!(result.exclusions[0]
            .evidence
            .iter()
            .any(|evidence| evidence.actual.is_none()));
    }

    #[test]
    fn result_limit_is_explicit_and_does_not_change_rank_order() {
        let mut definition = builtin_screen_definitions().remove(1);
        definition.limit = 2;
        let result = evaluate_screen(
            &definition,
            universe(vec![
                member("us:a", "A", Some(0.0), Some(30_000_000.0)),
                member("us:b", "B", Some(0.0), Some(50_000_000.0)),
                member("us:c", "C", Some(0.0), Some(40_000_000.0)),
            ]),
        )
        .unwrap();

        assert_eq!(
            result
                .rows
                .iter()
                .map(|row| row.member.symbol.as_str())
                .collect::<Vec<_>>(),
            ["B", "C"]
        );
        assert_eq!(result.truncated_count, 1);
    }

    #[test]
    fn persisted_catalog_rejects_builtin_shadowing_and_future_schema() {
        let builtin = builtin_screen_definitions().remove(0);
        assert!(ScreenCatalogState::new(1, vec![builtin]).is_err());
        let mut state = ScreenCatalogState::new(1, Vec::new()).unwrap();
        state.schema_version = 99;
        assert_eq!(state.validate(), Err(ScreenError::UnsupportedSchema(99)));
    }

    #[test]
    fn universe_rejects_unbounded_invalid_or_non_finite_member_fields() {
        let mut invalid = member("us:test:bad", "BAD", Some(1.0), Some(-1.0));
        assert_eq!(
            try_universe(vec![invalid.clone()]).unwrap_err(),
            ScreenError::InvalidUniverseMember
        );

        invalid.volume = Some(1.0);
        invalid.change_percent = Some(f64::NAN);
        assert_eq!(
            try_universe(vec![invalid.clone()]).unwrap_err(),
            ScreenError::InvalidUniverseMember
        );

        invalid.change_percent = Some(1.0);
        invalid.provider = " ".to_owned();
        assert_eq!(
            try_universe(vec![invalid]).unwrap_err(),
            ScreenError::InvalidUniverseMember
        );
    }
}
