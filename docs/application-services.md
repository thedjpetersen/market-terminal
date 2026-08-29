# Analytical Application Services

`market-terminal-application` is the reusable use-case layer between hosts and
the deterministic engine. It prevents HTTP, workers, MCP servers, and future web
frontends from each inventing their own authorization and workload policy.

```text
HTTP / worker / MCP host
          │ authenticated server-owned actor
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

Authentication remains host-owned. The current HTTP host maps one configured
bearer credential to one actor context. A future credential/session adapter may
resolve many actors, but it must produce the same validated `ExecutionContext`.
Tenant-owned artifact and future provider services expose narrow application
ports; they are not added to the deterministic engine. The artifact port has no
concrete storage implementation in this crate, so local files, PostgreSQL, and
remote document services can implement the same ownership contract. The first
local implementation lives in the independent `market-terminal-artifact-store`
crate; only the API binary composition root imports it.

## Next web extraction

1. Add a transactional ingestion writer or service-backed adapter outside this
   crate. The local read adapter and HTTP routes are deliberately mutation-free.
2. Add credential/session resolution with adversarial cross-tenant integration
   tests against each concrete repository.
3. Add deadline, cancellation, and aggregate rate-budget contracts at the host
   edge while retaining deterministic per-request budgets here.
4. Publish cross-language fixtures for actor capabilities and every v1 engine
   request/result before a TypeScript client is allowed to ship.
