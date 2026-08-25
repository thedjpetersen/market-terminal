# Architecture

Market Terminal uses domain-driven design with package-by-feature boundaries.
The goal is to let many teams add terminal functions without coordinating
changes through a central screen enum, global data service, or monolithic
renderer.

## Dependency direction

```text
bootstrap ──▶ app kernel
    │            ▲
    ├────────▶ features ──▶ foundation + shared UI primitives
    │              ▲
    └────▶ infrastructure adapters
```

- `app` owns lifecycle, input modes, and the stable `Workspace` plug-in
  contract. It has no market or portfolio business rules. It snapshots its
  shell state through the persistence context's narrow repository port.
- `features/<name>` is a bounded context. It owns its domain types, outbound
  query port, local UI state, and terminal workspace adapter.
- `foundation` contains only stable, narrowly shared value objects. Canonical
  instrument identity lives here; provider quote schemas and feature state do
  not.
- `infrastructure` implements feature-owned ports. A live adapter can replace
  `DemoData` without changing feature code.
- `persistence` owns versioned session and opaque feature-document contracts;
  the local adapter provides bounded reads, atomic writes, and previous-valid
  generation recovery without knowing feature internals.
- `ui` contains the design system only: theme tokens, chrome, tables, panels,
  and value styling. It does not know business entities.
- `bootstrap.rs` is the composition root. It is the only place that selects
  concrete adapters and registers the complete product surface.

## Why there is no global data service

A single `MarketDataProvider` spanning quotes, portfolios, news, analytics,
and execution becomes a dependency magnet. Instead, each bounded context owns
the smallest port it needs (`MarketsQuery`, `PortfolioQuery`, `NewsQuery`, and
so on). Infrastructure may implement several ports, but features never depend
on that concrete adapter.

## Adding a terminal function

1. Create `src/features/<function>/` with `domain.rs`, `port.rs`, and
   `workspace.rs`.
2. Implement the `Workspace` contract and publish a unique `WorkspaceId`,
   hotkey, and command aliases.
3. Add an infrastructure adapter for the feature-owned port.
4. Register the workspace in `bootstrap.rs`.

No root router match, shared screen state, or central data trait needs to be
edited. The registry validates duplicate IDs and hotkeys at startup.

## Cross-feature events

The application kernel owns an in-process, typed event bus. Subscriptions are
topic-filtered and bounded: a slow consumer drops its oldest pending envelope
and exposes a drop count instead of allowing an unbounded queue to stall the
terminal. Subscriptions can be cancelled explicitly and each envelope carries
a monotonic sequence number.

The bus does not own business schemas. A publishing feature defines its event
type and topic; consumers downcast the typed envelope at their boundary. This
keeps transport mechanics in the kernel while preserving domain ownership.

## AI command plane

The Assistant bounded context owns conversation state and an
`AssistantGateway` port. The OpenRouter adapter lives in `infrastructure` and
uses the standardized chat-completions/tool-calling API. Requests run on a
background worker; the render/input loop only polls a channel.

Model output never receives an `App` reference. It is translated into the
closed `AppIntent` vocabulary and revalidated by `WorkspaceRegistry`:

```text
user prompt -> AssistantGateway -> OpenRouter tool call
            -> UiAction -> AppIntent -> exact registry resolution -> shell update
```

The allowed mutations are workspace focus, navigation promotion, existing
command dispatch, and default-order restoration. Unknown tools, malformed
arguments, unknown targets, and unknown commands are rejected. Credentials are
read from the process environment by the infrastructure adapter and are not
stored in feature state, conversation history, logs, or model context.

## Growth path

The next structural steps are intentionally additive:

- split `DemoData` into live quote, reference-data, news, and portfolio
  adapters;
- connect streaming adapters to the bounded event bus with acknowledgement and
  tracing where delivery guarantees require it;
- add caching, retries, entitlements, and observability as infrastructure
  decorators around feature ports;
- move bounded contexts into workspace crates when build times or team
  ownership justify a Cargo workspace;
- migrate individual saved watchlists, workbooks, charts, and alert rules onto
  the opaque feature-document repository as their domain contracts stabilize.

The current boundary is deliberately a modular monolith. It gives strong
ownership and test seams without paying the operational cost of services or a
large multi-crate graph before those costs are warranted.
