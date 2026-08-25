mod domain;
mod port;
mod workspace;

pub use domain::{
    AlertAuditEntry, AlertAuditKind, AlertCondition, AlertDelivery, AlertEvaluation,
    AlertLifecycle, AlertObservation, AlertRule, AlertRuleId, AlertSnapshot, AlertStatus,
    AlertTrigger, DebouncePolicy, InstrumentRef,
};
pub use port::AlertsQuery;
pub use workspace::AlertsWorkspace;

use crate::app::WorkspaceId;

pub const ID: WorkspaceId = WorkspaceId::new("alerts");
