use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::features::alerts::{
    AlertAuditEntry, AlertAuditKind, AlertCondition, AlertLifecycle, AlertObservation, AlertRule,
    AlertRuleId, AlertRuleRuntimeState, AlertRulesState, AlertStateError, AlertStatus,
    DebouncePolicy, InstrumentRef, MAX_ALERT_RULES,
};

const ALERT_STATE_FORMAT_VERSION: u64 = 1;
const MAX_ID_BYTES: usize = 512;
const MAX_SYMBOL_BYTES: usize = 64;
const MAX_CONFIRMATIONS: u8 = 64;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredAlertRules {
    format_version: u64,
    rules: Vec<StoredAlertRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredAlertRule {
    id: String,
    canonical_instrument_id: String,
    symbol: String,
    condition: StoredCondition,
    lifecycle: StoredLifecycle,
    status: StoredStatus,
    debounce_confirmations: u8,
    last_observation: Option<StoredObservation>,
    audit: Vec<StoredAuditEntry>,
    processed_evaluation_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum StoredCondition {
    PriceAbove { threshold: f64 },
    PriceBelow { threshold: f64 },
    PercentMoveAbove { threshold: f64 },
    PercentMoveBelow { threshold: f64 },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum StoredLifecycle {
    Enabled,
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum StoredStatus {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredObservation {
    evaluation_id: String,
    instrument_id: String,
    price: f64,
    percent_move: f64,
    observed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredAuditEntry {
    kind: StoredAuditKind,
    at: String,
    detail: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum StoredAuditKind {
    Enabled,
    Disabled,
    Triggered,
    Acknowledged,
    Rearmed,
}

pub(super) fn encode_alert_rules(state: &AlertRulesState) -> Result<Value, AlertStateError> {
    if state.rules.len() > MAX_ALERT_RULES {
        return Err(AlertStateError::Corrupt(format!(
            "rule count exceeds {MAX_ALERT_RULES}"
        )));
    }
    let mut ids = BTreeSet::new();
    let mut rules = Vec::with_capacity(state.rules.len());
    for rule in &state.rules {
        if !ids.insert(rule.id.as_str()) {
            return Err(AlertStateError::Corrupt(format!(
                "duplicate rule ID {}",
                rule.id
            )));
        }
        let stored = StoredAlertRule::from(rule);
        stored.clone().into_domain()?;
        rules.push(stored);
    }
    let stored = StoredAlertRules {
        format_version: ALERT_STATE_FORMAT_VERSION,
        rules,
    };
    serde_json::to_value(stored).map_err(|error| AlertStateError::Io(error.to_string()))
}

pub(super) fn decode_alert_rules(
    revision: u64,
    payload: &Value,
) -> Result<AlertRulesState, AlertStateError> {
    let stored: StoredAlertRules = serde_json::from_value(payload.clone())
        .map_err(|error| AlertStateError::Corrupt(error.to_string()))?;
    if stored.format_version != ALERT_STATE_FORMAT_VERSION {
        return Err(AlertStateError::Unsupported(format!(
            "expected alert-state format {ALERT_STATE_FORMAT_VERSION}, found {}",
            stored.format_version
        )));
    }
    if stored.rules.len() > MAX_ALERT_RULES {
        return Err(AlertStateError::Corrupt(format!(
            "rule count exceeds {MAX_ALERT_RULES}"
        )));
    }

    let mut ids = BTreeSet::new();
    let mut rules = Vec::with_capacity(stored.rules.len());
    for stored_rule in stored.rules {
        let rule = stored_rule.into_domain()?;
        if !ids.insert(rule.id.as_str().to_owned()) {
            return Err(AlertStateError::Corrupt(format!(
                "duplicate rule ID {}",
                rule.id
            )));
        }
        rules.push(rule);
    }
    AlertRulesState::new(revision, rules)
}

impl From<&AlertRule> for StoredAlertRule {
    fn from(rule: &AlertRule) -> Self {
        let runtime = rule.runtime_state();
        Self {
            id: rule.id.as_str().to_owned(),
            canonical_instrument_id: rule.instrument.canonical_id.as_str().to_owned(),
            symbol: rule.instrument.symbol.clone(),
            condition: rule.condition.into(),
            lifecycle: runtime.lifecycle.into(),
            status: runtime.status.into(),
            debounce_confirmations: rule.debounce.confirmations(),
            last_observation: runtime.last_observation.map(Into::into),
            audit: runtime.audit.into_iter().map(Into::into).collect(),
            processed_evaluation_ids: runtime.processed_evaluation_ids,
        }
    }
}

impl StoredAlertRule {
    fn into_domain(self) -> Result<AlertRule, AlertStateError> {
        validate_identifier(&self.id, "rule ID", MAX_ID_BYTES)?;
        validate_identifier(
            &self.canonical_instrument_id,
            "canonical instrument ID",
            MAX_ID_BYTES,
        )?;
        validate_symbol(&self.symbol)?;
        if self.debounce_confirmations == 0 || self.debounce_confirmations > MAX_CONFIRMATIONS {
            return Err(AlertStateError::Corrupt(format!(
                "debounce confirmations must be between 1 and {MAX_CONFIRMATIONS}"
            )));
        }
        let instrument = InstrumentRef::new(self.canonical_instrument_id, self.symbol);
        let last_observation = self
            .last_observation
            .map(StoredObservation::into_domain)
            .transpose()?;
        let state = AlertRuleRuntimeState {
            lifecycle: self.lifecycle.into(),
            status: self.status.into_domain()?,
            last_observation,
            audit: self.audit.into_iter().map(Into::into).collect(),
            processed_evaluation_ids: self.processed_evaluation_ids,
        };
        AlertRule::restore(
            AlertRuleId::new(self.id),
            instrument,
            self.condition.into_domain()?,
            DebouncePolicy::consecutive(self.debounce_confirmations),
            state,
        )
        .map_err(|error| AlertStateError::Corrupt(error.to_string()))
    }
}

impl From<AlertCondition> for StoredCondition {
    fn from(condition: AlertCondition) -> Self {
        match condition {
            AlertCondition::PriceAbove { threshold } => Self::PriceAbove { threshold },
            AlertCondition::PriceBelow { threshold } => Self::PriceBelow { threshold },
            AlertCondition::PercentMoveAbove { threshold } => Self::PercentMoveAbove { threshold },
            AlertCondition::PercentMoveBelow { threshold } => Self::PercentMoveBelow { threshold },
        }
    }
}

impl StoredCondition {
    fn into_domain(self) -> Result<AlertCondition, AlertStateError> {
        let (threshold, price) = match self {
            Self::PriceAbove { threshold } | Self::PriceBelow { threshold } => (threshold, true),
            Self::PercentMoveAbove { threshold } | Self::PercentMoveBelow { threshold } => {
                (threshold, false)
            }
        };
        if !threshold.is_finite() || price && threshold < 0.0 {
            return Err(AlertStateError::Corrupt(
                "alert threshold is invalid".to_owned(),
            ));
        }
        Ok(match self {
            Self::PriceAbove { threshold } => AlertCondition::price_above(threshold),
            Self::PriceBelow { threshold } => AlertCondition::price_below(threshold),
            Self::PercentMoveAbove { threshold } => AlertCondition::percent_move_above(threshold),
            Self::PercentMoveBelow { threshold } => AlertCondition::percent_move_below(threshold),
        })
    }
}

impl From<AlertLifecycle> for StoredLifecycle {
    fn from(lifecycle: AlertLifecycle) -> Self {
        match lifecycle {
            AlertLifecycle::Enabled => Self::Enabled,
            AlertLifecycle::Disabled => Self::Disabled,
        }
    }
}

impl From<StoredLifecycle> for AlertLifecycle {
    fn from(lifecycle: StoredLifecycle) -> Self {
        match lifecycle {
            StoredLifecycle::Enabled => Self::Enabled,
            StoredLifecycle::Disabled => Self::Disabled,
        }
    }
}

impl From<AlertStatus> for StoredStatus {
    fn from(status: AlertStatus) -> Self {
        match status {
            AlertStatus::Armed => Self::Armed,
            AlertStatus::Pending { matched, required } => Self::Pending { matched, required },
            AlertStatus::Triggered {
                occurrence_id,
                triggered_at,
            } => Self::Triggered {
                occurrence_id,
                triggered_at,
            },
            AlertStatus::Acknowledged {
                occurrence_id,
                acknowledged_at,
            } => Self::Acknowledged {
                occurrence_id,
                acknowledged_at,
            },
        }
    }
}

impl StoredStatus {
    fn into_domain(self) -> Result<AlertStatus, AlertStateError> {
        Ok(match self {
            Self::Armed => AlertStatus::Armed,
            Self::Pending { matched, required } => AlertStatus::Pending { matched, required },
            Self::Triggered {
                occurrence_id,
                triggered_at,
            } => {
                validate_identifier(&occurrence_id, "occurrence ID", MAX_ID_BYTES)?;
                validate_identifier(&triggered_at, "trigger time", MAX_ID_BYTES)?;
                AlertStatus::Triggered {
                    occurrence_id,
                    triggered_at,
                }
            }
            Self::Acknowledged {
                occurrence_id,
                acknowledged_at,
            } => {
                validate_identifier(&occurrence_id, "occurrence ID", MAX_ID_BYTES)?;
                validate_identifier(&acknowledged_at, "acknowledgement time", MAX_ID_BYTES)?;
                AlertStatus::Acknowledged {
                    occurrence_id,
                    acknowledged_at,
                }
            }
        })
    }
}

impl From<AlertObservation> for StoredObservation {
    fn from(observation: AlertObservation) -> Self {
        Self {
            evaluation_id: observation.evaluation_id,
            instrument_id: observation.instrument_id.as_str().to_owned(),
            price: observation.price,
            percent_move: observation.percent_move,
            observed_at: observation.observed_at,
        }
    }
}

impl StoredObservation {
    fn into_domain(self) -> Result<AlertObservation, AlertStateError> {
        validate_identifier(&self.evaluation_id, "evaluation ID", MAX_ID_BYTES)?;
        validate_identifier(
            &self.instrument_id,
            "observation instrument ID",
            MAX_ID_BYTES,
        )?;
        validate_identifier(&self.observed_at, "observation time", MAX_ID_BYTES)?;
        if !self.price.is_finite() || !self.percent_move.is_finite() {
            return Err(AlertStateError::Corrupt(
                "observation contains a non-finite value".to_owned(),
            ));
        }
        Ok(AlertObservation::new(
            self.evaluation_id,
            self.instrument_id,
            self.price,
            self.percent_move,
            self.observed_at,
        ))
    }
}

impl From<AlertAuditEntry> for StoredAuditEntry {
    fn from(entry: AlertAuditEntry) -> Self {
        Self {
            kind: entry.kind.into(),
            at: entry.at,
            detail: entry.detail,
        }
    }
}

impl From<StoredAuditEntry> for AlertAuditEntry {
    fn from(entry: StoredAuditEntry) -> Self {
        Self {
            kind: entry.kind.into(),
            at: entry.at,
            detail: entry.detail,
        }
    }
}

impl From<AlertAuditKind> for StoredAuditKind {
    fn from(kind: AlertAuditKind) -> Self {
        match kind {
            AlertAuditKind::Enabled => Self::Enabled,
            AlertAuditKind::Disabled => Self::Disabled,
            AlertAuditKind::Triggered => Self::Triggered,
            AlertAuditKind::Acknowledged => Self::Acknowledged,
            AlertAuditKind::Rearmed => Self::Rearmed,
        }
    }
}

impl From<StoredAuditKind> for AlertAuditKind {
    fn from(kind: StoredAuditKind) -> Self {
        match kind {
            StoredAuditKind::Enabled => Self::Enabled,
            StoredAuditKind::Disabled => Self::Disabled,
            StoredAuditKind::Triggered => Self::Triggered,
            StoredAuditKind::Acknowledged => Self::Acknowledged,
            StoredAuditKind::Rearmed => Self::Rearmed,
        }
    }
}

fn validate_identifier(value: &str, field: &str, maximum: usize) -> Result<(), AlertStateError> {
    if value.trim().is_empty() || value.len() > maximum {
        return Err(AlertStateError::Corrupt(format!(
            "{field} is empty or exceeds {maximum} bytes"
        )));
    }
    Ok(())
}

fn validate_symbol(value: &str) -> Result<(), AlertStateError> {
    validate_identifier(value, "symbol", MAX_SYMBOL_BYTES)?;
    if !value
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_alphanumeric() || character == '^')
        || value.contains("..")
        || !value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '/' | '^' | '_')
        })
    {
        return Err(AlertStateError::Corrupt(
            "symbol contains an unsafe provider character".to_owned(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_trigger_and_idempotency_state_round_trips() {
        let mut rule = AlertRule::new(
            AlertRuleId::new("local:ibm:1"),
            InstrumentRef::new("us:listed:ibm", "IBM"),
            AlertCondition::price_above(100.0),
            DebouncePolicy::consecutive(1),
        );
        let observation = AlertObservation::new(
            "provider:ibm:2026-08-27",
            "us:listed:ibm",
            250.0,
            1.5,
            "2026-08-27T20:00:00Z",
        );
        assert!(matches!(
            rule.evaluate(&observation),
            crate::features::alerts::AlertEvaluation::Triggered(_)
        ));
        assert!(rule.acknowledge("2026-08-27T20:01:00Z"));
        let state = AlertRulesState::new(7, vec![rule]).unwrap();

        let restored = decode_alert_rules(7, &encode_alert_rules(&state).unwrap()).unwrap();

        assert_eq!(restored, state);
        assert_eq!(
            restored.rules[0].clone().evaluate(&observation),
            crate::features::alerts::AlertEvaluation::Duplicate
        );
    }

    #[test]
    fn unknown_versions_and_duplicate_rules_are_rejected() {
        let unsupported = serde_json::json!({"format_version": 99, "rules": []});
        assert!(matches!(
            decode_alert_rules(1, &unsupported),
            Err(AlertStateError::Unsupported(_))
        ));

        let rule = AlertRule::new(
            AlertRuleId::new("same"),
            InstrumentRef::new("us:listed:ibm", "IBM"),
            AlertCondition::price_above(100.0),
            DebouncePolicy::consecutive(1),
        );
        let state = AlertRulesState {
            revision: 1,
            rules: vec![rule.clone(), rule],
        };
        assert!(matches!(
            encode_alert_rules(&state),
            Err(AlertStateError::Corrupt(_))
        ));
    }
}
