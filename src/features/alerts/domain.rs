use std::{
    collections::{BTreeSet, VecDeque},
    fmt,
};

use crate::foundation::InstrumentId;

pub const MAX_ALERT_RULES: usize = 256;
pub const MAX_ALERT_AUDIT_ENTRIES: usize = 256;
pub const MAX_ALERT_EVALUATION_IDS: usize = 1_024;
const MAX_ALERT_STATE_TEXT_BYTES: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AlertRuleId(String);

impl AlertRuleId {
    pub fn new(value: impl Into<String>) -> Self {
        let value = value.into();
        assert!(!value.trim().is_empty(), "alert rule ID cannot be empty");
        Self(value)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AlertRuleId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstrumentRef {
    pub canonical_id: InstrumentId,
    pub symbol: String,
}

impl InstrumentRef {
    pub fn new(canonical_id: impl Into<String>, symbol: impl Into<String>) -> Self {
        let canonical_id = canonical_id.into();
        let symbol = symbol.into();
        assert!(
            !canonical_id.trim().is_empty(),
            "instrument ID cannot be empty"
        );
        assert!(
            !symbol.trim().is_empty(),
            "instrument symbol cannot be empty"
        );
        Self {
            canonical_id: InstrumentId::new(canonical_id),
            symbol,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AlertCondition {
    PriceAbove { threshold: f64 },
    PriceBelow { threshold: f64 },
    PercentMoveAbove { threshold: f64 },
    PercentMoveBelow { threshold: f64 },
}

impl AlertCondition {
    pub fn price_above(threshold: f64) -> Self {
        assert_valid_price_threshold(threshold);
        Self::PriceAbove { threshold }
    }

    pub fn price_below(threshold: f64) -> Self {
        assert_valid_price_threshold(threshold);
        Self::PriceBelow { threshold }
    }

    pub fn percent_move_above(threshold: f64) -> Self {
        assert_valid_threshold(threshold);
        Self::PercentMoveAbove { threshold }
    }

    pub fn percent_move_below(threshold: f64) -> Self {
        assert_valid_threshold(threshold);
        Self::PercentMoveBelow { threshold }
    }

    pub fn matches(self, observation: &AlertObservation) -> bool {
        match self {
            Self::PriceAbove { threshold } => observation.price > threshold,
            Self::PriceBelow { threshold } => observation.price < threshold,
            Self::PercentMoveAbove { threshold } => observation.percent_move > threshold,
            Self::PercentMoveBelow { threshold } => observation.percent_move < threshold,
        }
    }

    pub fn label(self) -> String {
        match self {
            Self::PriceAbove { threshold } => format!("PRICE > {threshold:.2}"),
            Self::PriceBelow { threshold } => format!("PRICE < {threshold:.2}"),
            Self::PercentMoveAbove { threshold } => format!("MOVE > {threshold:.2}%"),
            Self::PercentMoveBelow { threshold } => format!("MOVE < {threshold:.2}%"),
        }
    }
}

fn assert_valid_threshold(threshold: f64) {
    assert!(threshold.is_finite(), "alert threshold must be finite");
}

fn assert_valid_price_threshold(threshold: f64) {
    assert_valid_threshold(threshold);
    assert!(threshold >= 0.0, "price alert threshold cannot be negative");
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DebouncePolicy {
    confirmations: u8,
}

impl DebouncePolicy {
    pub fn consecutive(confirmations: u8) -> Self {
        assert!(confirmations > 0, "debounce confirmations must be positive");
        Self { confirmations }
    }

    pub const fn confirmations(self) -> u8 {
        self.confirmations
    }
}

impl Default for DebouncePolicy {
    fn default() -> Self {
        Self::consecutive(1)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertDelivery {
    SimulatedLocal,
}

impl AlertDelivery {
    pub const fn label(self) -> &'static str {
        match self {
            Self::SimulatedLocal => "SIMULATED · LOCAL ONLY",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AlertObservation {
    pub evaluation_id: String,
    pub instrument_id: InstrumentId,
    pub price: f64,
    pub percent_move: f64,
    pub observed_at: String,
}

impl AlertObservation {
    pub fn new(
        evaluation_id: impl Into<String>,
        instrument_id: impl Into<String>,
        price: f64,
        percent_move: f64,
        observed_at: impl Into<String>,
    ) -> Self {
        let evaluation_id = evaluation_id.into();
        let instrument_id = instrument_id.into();
        let observed_at = observed_at.into();
        assert!(
            !evaluation_id.trim().is_empty(),
            "evaluation ID cannot be empty"
        );
        assert!(
            !instrument_id.trim().is_empty(),
            "instrument ID cannot be empty"
        );
        assert!(price.is_finite(), "observed price must be finite");
        assert!(
            percent_move.is_finite(),
            "observed percent move must be finite"
        );
        assert!(
            !observed_at.trim().is_empty(),
            "observation time cannot be empty"
        );
        Self {
            evaluation_id,
            instrument_id: InstrumentId::new(instrument_id),
            price,
            percent_move,
            observed_at,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertLifecycle {
    Enabled,
    Disabled,
}

impl AlertLifecycle {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Enabled => "ENABLED",
            Self::Disabled => "DISABLED",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AlertStatus {
    Armed,
    Pending {
        matched: u8,
        required: u8,
    },
    Triggered {
        occurrence_id: String,
        triggered_at: String,
    },
    Acknowledged {
        occurrence_id: String,
        acknowledged_at: String,
    },
}

impl AlertStatus {
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Armed => "ARMED",
            Self::Pending { .. } => "DEBOUNCE",
            Self::Triggered { .. } => "TRIGGERED",
            Self::Acknowledged { .. } => "ACKNOWLEDGED",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertAuditKind {
    Enabled,
    Disabled,
    Triggered,
    Acknowledged,
    Rearmed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlertAuditEntry {
    pub kind: AlertAuditKind,
    pub at: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AlertTrigger {
    pub occurrence_id: String,
    pub rule_id: AlertRuleId,
    pub evaluation_id: String,
    pub observed_at: String,
    pub delivery: AlertDelivery,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AlertEvaluation {
    Duplicate,
    NotApplicable,
    IgnoredDisabled,
    Armed,
    Pending { matched: u8, required: u8 },
    Triggered(AlertTrigger),
    Latched,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AlertRule {
    pub id: AlertRuleId,
    pub instrument: InstrumentRef,
    pub condition: AlertCondition,
    pub lifecycle: AlertLifecycle,
    pub status: AlertStatus,
    pub debounce: DebouncePolicy,
    pub delivery: AlertDelivery,
    pub last_observation: Option<AlertObservation>,
    pub audit: Vec<AlertAuditEntry>,
    processed_evaluation_ids: BTreeSet<String>,
    processed_evaluation_order: VecDeque<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AlertRuleRuntimeState {
    pub lifecycle: AlertLifecycle,
    pub status: AlertStatus,
    pub last_observation: Option<AlertObservation>,
    pub audit: Vec<AlertAuditEntry>,
    pub processed_evaluation_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlertStateValidationError(String);

impl fmt::Display for AlertStateValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for AlertStateValidationError {}

impl AlertRule {
    pub fn new(
        id: AlertRuleId,
        instrument: InstrumentRef,
        condition: AlertCondition,
        debounce: DebouncePolicy,
    ) -> Self {
        Self {
            id,
            instrument,
            condition,
            lifecycle: AlertLifecycle::Enabled,
            status: AlertStatus::Armed,
            debounce,
            delivery: AlertDelivery::SimulatedLocal,
            last_observation: None,
            audit: Vec::new(),
            processed_evaluation_ids: BTreeSet::new(),
            processed_evaluation_order: VecDeque::new(),
        }
    }

    pub fn restore(
        id: AlertRuleId,
        instrument: InstrumentRef,
        condition: AlertCondition,
        debounce: DebouncePolicy,
        state: AlertRuleRuntimeState,
    ) -> Result<Self, AlertStateValidationError> {
        validate_runtime_state(&instrument, debounce, &state)?;
        let mut rule = Self::new(id, instrument, condition, debounce);
        rule.lifecycle = state.lifecycle;
        rule.status = state.status;
        rule.last_observation = state.last_observation;
        rule.audit = state.audit;
        for evaluation_id in state.processed_evaluation_ids {
            if !rule.record_evaluation_id(evaluation_id) {
                return Err(AlertStateValidationError(
                    "processed evaluation IDs must be unique".to_owned(),
                ));
            }
        }
        Ok(rule)
    }

    pub fn runtime_state(&self) -> AlertRuleRuntimeState {
        AlertRuleRuntimeState {
            lifecycle: self.lifecycle,
            status: self.status.clone(),
            last_observation: self.last_observation.clone(),
            audit: self.audit.clone(),
            processed_evaluation_ids: self.processed_evaluation_order.iter().cloned().collect(),
        }
    }

    /// Applies one uniquely identified observation.
    ///
    /// Matching observations must be consecutive to satisfy the debounce
    /// policy. A repeated evaluation ID is ignored, and a triggered or
    /// acknowledged rule stays latched until a non-matching observation
    /// rearms it. Together those rules make replay idempotent and deterministic.
    pub fn evaluate(&mut self, observation: &AlertObservation) -> AlertEvaluation {
        if observation.instrument_id != self.instrument.canonical_id {
            return AlertEvaluation::NotApplicable;
        }
        if !self.record_evaluation_id(observation.evaluation_id.clone()) {
            return AlertEvaluation::Duplicate;
        }

        self.last_observation = Some(observation.clone());
        if self.lifecycle == AlertLifecycle::Disabled {
            return AlertEvaluation::IgnoredDisabled;
        }

        if !self.condition.matches(observation) {
            let was_latched = !matches!(&self.status, AlertStatus::Armed);
            self.status = AlertStatus::Armed;
            if was_latched {
                self.push_audit(AlertAuditEntry {
                    kind: AlertAuditKind::Rearmed,
                    at: observation.observed_at.clone(),
                    detail: format!("rearmed by {}", observation.evaluation_id),
                });
            }
            return AlertEvaluation::Armed;
        }

        let matched = match &self.status {
            AlertStatus::Triggered { .. } | AlertStatus::Acknowledged { .. } => {
                return AlertEvaluation::Latched;
            }
            AlertStatus::Armed => 1,
            AlertStatus::Pending { matched, .. } => matched.saturating_add(1),
        };
        self.advance_debounce(observation, matched)
    }

    pub fn toggle(&mut self, at: impl Into<String>) {
        let at = at.into();
        match self.lifecycle {
            AlertLifecycle::Enabled => {
                self.lifecycle = AlertLifecycle::Disabled;
                self.status = AlertStatus::Armed;
                self.push_audit(AlertAuditEntry {
                    kind: AlertAuditKind::Disabled,
                    at,
                    detail: "rule disabled locally".to_owned(),
                });
            }
            AlertLifecycle::Disabled => {
                self.lifecycle = AlertLifecycle::Enabled;
                self.status = AlertStatus::Armed;
                self.push_audit(AlertAuditEntry {
                    kind: AlertAuditKind::Enabled,
                    at,
                    detail: "rule enabled locally".to_owned(),
                });
            }
        }
    }

    pub fn acknowledge(&mut self, at: impl Into<String>) -> bool {
        let AlertStatus::Triggered { occurrence_id, .. } = &self.status else {
            return false;
        };
        let at = at.into();
        let occurrence_id = occurrence_id.clone();
        self.status = AlertStatus::Acknowledged {
            occurrence_id: occurrence_id.clone(),
            acknowledged_at: at.clone(),
        };
        self.push_audit(AlertAuditEntry {
            kind: AlertAuditKind::Acknowledged,
            at,
            detail: format!("acknowledged {occurrence_id}"),
        });
        true
    }

    fn advance_debounce(&mut self, observation: &AlertObservation, matched: u8) -> AlertEvaluation {
        let required = self.debounce.confirmations();
        if matched < required {
            self.status = AlertStatus::Pending { matched, required };
            return AlertEvaluation::Pending { matched, required };
        }

        let occurrence_id = format!("{}@{}", self.id, observation.evaluation_id);
        self.status = AlertStatus::Triggered {
            occurrence_id: occurrence_id.clone(),
            triggered_at: observation.observed_at.clone(),
        };
        self.push_audit(AlertAuditEntry {
            kind: AlertAuditKind::Triggered,
            at: observation.observed_at.clone(),
            detail: format!("triggered by {}", observation.evaluation_id),
        });
        AlertEvaluation::Triggered(AlertTrigger {
            occurrence_id,
            rule_id: self.id.clone(),
            evaluation_id: observation.evaluation_id.clone(),
            observed_at: observation.observed_at.clone(),
            delivery: self.delivery,
        })
    }

    fn record_evaluation_id(&mut self, evaluation_id: String) -> bool {
        if !self.processed_evaluation_ids.insert(evaluation_id.clone()) {
            return false;
        }
        self.processed_evaluation_order.push_back(evaluation_id);
        if self.processed_evaluation_order.len() > MAX_ALERT_EVALUATION_IDS {
            if let Some(expired) = self.processed_evaluation_order.pop_front() {
                self.processed_evaluation_ids.remove(&expired);
            }
        }
        true
    }

    fn push_audit(&mut self, entry: AlertAuditEntry) {
        self.audit.push(entry);
        if self.audit.len() > MAX_ALERT_AUDIT_ENTRIES {
            let expired = self.audit.len() - MAX_ALERT_AUDIT_ENTRIES;
            self.audit.drain(..expired);
        }
    }
}

fn validate_runtime_state(
    instrument: &InstrumentRef,
    debounce: DebouncePolicy,
    state: &AlertRuleRuntimeState,
) -> Result<(), AlertStateValidationError> {
    if state.audit.len() > MAX_ALERT_AUDIT_ENTRIES {
        return Err(AlertStateValidationError(
            "alert audit exceeds its limit".to_owned(),
        ));
    }
    if state.processed_evaluation_ids.len() > MAX_ALERT_EVALUATION_IDS {
        return Err(AlertStateValidationError(
            "processed evaluation IDs exceed their limit".to_owned(),
        ));
    }
    match &state.status {
        AlertStatus::Pending { matched, required }
            if *matched == 0 || *matched >= *required || *required != debounce.confirmations() =>
        {
            return Err(AlertStateValidationError(
                "persisted debounce state is inconsistent".to_owned(),
            ));
        }
        AlertStatus::Triggered {
            occurrence_id,
            triggered_at,
        } => {
            validate_state_text(occurrence_id, "trigger occurrence ID")?;
            validate_state_text(triggered_at, "trigger time")?;
        }
        AlertStatus::Acknowledged {
            occurrence_id,
            acknowledged_at,
        } => {
            validate_state_text(occurrence_id, "acknowledged occurrence ID")?;
            validate_state_text(acknowledged_at, "acknowledgement time")?;
        }
        _ => {}
    }
    if let Some(observation) = &state.last_observation {
        if observation.instrument_id != instrument.canonical_id {
            return Err(AlertStateValidationError(
                "last observation belongs to another instrument".to_owned(),
            ));
        }
        validate_state_text(&observation.evaluation_id, "evaluation ID")?;
        validate_state_text(&observation.observed_at, "observation time")?;
        if !observation.price.is_finite() || !observation.percent_move.is_finite() {
            return Err(AlertStateValidationError(
                "last observation contains a non-finite value".to_owned(),
            ));
        }
        if !state
            .processed_evaluation_ids
            .contains(&observation.evaluation_id)
        {
            return Err(AlertStateValidationError(
                "last observation is missing from processed evaluation IDs".to_owned(),
            ));
        }
    }
    for entry in &state.audit {
        validate_state_text(&entry.at, "audit time")?;
        validate_state_text(&entry.detail, "audit detail")?;
    }
    for evaluation_id in &state.processed_evaluation_ids {
        validate_state_text(evaluation_id, "evaluation ID")?;
    }
    Ok(())
}

fn validate_state_text(value: &str, field: &str) -> Result<(), AlertStateValidationError> {
    if value.trim().is_empty() || value.len() > MAX_ALERT_STATE_TEXT_BYTES {
        return Err(AlertStateValidationError(format!(
            "{field} is empty or exceeds {MAX_ALERT_STATE_TEXT_BYTES} bytes"
        )));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
pub struct AlertSnapshot {
    pub sequence: u64,
    pub as_of: String,
    /// Rules are populated by the initial snapshot. Replay snapshots may leave
    /// this empty so local acknowledgement and enablement state is preserved.
    pub rules: Vec<AlertRule>,
    pub observations: Vec<AlertObservation>,
    pub source: String,
}

impl AlertSnapshot {
    pub fn new(
        sequence: u64,
        as_of: impl Into<String>,
        rules: Vec<AlertRule>,
        observations: Vec<AlertObservation>,
        source: impl Into<String>,
    ) -> Self {
        Self {
            sequence,
            as_of: as_of.into(),
            rules,
            observations,
            source: source.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observation(id: &str, price: f64) -> AlertObservation {
        AlertObservation::new(
            id,
            "us:xnas:aapl",
            price,
            0.5,
            format!("2026-08-25T20:00:0{id}Z"),
        )
    }

    fn rule(confirmations: u8) -> AlertRule {
        AlertRule::new(
            AlertRuleId::new("price:aapl:206"),
            InstrumentRef::new("us:xnas:aapl", "AAPL"),
            AlertCondition::price_above(206.0),
            DebouncePolicy::consecutive(confirmations),
        )
    }

    #[test]
    fn debounce_requires_consecutive_unique_matches() {
        let mut rule = rule(2);

        assert_eq!(
            rule.evaluate(&observation("1", 206.2)),
            AlertEvaluation::Pending {
                matched: 1,
                required: 2
            }
        );
        assert_eq!(
            rule.evaluate(&observation("1", 206.2)),
            AlertEvaluation::Duplicate
        );
        assert_eq!(
            rule.evaluate(&observation("2", 205.9)),
            AlertEvaluation::Armed
        );
        assert_eq!(
            rule.evaluate(&observation("1", 206.2)),
            AlertEvaluation::Duplicate
        );
        assert_eq!(
            rule.evaluate(&observation("3", 206.3)),
            AlertEvaluation::Pending {
                matched: 1,
                required: 2
            }
        );
        let AlertEvaluation::Triggered(trigger) = rule.evaluate(&observation("4", 206.4)) else {
            panic!("second consecutive match should trigger");
        };
        assert_eq!(trigger.occurrence_id, "price:aapl:206@4");
    }

    #[test]
    fn acknowledgement_latches_until_rule_rearms() {
        let mut rule = rule(1);
        rule.evaluate(&observation("1", 207.0));

        assert!(rule.acknowledge("2026-08-25T20:00:02Z"));
        assert_eq!(
            rule.evaluate(&observation("3", 208.0)),
            AlertEvaluation::Latched
        );
        assert_eq!(
            rule.evaluate(&observation("4", 205.0)),
            AlertEvaluation::Armed
        );
        assert!(matches!(
            rule.evaluate(&observation("5", 207.0)),
            AlertEvaluation::Triggered(_)
        ));
        assert_eq!(
            rule.audit
                .iter()
                .filter(|entry| entry.kind == AlertAuditKind::Triggered)
                .count(),
            2
        );
    }

    #[test]
    fn disabled_rules_record_observations_without_triggering() {
        let mut rule = rule(1);
        rule.toggle("2026-08-25T20:00:00Z");

        assert_eq!(
            rule.evaluate(&observation("1", 210.0)),
            AlertEvaluation::IgnoredDisabled
        );
        assert_eq!(rule.lifecycle, AlertLifecycle::Disabled);
        assert_eq!(rule.status, AlertStatus::Armed);
    }

    #[test]
    fn percent_move_conditions_are_directional() {
        let observation = AlertObservation::new(
            "move-1",
            "us:xnas:nvda",
            180.0,
            -3.25,
            "2026-08-25T20:00:00Z",
        );

        assert!(AlertCondition::percent_move_below(-3.0).matches(&observation));
        assert!(!AlertCondition::percent_move_above(2.0).matches(&observation));
    }

    #[test]
    fn audit_and_idempotency_retention_are_bounded() {
        let mut rule = rule(1);
        for index in 0..(MAX_ALERT_EVALUATION_IDS + 10) {
            let observation = AlertObservation::new(
                format!("evaluation-{index}"),
                "us:xnas:aapl",
                200.0,
                0.0,
                format!("2026-08-25T20:{:02}:00Z", index % 60),
            );
            assert_eq!(rule.evaluate(&observation), AlertEvaluation::Armed);
        }
        assert_eq!(
            rule.runtime_state().processed_evaluation_ids.len(),
            MAX_ALERT_EVALUATION_IDS
        );
        assert_eq!(
            rule.evaluate(&AlertObservation::new(
                "evaluation-10",
                "us:xnas:aapl",
                200.0,
                0.0,
                "2026-08-25T21:00:00Z",
            )),
            AlertEvaluation::Duplicate
        );

        for index in 0..(MAX_ALERT_AUDIT_ENTRIES + 10) {
            rule.toggle(format!("2026-08-26T20:{:02}:00Z", index % 60));
        }
        assert_eq!(rule.audit.len(), MAX_ALERT_AUDIT_ENTRIES);
    }
}
