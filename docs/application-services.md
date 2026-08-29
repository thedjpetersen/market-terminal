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
          │ authorized bounded EngineRequest
          ▼
market-terminal-engine
  deterministic validation and analytics
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
Likewise, future tenant-owned artifact and provider services should expose narrow
application ports; they must not be added to the deterministic engine.

## Next web extraction

1. Add an application-owned read-only artifact query port with tenant ownership
   in every key and bounded list/get methods.
2. Implement local and service persistence adapters outside this crate; add
   adversarial cross-tenant tests before exposing routes.
3. Add deadline, cancellation, and aggregate rate-budget contracts at the host
   edge while retaining deterministic per-request budgets here.
4. Publish cross-language fixtures for actor capabilities and every v1 engine
   request/result before a TypeScript client is allowed to ship.
