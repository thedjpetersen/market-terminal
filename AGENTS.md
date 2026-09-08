# Repository architecture

Market Terminal is a modular monolith organized with domain-driven design and
package-by-feature. Treat each `src/features/<name>/` directory as a bounded
context, and preserve the dependency rules in `docs/architecture.md`.

## Ownership

- Put new product behavior in the feature that owns the domain concept. A
  feature owns its domain types and invariants, feature-owned ports,
  application behavior, local interaction state, and terminal workspace
  adapter.
- When a feature grows, split it into private modules around cohesive domain
  capabilities. Do not create horizontal catch-all modules such as global
  `models`, `handlers`, `utils`, screen registries, renderers, or data providers.
- Keep domain rules in the owning feature and free of terminal rendering,
  application-shell state, provider schemas, networking, and filesystem
  formats.
- Re-export only the types and behavior callers genuinely need. File size alone
  is not a reason to widen a module's public surface.

## Dependency direction

- `app` owns lifecycle, input, shell state, and the stable `Workspace` contract;
  it must remain free of market, portfolio, and other feature business rules.
- Features may depend on `foundation`, shared `ui` primitives, and `app`
  contracts where presentation requires them. Feature domain and port modules
  must not depend on `app`, `ui`, or `infrastructure`.
- A feature must never import `infrastructure` or another feature. Cross-context
  reads use consumer-owned types and ports, implemented by an infrastructure
  translator and wired in `bootstrap.rs`.
- Cross-context notifications use publisher-owned typed events carried by the
  application event bus; consumers must not import the publishing feature's
  domain graph.
- `foundation` contains only stable, narrowly shared value objects. Do not move
  provider types or feature state into it merely to avoid a dependency.
- `ui` contains domain-free visual primitives and shell geometry. It must not
  know feature entities or infrastructure adapters.
- `infrastructure` implements feature-owned ports and owns provider, network,
  filesystem, and wire-format details.
- `bootstrap.rs` is the composition root and the only place that selects
  concrete adapters and registers the complete product surface.

## Module design

- Prefer deep modules: keep caller-facing interfaces small while concentrating
  related behavior, invariants, error handling, and tests inside the owning
  module.
- Add a seam only for real variation. A live and deterministic adapter justify
  a feature-owned port; a single implementation with no credible alternative
  usually does not.
- Preserve locality when modularizing: callers should not need to coordinate a
  sequence of shallow modules to complete one domain operation.
- Keep the external `Workspace` and feature-owned port contracts stable while
  reorganizing private implementation details inside a bounded context.
- Apply these dependency rules to every domain module regardless of its file or
  directory name. The architecture test catches common violations but is not a
  complete dependency-graph analysis.

## Verification

Run `cargo test --test architecture_boundaries` after dependency or module
changes. This test enforces production feature isolation, domain/port
independence, and the stable dependency direction of `foundation` and `ui`.

## Agent skills

### Issue tracker

Issues and PRDs are tracked in GitHub Issues. See `docs/agents/issue-tracker.md`.

### Triage labels

Triage uses the five canonical role labels. See `docs/agents/triage-labels.md`.

### Domain docs

Read `docs/agents/domain.md` for domain-document discovery and terminology
rules. Its document layout does not redefine the feature bounded contexts.
