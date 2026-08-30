mod controls;
mod domain;
mod port;
mod workspace;

pub use domain::{
    AlertAuditEntry, AlertAuditKind, AlertCondition, AlertDelivery, AlertEvaluation,
    AlertLifecycle, AlertObservation, AlertRule, AlertRuleId, AlertRuleRuntimeState, AlertSnapshot,
    AlertStateValidationError, AlertStatus, AlertTrigger, DebouncePolicy, InstrumentRef,
    MAX_ALERT_AUDIT_ENTRIES, MAX_ALERT_EVALUATION_IDS, MAX_ALERT_RULES,
};
pub use port::{AlertRulesState, AlertStateError, AlertStateStore, AlertsError, AlertsQuery};
pub use workspace::AlertsWorkspace;

use crate::app::WorkspaceId;

pub const ID: WorkspaceId = WorkspaceId::new("alerts");
