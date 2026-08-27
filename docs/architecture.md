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

- `app` owns lifecycle, input modes, keyboard/mouse routing, and the stable
  `Workspace` plug-in contract. It has no market or portfolio business rules.
  Its generic `DeskWorkspace` composes three existing workspace instances and
  routes focus/render/input without importing their domain models.
  It snapshots its shell state through the persistence context's narrow
  repository port.
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
- `ui` contains shell geometry and hit testing plus the design system: theme
  tokens, chrome, tables, panels, and value styling. It does not know business
  entities.
- `bootstrap.rs` is the composition root. It is the only place that selects
  concrete adapters and registers the complete product surface.

The chat context follows the same boundary: `features/chat` owns endpoint and
message rules plus the `ChatGateway` port, while `infrastructure/irc.rs` owns
Tokio, TLS, environment configuration, reconnection, and wire protocol details.
The native workspace never depends on the IRC crate and never performs network
work during input or rendering.

The interactive composition root uses `LiveNewsFeed`, which owns a bounded
background RSS/Atom worker, performs explicit on-demand readability extraction,
and exposes cloned provider-neutral workbench snapshots, and
`CsvPortfolioRepository`, which owns the last successfully validated USD
positions import. Demo news and portfolio data are wired only by `demo_app` for
deterministic tests and gallery captures. Network and filesystem formats remain
outside the feature packages.

`LiveOverviewQuery` composes those two already-loaded, in-memory snapshots for
the interactive Overview. It performs no network or filesystem work and does
not infer performance history from a point-in-time CSV. Missing return, risk,
attribution, and mover data is rendered as unavailable rather than replaced by
the gallery values.

Spreadsheet financial formulas use the Spreadsheet-owned
`SpreadsheetMarketData` batch port. The workspace recursively extracts
`PX_LAST`, `PX_CHANGE`, `HISTORY`, and `FUNDAMENTAL` requests, sends them through
a capacity-bounded worker, and substitutes returned values into a cloned
evaluation snapshot so nested formulas and undo history remain provider-neutral
and deterministic. External-cell state is
kept alongside the workbook and carries provider, observation/receive times,
quality, entitlement failure, and availability. The pure formula evaluator does
not make arbitrary provider calls.

At the persistent-app composition root, `LiveSpreadsheetMarketData` routes
quote/history requests to the operator-selected market adapter and annual
fundamental requests to the existing SEC adapter. Alpha Vantage resolves scalar
daily `HISTORY`; SEC Company Facts resolves reported `FUNDAMENTAL`. This
infrastructure-only composition preserves request order and keeps provider
types out of Spreadsheet, Security, and the kernel.

The persistent Spreadsheet starts with an empty workbook and receives
`LocalSpreadsheetFiles` through its feature-owned `SpreadsheetFileStore` port.
CSV import/export is bounded, formula-preserving, active-sheet scoped, and
explicit about overwrite intent. Complete workbook persistence is a separate
versioned payload behind the Spreadsheet-owned `SpreadsheetWorkbookStore` port
and the crash-safe feature-document adapter. The gallery-only constructor
retains the seeded IBM workbook for deterministic captures; `persistent_app`
never wires that seed.

The interactive Monitor uses the same Alpha Vantage adapter through the
Market-Data-owned `MarketDataQuery` port. `WatchlistWorkspace` submits snapshots
to a capacity-one worker and only applies results during polling; construction,
input, and rendering perform no network I/O. A shared 60-second adapter cache
coalesces the Monitor and Spreadsheet's identical provider requests. Demo
adapters remain available only to deterministic unit/gallery hosts and are not
wired by `persistent_app` for these surfaces.

`LiveMarketsQuery` applies the same rule to Markets: listed-instrument
snapshots are loaded on a coalescing background worker and retain provider,
observation time, and quality. Cross-asset rates, currencies, commodities,
breadth, sectors, and calendar panels stay explicitly unavailable until
source-specific ports exist; equity proxies are not presented as those data.

## Why there is no global data service

A single `MarketDataProvider` spanning quotes, portfolios, news, analytics,
and execution becomes a dependency magnet. Instead, each bounded context owns
the smallest port it needs (`MarketsQuery`, `PortfolioRepository`, `NewsFeed`, and
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
`AssistantGateway` port. Infrastructure provides two adapters: the default
Codex app-server adapter uses the user's cached ChatGPT login and constrained
structured output, while the OpenRouter adapter uses the standardized
chat-completions/tool-calling API. The Codex worker keeps one app-server process
warm and creates a fresh ephemeral, read-only thread for each request. Requests
run behind a bounded background channel; the render/input loop only polls for
results. The shell presents the conversation as a toggleable drawer so the
underlying research workspace remains mounted and visible.

Model output never receives an `App` reference. It is translated into the
closed `AppIntent` vocabulary and revalidated by `WorkspaceRegistry`:

```text
user prompt -> AssistantGateway -> constrained provider response
            -> UiAction -> AppIntent -> exact registry resolution -> shell update
```

The allowed mutations are workspace focus, navigation promotion, existing
command dispatch, and default-order restoration. Unknown tools, malformed
arguments, unknown targets, and unknown commands are rejected. The Codex
adapter reuses the CLI's authentication cache without reading or copying its
tokens; API-provider credentials are read from the process environment. No
credential is stored in feature state, conversation history, logs, or model
context.

## Growth path

The next structural steps are intentionally additive:

- add source-specific, entitlement-aware rates, currencies, commodities,
  breadth, sector aggregation, calendar, and portfolio-history adapters;
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
