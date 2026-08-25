use super::AlertSnapshot;

/// Replay/read-model boundary owned by the Alerts bounded context.
///
/// Implementations may read market events or persisted alert state, but the
/// workspace only sees this context's deterministic snapshot vocabulary.
pub trait AlertsQuery: Send + Sync {
    fn load_snapshot(&self) -> AlertSnapshot;
}
