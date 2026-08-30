# API Admission, Deadlines, and Concurrency

The web host applies aggregate admission and bounded execution before invoking
application services. These controls protect the process and its tenants; they
do not alter deterministic analytical inputs, results, or digests.

## Dependency boundary

`market-terminal-admission` owns a small host-neutral contract:

```text
host-observed monotonic time + validated tenant/principal
                            │
                            ▼
                  AdmissionController
                            │ allowed / limited / unavailable
                            ▼
        application services -> deterministic engine
```

The crate has no HTTP, async runtime, clock, environment, filesystem, network,
authentication, or engine dependency. The HTTP host supplies elapsed monotonic
time after authentication. A distributed gateway can implement the same trait
without changing the application or engine.

The default implementation is a bounded in-memory token bucket keyed by the
validated `(tenant_id, principal_id)` pair—not by bearer-token identity. Token
rotation therefore cannot reset an actor's aggregate budget. It has configurable
requests-per-minute, burst, and tracked-actor ceilings. When the actor table is
full it evicts only actors idle for a complete rate window; otherwise admission
fails closed with `503` rather than silently dropping policy state. Unknown
credentials never allocate a bucket.

This first adapter is process-local. Horizontally scaled deployments must inject
a shared controller with equivalent actor isolation and fail-closed behavior.

## Blocking-work boundary

Engine execution and local artifact queries are synchronous, so the Axum host
moves them to Tokio's blocking pool. Independent non-queuing semaphores bound
engine and artifact work. Saturated requests receive `429` immediately rather
than accumulating an unbounded queue.

Each work class has a response deadline. When it expires, the host returns `504`
and cancels its wait for the result. Rust blocking work cannot be forcibly
stopped safely: the task continues to completion and retains its semaphore
permit, ensuring timed-out work cannot evade the in-flight ceiling. This is
bounded response cancellation, not a claim of cooperative CPU cancellation.
Future long-running engines should add job-state cancellation at their own
orchestration boundary while keeping deterministic calculation functions pure.

Rate and concurrency limits are deliberately separate. Every authenticated
protected request consumes aggregate rate capacity, including capability and
artifact reads. Only engine and artifact work consume their respective
concurrency permits.

## Response contract

Allowed authenticated responses include `RateLimit-Limit` and
`RateLimit-Remaining`. A rate rejection returns `429`, `Retry-After`, and
`RateLimit-Reset` in whole seconds. Concurrency saturation also returns `429`
with `Retry-After: 1`. Deadline expiry returns `504`. Authentication and
admission backend failures remain distinct secret-free `503` responses.

The authenticated capability response declares the configured rate, burst,
deadlines, and concurrency ceilings so clients can adapt without guessing.
