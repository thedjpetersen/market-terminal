# Analytical Application Services

`market-terminal-application` is the reusable use-case layer between hosts and
the deterministic engine. It prevents HTTP, workers, MCP servers, and future web
frontends from each inventing their own authorization and workload policy.

```text
HTTP / worker / MCP host
          │ host-neutral credential resolver
          │ authenticated server-owned actor
          │ aggregate admission + bounded host execution
          ▼
market-terminal-application
  tenant + principal + capabilities + budgets
          ├── authorized bounded EngineRequest ──▶ market-terminal-engine
          │                                         deterministic analytics
          └── tenant-bound read-only artifact keys ─▶ host-owned query adapter
```

## Owned contracts

- `TenantId` and `PrincipalId` are stable, bounded identities containing only
  ASCII letters, numbers, `-`, `_`, `.`, and `:`. A host derives them from an
  authenticated server-side credential or session; clients never place them in
  `EngineRequest`.
- `CapabilitySet` is a closed set matching the four engine operations. Unknown
  names and empty deployment lists fail configuration rather than silently
  broadening access.
- `ExecutionBudget` limits backtest bars and the combined decisions, trades, and
  equity points in a comparison. Authorization and budgets run before engine
  execution. Domain validation remains the engine's responsibility.
- `AnalyticalApplicationService` is stateless and synchronous. It returns the
  unchanged versioned `EngineResponse`, preserving the v1 transport contract.
- `ResearchArtifactQuery` is a host-neutral, read-only port for Backtest,
  comparison, Screening, News, and Security research artifacts. Its list/get
  keys always contain the authenticated `TenantId`; the client cannot supply or
  override ownership. `ArtifactCapabilitySet` independently gates this surface.
- Artifact pages are capped at 100 entries, cursor/identifiers at 128 bytes, and
  documents at 1 MiB. A next cursor must equal the last already-visible artifact
  ID, preventing an opaque adapter cursor from becoming a tenant data channel.
  The application revalidates schema, ownership, kind filters, identifiers,
  provenance labels, digest envelopes, page size, and document size after every
  adapter call. Cross-tenant or malformed adapter results fail closed without
  disclosing the conflicting tenant.

The application crate intentionally re-exports transport-neutral engine request,
response, error, and schema types. Hosts therefore depend on the application
crate alone and cannot accidentally import the raw engine dispatcher.

## Boundary rules

The crate may depend on the engine and deterministic serialization libraries. It
may not depend on HTTP, async runtimes, clocks, environment variables,
filesystems, sockets, provider clients, persistence adapters, native feature
modules, or terminal UI. `tests/architecture_boundaries.rs` enforces these rules
and also rejects a direct API-to-engine dependency.

Authentication remains host-owned. The independent `market-terminal-auth`
crate defines only a bounded `CredentialResolver` contract that maps presented
credentials to validated actor contexts at a host-supplied observation time. It
has no HTTP, hashing, clock, filesystem, or serialization dependency. The local
`market-terminal-credential-store` adapter implements that contract with a
private, digest-only, immutable startup catalog; a future browser-session or
service adapter can replace it without changing application or engine code.
Tenant-owned artifact and future provider services expose narrow application
ports; they are not added to the deterministic engine. The artifact port has no
concrete storage implementation in this crate, so local files, PostgreSQL, and
remote document services can implement the same ownership contract. The first
local implementation lives in the independent `market-terminal-artifact-store`
crate; only the API binary composition root imports it.

Aggregate rate admission is likewise outside deterministic application
services. `market-terminal-admission` consumes only validated tenant/principal
identity and host-supplied monotonic time. The API applies it after credential
resolution and before any route handler. Blocking-work semaphores and response
deadlines remain HTTP-host concerns; neither the application nor engine imports
Tokio or cancellation policy.

## Next web extraction

1. Add a transactional ingestion writer or service-backed adapter outside this
   crate. The local read adapter and HTTP routes are deliberately mutation-free.
2. Add service-backed hot credential revocation and interactive browser-session
   issuance around the existing resolver contract.
3. Publish cross-language fixtures for actor capabilities and every v1 engine
   request/result before a TypeScript client is allowed to ship.
4. Add distributed admission and explicit cancellable job orchestration before
   horizontal or long-running workloads replace this process-local host.
