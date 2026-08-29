use std::{cmp::Ordering, collections::BTreeSet, fmt};

use serde::{
    de::{self, Deserializer},
    Deserialize, Serialize, Serializer,
};

use crate::foundation::InstrumentId;

pub const MAX_UNIVERSE_MEMBERS: usize = 2_000;
pub const MAX_SCREEN_CLAUSES: usize = 8;
pub const MAX_SCREEN_EXPRESSION_DEPTH: usize = 8;
pub const MAX_SCREEN_RESULTS: usize = 200;
pub const MAX_SAVED_SCREENS: usize = 64;
pub const MAX_UNIVERSE_HISTORY: usize = 32;
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

    pub const fn dimension(self) -> ScreenDimension {
        match self {
            Self::Last => ScreenDimension::Price,
            Self::ChangePercent | Self::DayRangePercent => ScreenDimension::Percent,
            Self::Volume => ScreenDimension::Quantity,
            Self::SpreadBps => ScreenDimension::BasisPoints,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenDimension {
    Price,
    Percent,
    Quantity,
    BasisPoints,
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

/// Bounded boolean predicate tree owned by Screening. The tagged representation
/// is deliberately explicit so saved definitions remain inspectable and future
/// migrations never have to reverse-engineer a command string.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "items", rename_all = "snake_case")]
pub enum ScreenExpression {
    Predicate(ScreenClause),
    All(Vec<ScreenExpression>),
    Any(Vec<ScreenExpression>),
    Not(Box<ScreenExpression>),
}

impl ScreenExpression {
    pub fn validate(&self) -> Result<(), ScreenError> {
        let mut predicates = 0;
        self.validate_at_depth(1, &mut predicates)?;
        if !(1..=MAX_SCREEN_CLAUSES).contains(&predicates) {
            return Err(ScreenError::InvalidClauseCount);
        }
        Ok(())
    }

    fn validate_at_depth(&self, depth: usize, predicates: &mut usize) -> Result<(), ScreenError> {
        if depth > MAX_SCREEN_EXPRESSION_DEPTH {
            return Err(ScreenError::ExpressionTooDeep);
        }
        match self {
            Self::Predicate(clause) => {
                if !clause.value.is_finite() {
                    return Err(ScreenError::InvalidThreshold);
                }
                *predicates = predicates.saturating_add(1);
            }
            Self::All(items) | Self::Any(items) => {
                if items.len() < 2 {
                    return Err(ScreenError::InvalidExpression);
                }
                for item in items {
                    item.validate_at_depth(depth.saturating_add(1), predicates)?;
                }
            }
            Self::Not(item) => item.validate_at_depth(depth.saturating_add(1), predicates)?,
        }
        Ok(())
    }

    pub fn label(&self) -> String {
        self.label_with_precedence(0)
    }

    fn label_with_precedence(&self, parent_precedence: u8) -> String {
        let (precedence, rendered) = match self {
            Self::Predicate(clause) => (4, clause.label()),
            Self::Not(item) => (3, format!("NOT {}", item.label_with_precedence(3))),
            Self::All(items) => (
                2,
                items
                    .iter()
                    .map(|item| item.label_with_precedence(2))
                    .collect::<Vec<_>>()
                    .join(" AND "),
            ),
            Self::Any(items) => (
                1,
                items
                    .iter()
                    .map(|item| item.label_with_precedence(1))
                    .collect::<Vec<_>>()
                    .join(" OR "),
            ),
        };
        if precedence < parent_precedence {
            format!("({rendered})")
        } else {
            rendered
        }
    }

    fn clauses(&self, target: &mut Vec<ScreenClause>) {
        match self {
            Self::Predicate(clause) => target.push(clause.clone()),
            Self::All(items) | Self::Any(items) => {
                for item in items {
                    item.clauses(target);
                }
            }
            Self::Not(item) => item.clauses(target),
        }
    }

    fn evaluate(
        &self,
        member: &UniverseMember,
        evidence: &mut Vec<ClauseEvidence>,
    ) -> Option<bool> {
        match self {
            Self::Predicate(clause) => {
                let actual = clause.field.value(member);
                let passed =
                    actual.is_some_and(|actual| clause.comparison.matches(actual, clause.value));
                evidence.push(ClauseEvidence {
                    clause: clause.clone(),
                    actual,
                    passed,
                });
                actual.map(|_| passed)
            }
            // Evaluate every branch rather than short-circuiting so missing-data
            // coverage and per-predicate evidence remain complete.
            Self::All(items) => {
                let results = items
                    .iter()
                    .map(|item| item.evaluate(member, evidence))
                    .collect::<Vec<_>>();
                if results.contains(&Some(false)) {
                    Some(false)
                } else if results.iter().all(|result| *result == Some(true)) {
                    Some(true)
                } else {
                    None
                }
            }
            Self::Any(items) => {
                let results = items
                    .iter()
                    .map(|item| item.evaluate(member, evidence))
                    .collect::<Vec<_>>();
                if results.contains(&Some(true)) {
                    Some(true)
                } else if results.iter().all(|result| *result == Some(false)) {
                    Some(false)
                } else {
                    None
                }
            }
            Self::Not(item) => item.evaluate(member, evidence).map(|passed| !passed),
        }
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
    /// New definitions store their exact boolean tree. `None` is the schema-v1
    /// migration path and means the legacy `clauses` vector joined by `AND`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expression_tree: Option<ScreenExpression>,
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
            expression_tree: None,
            sort_field,
            sort_direction,
            limit,
            built_in,
        };
        definition.validate()?;
        Ok(definition)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_expression(
        id: impl Into<String>,
        label: impl Into<String>,
        universe_id: impl Into<String>,
        expression_tree: ScreenExpression,
        sort_field: ScreenField,
        sort_direction: ScreenSortDirection,
        limit: usize,
        built_in: bool,
    ) -> Result<Self, ScreenError> {
        expression_tree.validate()?;
        let mut clauses = Vec::new();
        expression_tree.clauses(&mut clauses);
        let definition = Self {
            id: id.into(),
            label: label.into(),
            universe_id: universe_id.into(),
            clauses,
            expression_tree: Some(expression_tree),
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
        if let Some(expression) = &self.expression_tree {
            expression.validate()?;
            let mut expression_clauses = Vec::new();
            expression.clauses(&mut expression_clauses);
            if expression_clauses != self.clauses {
                return Err(ScreenError::ExpressionClauseMismatch);
            }
        }
        if !(1..=MAX_SCREEN_RESULTS).contains(&self.limit) {
            return Err(ScreenError::InvalidLimit);
        }
        Ok(())
    }

    pub fn expression(&self) -> String {
        self.expression_tree.as_ref().map_or_else(
            || {
                self.clauses
                    .iter()
                    .map(ScreenClause::label)
                    .collect::<Vec<_>>()
                    .join(" AND ")
            },
            ScreenExpression::label,
        )
    }

    fn evaluate_member(&self, member: &UniverseMember, evidence: &mut Vec<ClauseEvidence>) -> bool {
        if let Some(expression) = &self.expression_tree {
            expression.evaluate(member, evidence).unwrap_or(false)
        } else {
            self.clauses
                .iter()
                .map(|clause| {
                    ScreenExpression::Predicate(clause.clone())
                        .evaluate(member, evidence)
                        .unwrap_or(false)
                })
                .fold(true, |result, passed| result & passed)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UniverseMember {
    #[serde(
        serialize_with = "serialize_instrument_id",
        deserialize_with = "deserialize_instrument_id"
    )]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn validate(&self) -> Result<(), ScreenError> {
        validate_id(&self.id, "universe ID")?;
        if self.label.trim().is_empty() || self.label.len() > MAX_LABEL_BYTES {
            return Err(ScreenError::InvalidLabel);
        }
        if !valid_bounded_text(&self.as_of, MAX_METADATA_BYTES, false)
            || !valid_bounded_text(&self.source, MAX_METADATA_BYTES, false)
        {
            return Err(ScreenError::MissingMetadata);
        }
        if self.members.len() > MAX_UNIVERSE_MEMBERS {
            return Err(ScreenError::UniverseTooLarge);
        }
        let mut identities = BTreeSet::new();
        if self.members.iter().any(|member| {
            !valid_member(member) || !identities.insert(member.instrument_id.as_str().to_owned())
        }) {
            return Err(ScreenError::InvalidUniverseMember);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UniverseHistoryEntry {
    pub universe_id: String,
    pub universe_label: String,
    pub version: u64,
    pub content_digest: u64,
    pub as_of: String,
    pub source: String,
    pub member_count: usize,
}

impl UniverseHistoryEntry {
    pub fn from_snapshot(snapshot: &UniverseSnapshot) -> Self {
        Self {
            universe_id: snapshot.id.clone(),
            universe_label: snapshot.label.clone(),
            version: snapshot.version,
            content_digest: universe_content_digest(snapshot),
            as_of: snapshot.as_of.clone(),
            source: snapshot.source.clone(),
            member_count: snapshot.members.len(),
        }
    }

    pub fn validate(&self) -> Result<(), ScreenError> {
        validate_id(&self.universe_id, "universe ID")?;
        if self.universe_label.trim().is_empty() || self.universe_label.len() > MAX_LABEL_BYTES {
            return Err(ScreenError::InvalidLabel);
        }
        if self.version == 0
            || self.content_digest == 0
            || !valid_bounded_text(&self.as_of, MAX_METADATA_BYTES, false)
            || !valid_bounded_text(&self.source, MAX_METADATA_BYTES, false)
            || self.member_count > MAX_UNIVERSE_MEMBERS
        {
            return Err(ScreenError::InvalidHistoryEntry);
        }
        Ok(())
    }
}

/// Stable digest over every decision-relevant universe field. History uses it
/// independently from the caller-supplied version so valid-looking JSON cannot
/// mutate after publication without being detected during replay.
pub fn universe_content_digest(snapshot: &UniverseSnapshot) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    digest_text(&mut hash, &snapshot.id);
    digest_text(&mut hash, &snapshot.as_of);
    digest_text(&mut hash, &snapshot.source);
    digest_u64(&mut hash, snapshot.members.len() as u64);
    for member in &snapshot.members {
        digest_text(&mut hash, member.instrument_id.as_str());
        digest_text(&mut hash, &member.symbol);
        digest_text(&mut hash, &member.description);
        digest_text(&mut hash, &member.currency);
        digest_optional_f64(&mut hash, member.last);
        digest_optional_f64(&mut hash, member.change_percent);
        digest_optional_f64(&mut hash, member.volume);
        digest_optional_f64(&mut hash, member.spread_bps);
        digest_optional_f64(&mut hash, member.day_range_percent);
        digest_text(&mut hash, &member.quality);
        digest_text(&mut hash, &member.provider);
    }
    hash
}

fn digest_text(hash: &mut u64, value: &str) {
    digest_u64(hash, value.len() as u64);
    for byte in value.bytes() {
        digest_byte(hash, byte);
    }
}

fn digest_optional_f64(hash: &mut u64, value: Option<f64>) {
    match value {
        Some(value) => {
            digest_byte(hash, 1);
            digest_u64(hash, value.to_bits());
        }
        None => digest_byte(hash, 0),
    }
}

fn digest_u64(hash: &mut u64, value: u64) {
    for byte in value.to_le_bytes() {
        digest_byte(hash, byte);
    }
}

fn digest_byte(hash: &mut u64, byte: u8) {
    *hash ^= u64::from(byte);
    *hash = (*hash).wrapping_mul(0x100000001b3);
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UniverseHistoryManifest {
    pub schema_version: u16,
    pub revision: u64,
    pub entries: Vec<UniverseHistoryEntry>,
}

impl UniverseHistoryManifest {
    pub const SCHEMA_VERSION: u16 = 1;

    pub fn empty() -> Self {
        Self {
            schema_version: Self::SCHEMA_VERSION,
            revision: 0,
            entries: Vec::new(),
        }
    }

    pub fn validate(&self) -> Result<(), ScreenError> {
        if self.schema_version != Self::SCHEMA_VERSION {
            return Err(ScreenError::UnsupportedHistorySchema(self.schema_version));
        }
        if self.entries.len() > MAX_UNIVERSE_HISTORY {
            return Err(ScreenError::TooManyHistoryEntries);
        }
        let mut versions = BTreeSet::new();
        for entry in &self.entries {
            entry.validate()?;
            if !versions.insert(entry.version) {
                return Err(ScreenError::DuplicateHistoryVersion(entry.version));
            }
        }
        Ok(())
    }

    pub fn entries_for(&self, universe_id: &str) -> Vec<&UniverseHistoryEntry> {
        self.entries
            .iter()
            .rev()
            .filter(|entry| entry.universe_id.eq_ignore_ascii_case(universe_id))
            .collect()
    }

    pub fn record(
        &self,
        snapshot: &UniverseSnapshot,
    ) -> Result<(Self, Option<UniverseHistoryEntry>), ScreenError> {
        self.validate()?;
        snapshot.validate()?;
        let entry = UniverseHistoryEntry::from_snapshot(snapshot);
        entry.validate()?;
        if let Some(existing) = self
            .entries
            .iter()
            .find(|existing| existing.version == entry.version)
        {
            if existing == &entry {
                return Ok((self.clone(), None));
            }
            return Err(ScreenError::HistoryVersionCollision(entry.version));
        }

        let mut entries = self.entries.clone();
        entries.push(entry);
        let evicted = (entries.len() > MAX_UNIVERSE_HISTORY).then(|| entries.remove(0));
        let next = Self {
            schema_version: Self::SCHEMA_VERSION,
            revision: self.revision.saturating_add(1),
            entries,
        };
        next.validate()?;
        Ok((next, evicted))
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
        let mut evidence = Vec::with_capacity(definition.clauses.len());
        let expression_passed = definition.evaluate_member(member, &mut evidence);
        let sort_value = definition.sort_field.value(member);
        let complete = evidence.iter().all(|item| item.actual.is_some()) && sort_value.is_some();
        coverage_count += usize::from(complete);
        if let Some(sort_value) = sort_value.filter(|_| complete && expression_passed) {
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
    InvalidExpression,
    ExpressionTooDeep,
    ExpressionClauseMismatch,
    InvalidLimit,
    MissingMetadata,
    UniverseTooLarge,
    InvalidUniverseMember,
    WrongUniverse { expected: String, actual: String },
    UnsupportedSchema(u16),
    TooManySavedScreens,
    ProtectedOrDuplicateId(String),
    InvalidHistoryEntry,
    UnsupportedHistorySchema(u16),
    TooManyHistoryEntries,
    DuplicateHistoryVersion(u64),
    HistoryVersionCollision(u64),
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
            Self::InvalidExpression => {
                formatter.write_str("screen boolean groups require at least two child expressions")
            }
            Self::ExpressionTooDeep => write!(
                formatter,
                "screen expression exceeds {MAX_SCREEN_EXPRESSION_DEPTH} levels",
            ),
            Self::ExpressionClauseMismatch => {
                formatter.write_str("screen expression predicates do not match its clause catalog")
            }
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
            Self::InvalidHistoryEntry => formatter.write_str("universe history entry is invalid"),
            Self::UnsupportedHistorySchema(version) => {
                write!(formatter, "unsupported universe history schema {version}")
            }
            Self::TooManyHistoryEntries => write!(
                formatter,
                "universe history exceeds {MAX_UNIVERSE_HISTORY} snapshots"
            ),
            Self::DuplicateHistoryVersion(version) => {
                write!(
                    formatter,
                    "universe history duplicates version {version:016X}"
                )
            }
            Self::HistoryVersionCollision(version) => {
                write!(
                    formatter,
                    "universe history version collision {version:016X}"
                )
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

fn serialize_instrument_id<S>(id: &InstrumentId, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(id.as_str())
}

fn deserialize_instrument_id<'de, D>(deserializer: D) -> Result<InstrumentId, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    if !valid_bounded_text(&value, 128, false) {
        return Err(de::Error::custom("instrument identity is invalid"));
    }
    Ok(InstrumentId::new(value))
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
    fn nested_boolean_expressions_preserve_precedence_and_fail_closed() {
        let expression = ScreenExpression::All(vec![
            ScreenExpression::Any(vec![
                ScreenExpression::Predicate(
                    ScreenClause::new(
                        ScreenField::ChangePercent,
                        Comparison::GreaterThanOrEqual,
                        1.0,
                    )
                    .unwrap(),
                ),
                ScreenExpression::Predicate(
                    ScreenClause::new(
                        ScreenField::Volume,
                        Comparison::GreaterThanOrEqual,
                        40_000_000.0,
                    )
                    .unwrap(),
                ),
            ]),
            ScreenExpression::Not(Box::new(ScreenExpression::Predicate(
                ScreenClause::new(ScreenField::SpreadBps, Comparison::GreaterThan, 5.0).unwrap(),
            ))),
        ]);
        let definition = ScreenDefinition::new_expression(
            "nested",
            "NESTED",
            "core",
            expression,
            ScreenField::ChangePercent,
            ScreenSortDirection::Descending,
            20,
            false,
        )
        .unwrap();
        assert_eq!(
            definition.expression(),
            "(% CHG >= 1.00% OR VOLUME >= 40.00M) AND NOT SPREAD BP > 5.00BP"
        );

        let mut missing_spread = member("us:m", "M", Some(2.0), Some(1_000_000.0));
        missing_spread.spread_bps = None;
        let result = evaluate_screen(
            &definition,
            universe(vec![
                member("us:a", "A", Some(2.0), Some(1_000_000.0)),
                member("us:b", "B", Some(-1.0), Some(50_000_000.0)),
                missing_spread,
            ]),
        )
        .unwrap();
        assert_eq!(
            result
                .rows
                .iter()
                .map(|row| row.member.symbol.as_str())
                .collect::<Vec<_>>(),
            ["A", "B"]
        );
        assert_eq!(result.coverage_count, 2);
        assert!(result.exclusions[0]
            .evidence
            .iter()
            .any(|evidence| evidence.actual.is_none()));
    }

    #[test]
    fn a_true_or_branch_cannot_mask_a_missing_predicate() {
        let expression = ScreenExpression::Any(vec![
            ScreenExpression::Predicate(
                ScreenClause::new(ScreenField::ChangePercent, Comparison::GreaterThan, 0.0)
                    .unwrap(),
            ),
            ScreenExpression::Predicate(
                ScreenClause::new(ScreenField::Volume, Comparison::GreaterThan, 0.0).unwrap(),
            ),
        ]);
        let definition = ScreenDefinition::new_expression(
            "strict-null",
            "STRICT NULL",
            "core",
            expression,
            ScreenField::ChangePercent,
            ScreenSortDirection::Descending,
            20,
            false,
        )
        .unwrap();
        let result = evaluate_screen(
            &definition,
            universe(vec![member("us:a", "A", Some(2.0), None)]),
        )
        .unwrap();
        assert!(result.rows.is_empty());
        assert_eq!(result.coverage_count, 0);
    }

    #[test]
    fn expression_tree_round_trips_while_legacy_and_definitions_still_load() {
        let clause = ScreenClause::new(ScreenField::Volume, Comparison::GreaterThan, 10.0).unwrap();
        let definition = ScreenDefinition::new_expression(
            "portable",
            "PORTABLE",
            "core",
            ScreenExpression::Not(Box::new(ScreenExpression::Predicate(clause))),
            ScreenField::Volume,
            ScreenSortDirection::Descending,
            10,
            false,
        )
        .unwrap();
        let encoded = serde_json::to_string(&definition).unwrap();
        let decoded: ScreenDefinition = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, definition);

        let legacy = r#"{
            "id":"legacy","label":"LEGACY","universe_id":"core",
            "clauses":[{"field":"volume","comparison":"greater_than","value":10.0}],
            "sort_field":"volume","sort_direction":"descending","limit":10,"built_in":false
        }"#;
        let decoded: ScreenDefinition = serde_json::from_str(legacy).unwrap();
        decoded.validate().unwrap();
        assert!(decoded.expression_tree.is_none());
        assert_eq!(decoded.expression(), "VOLUME > 10");
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

    #[test]
    fn history_is_bounded_idempotent_and_rejects_version_collisions() {
        let mut manifest = UniverseHistoryManifest::empty();
        for version in 1..=MAX_UNIVERSE_HISTORY as u64 + 1 {
            let mut snapshot = universe(vec![member(
                &format!("us:test:{version}"),
                &format!("T{version}"),
                Some(version as f64),
                Some(30_000_000.0),
            )]);
            snapshot.version = version;
            snapshot.as_of = format!("2026-08-29T00:00:{version:02}Z");
            let (next, evicted) = manifest.record(&snapshot).unwrap();
            if version <= MAX_UNIVERSE_HISTORY as u64 {
                assert!(evicted.is_none());
            } else {
                assert_eq!(evicted.unwrap().version, 1);
            }
            manifest = next;
        }

        assert_eq!(manifest.entries.len(), MAX_UNIVERSE_HISTORY);
        assert_eq!(manifest.entries.first().unwrap().version, 2);
        assert_eq!(manifest.entries.last().unwrap().version, 33);
        assert_eq!(manifest.revision, 33);

        let mut same = universe(vec![member(
            "us:test:33",
            "T33",
            Some(33.0),
            Some(30_000_000.0),
        )]);
        same.version = 33;
        same.as_of = "2026-08-29T00:00:33Z".to_owned();
        let (unchanged, evicted) = manifest.record(&same).unwrap();
        assert_eq!(unchanged, manifest);
        assert!(evicted.is_none());

        same.source = "DIFFERENT SOURCE".to_owned();
        assert_eq!(
            manifest.record(&same).unwrap_err(),
            ScreenError::HistoryVersionCollision(33)
        );
    }

    #[test]
    fn snapshot_json_round_trip_preserves_canonical_identity_and_revalidates() {
        let expected = universe(vec![member(
            "us:xnas:aapl",
            "AAPL",
            Some(1.5),
            Some(42_000_000.0),
        )]);
        let encoded = serde_json::to_value(&expected).unwrap();
        let restored: UniverseSnapshot = serde_json::from_value(encoded.clone()).unwrap();
        restored.validate().unwrap();
        assert_eq!(restored, expected);

        let mut invalid = encoded;
        invalid["members"][0]["instrument_id"] = serde_json::Value::String(" ".to_owned());
        assert!(serde_json::from_value::<UniverseSnapshot>(invalid).is_err());
    }
}
