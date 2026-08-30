//! Host-neutral aggregate request admission for HTTP, worker, MCP, and future
//! web hosts.
//!
//! This crate owns no transport, async runtime, clock, filesystem, network, or
//! persistence behavior. A host supplies monotonic elapsed time and may replace
//! the bounded in-memory controller with a distributed implementation.

use std::{
    collections::BTreeMap,
    fmt,
    sync::{Arc, Mutex},
};

use market_terminal_application::{PrincipalId, TenantId};

pub const RATE_WINDOW_MILLIS: u64 = 60_000;
pub const MIN_REQUESTS_PER_MINUTE: u32 = 1;
pub const MAX_REQUESTS_PER_MINUTE: u32 = 60_000;
pub const MIN_BURST_REQUESTS: u32 = 1;
pub const MAX_BURST_REQUESTS: u32 = 10_000;
pub const MIN_TRACKED_ACTORS: usize = 1;
pub const MAX_TRACKED_ACTORS: usize = 16_384;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ActorAdmissionKey {
    tenant_id: TenantId,
    principal_id: PrincipalId,
}

impl ActorAdmissionKey {
    pub fn new(tenant_id: TenantId, principal_id: PrincipalId) -> Self {
        Self {
            tenant_id,
            principal_id,
        }
    }

    pub fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub fn principal_id(&self) -> &PrincipalId {
        &self.principal_id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdmissionPolicy {
    requests_per_minute: u32,
    burst_requests: u32,
    max_tracked_actors: usize,
}

impl AdmissionPolicy {
    pub fn new(
        requests_per_minute: u32,
        burst_requests: u32,
        max_tracked_actors: usize,
    ) -> Result<Self, AdmissionConfigError> {
        if !(MIN_REQUESTS_PER_MINUTE..=MAX_REQUESTS_PER_MINUTE).contains(&requests_per_minute) {
            return Err(AdmissionConfigError::InvalidRequestsPerMinute(
                requests_per_minute,
            ));
        }
        if !(MIN_BURST_REQUESTS..=MAX_BURST_REQUESTS).contains(&burst_requests)
            || burst_requests > requests_per_minute
        {
            return Err(AdmissionConfigError::InvalidBurst {
                burst_requests,
                requests_per_minute,
            });
        }
        if !(MIN_TRACKED_ACTORS..=MAX_TRACKED_ACTORS).contains(&max_tracked_actors) {
            return Err(AdmissionConfigError::InvalidTrackedActors(
                max_tracked_actors,
            ));
        }
        Ok(Self {
            requests_per_minute,
            burst_requests,
            max_tracked_actors,
        })
    }

    pub const fn requests_per_minute(self) -> u32 {
        self.requests_per_minute
    }

    pub const fn burst_requests(self) -> u32 {
        self.burst_requests
    }

    pub const fn max_tracked_actors(self) -> usize {
        self.max_tracked_actors
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdmissionConfigError {
    InvalidRequestsPerMinute(u32),
    InvalidBurst {
        burst_requests: u32,
        requests_per_minute: u32,
    },
    InvalidTrackedActors(usize),
}

impl fmt::Display for AdmissionConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequestsPerMinute(value) => write!(
                formatter,
                "requests per minute {value} must be between {MIN_REQUESTS_PER_MINUTE} and {MAX_REQUESTS_PER_MINUTE}"
            ),
            Self::InvalidBurst {
                burst_requests,
                requests_per_minute,
            } => write!(
                formatter,
                "burst {burst_requests} must be between {MIN_BURST_REQUESTS} and {MAX_BURST_REQUESTS} and not exceed rate {requests_per_minute}"
            ),
            Self::InvalidTrackedActors(value) => write!(
                formatter,
                "tracked actors {value} must be between {MIN_TRACKED_ACTORS} and {MAX_TRACKED_ACTORS}"
            ),
        }
    }
}

impl std::error::Error for AdmissionConfigError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionDecision {
    Allowed { limit: u32, remaining: u32 },
    Limited { limit: u32, retry_after_millis: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionFailure {
    Unavailable,
}

impl fmt::Display for AdmissionFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("request admission is unavailable")
    }
}

impl std::error::Error for AdmissionFailure {}

pub trait AdmissionController: Send + Sync {
    fn admit(
        &self,
        actor: &ActorAdmissionKey,
        observed_at_millis: u64,
    ) -> Result<AdmissionDecision, AdmissionFailure>;
}

#[derive(Debug, Clone, Copy)]
struct ActorBucket {
    available_milliunits: u64,
    observed_at_millis: u64,
}

#[derive(Clone)]
pub struct InMemoryAdmissionController {
    policy: AdmissionPolicy,
    actors: Arc<Mutex<BTreeMap<ActorAdmissionKey, ActorBucket>>>,
}

impl InMemoryAdmissionController {
    pub fn new(policy: AdmissionPolicy) -> Self {
        Self {
            policy,
            actors: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    pub const fn policy(&self) -> AdmissionPolicy {
        self.policy
    }
}

impl AdmissionController for InMemoryAdmissionController {
    fn admit(
        &self,
        actor: &ActorAdmissionKey,
        observed_at_millis: u64,
    ) -> Result<AdmissionDecision, AdmissionFailure> {
        let mut actors = self
            .actors
            .lock()
            .map_err(|_| AdmissionFailure::Unavailable)?;
        if !actors.contains_key(actor) && actors.len() >= self.policy.max_tracked_actors {
            actors.retain(|_, bucket| {
                observed_at_millis.saturating_sub(bucket.observed_at_millis) < RATE_WINDOW_MILLIS
            });
            if actors.len() >= self.policy.max_tracked_actors {
                return Err(AdmissionFailure::Unavailable);
            }
        }

        let capacity = u64::from(self.policy.burst_requests) * RATE_WINDOW_MILLIS;
        let bucket = actors.entry(actor.clone()).or_insert(ActorBucket {
            available_milliunits: capacity,
            observed_at_millis,
        });
        let elapsed = observed_at_millis.saturating_sub(bucket.observed_at_millis);
        let replenished = elapsed.saturating_mul(u64::from(self.policy.requests_per_minute));
        bucket.available_milliunits = bucket
            .available_milliunits
            .saturating_add(replenished)
            .min(capacity);
        bucket.observed_at_millis = bucket.observed_at_millis.max(observed_at_millis);

        if bucket.available_milliunits >= RATE_WINDOW_MILLIS {
            bucket.available_milliunits -= RATE_WINDOW_MILLIS;
            return Ok(AdmissionDecision::Allowed {
                limit: self.policy.requests_per_minute,
                remaining: (bucket.available_milliunits / RATE_WINDOW_MILLIS) as u32,
            });
        }

        let deficit = RATE_WINDOW_MILLIS - bucket.available_milliunits;
        let rate = u64::from(self.policy.requests_per_minute);
        Ok(AdmissionDecision::Limited {
            limit: self.policy.requests_per_minute,
            retry_after_millis: deficit.div_ceil(rate).max(1),
        })
    }
}

#[cfg(test)]
mod tests {
    use market_terminal_application::{PrincipalId, TenantId};

    use super::*;

    fn actor(tenant: &str, principal: &str) -> ActorAdmissionKey {
        ActorAdmissionKey::new(
            TenantId::new(tenant).unwrap(),
            PrincipalId::new(principal).unwrap(),
        )
    }

    #[test]
    fn token_bucket_is_aggregate_per_actor_and_refills_deterministically() {
        let controller =
            InMemoryAdmissionController::new(AdmissionPolicy::new(60, 2, 8).expect("policy"));
        let first = actor("tenant-a", "analyst");
        let second = actor("tenant-b", "analyst");

        assert_eq!(
            controller.admit(&first, 1_000).unwrap(),
            AdmissionDecision::Allowed {
                limit: 60,
                remaining: 1
            }
        );
        assert!(matches!(
            controller.admit(&first, 1_000).unwrap(),
            AdmissionDecision::Allowed { remaining: 0, .. }
        ));
        assert_eq!(
            controller.admit(&first, 1_000).unwrap(),
            AdmissionDecision::Limited {
                limit: 60,
                retry_after_millis: 1_000
            }
        );
        assert!(matches!(
            controller.admit(&second, 1_000).unwrap(),
            AdmissionDecision::Allowed { remaining: 1, .. }
        ));
        assert!(matches!(
            controller.admit(&first, 2_000).unwrap(),
            AdmissionDecision::Allowed { remaining: 0, .. }
        ));
    }

    #[test]
    fn actor_bound_fails_closed_until_an_idle_bucket_can_be_evicted() {
        let controller =
            InMemoryAdmissionController::new(AdmissionPolicy::new(60, 1, 1).expect("policy"));
        controller.admit(&actor("tenant-a", "one"), 0).unwrap();
        assert_eq!(
            controller.admit(&actor("tenant-b", "two"), 1),
            Err(AdmissionFailure::Unavailable)
        );
        assert!(matches!(
            controller
                .admit(&actor("tenant-b", "two"), RATE_WINDOW_MILLIS)
                .unwrap(),
            AdmissionDecision::Allowed { .. }
        ));
    }

    #[test]
    fn policy_rejects_unbounded_or_self_contradictory_values() {
        assert!(AdmissionPolicy::new(0, 1, 1).is_err());
        assert!(AdmissionPolicy::new(10, 11, 1).is_err());
        assert!(AdmissionPolicy::new(10, 1, 0).is_err());
    }
}
