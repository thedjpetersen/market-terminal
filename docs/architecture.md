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
  Visible feature-local destinations are published as bounded `WorkspaceAction`
  values with opaque stable IDs, labels, enabled state, and render-relative
  rectangles. The registry rejects invalid geometry and duplicate or oversized
  actions; activation is revalidated against the current feature state.
  Its generic `DeskWorkspace` composes three existing workspace instances and
  routes focus/render/input without importing their domain models.
  Versioned role workspace presets are validated from a bounded declarative
  seed document. The shell projects them through the live registry, previews
  unavailable destinations before mutation, and stores one encoded custom
  return point inside the existing session preferences. Presets therefore do
  not import feature state, hard-code registry indices, or erase layouts when a
  workspace is added or retired.
  Saved views use a shell-owned typed envelope rather than importing feature
  domain structs. Each workspace owns a bounded field schema behind
  `capture_view`/`restore_view`; composed workspaces nest child envelopes. The
  shell validates depth, field, list, text, workspace, and catalog bounds,
  persists a versioned catalog through the opaque feature-document repository,
  and reports skipped or unavailable capabilities on restore. Desk owns a
  bounded geometry value object alongside its child workspaces: one typed state
  stores Monitor width and top-row height, while the same computed rectangles
  drive rendering, mouse hit testing, pane-body routing, resize buttons,
  spatial focus, and follow hints. Keyboard steps clamp at declared bounds;
  exact layout commands validate both axes before mutating, so a malformed
  two-axis request cannot partially resize the Desk. Legacy views restore the
  original geometry and invalid percentages produce a degraded report without
  preventing valid pane or child restoration. Security owns another typed
  adopter: its envelope carries provider-neutral instrument identity, terminal
  subject, research-tab key, and optional stable Form 4 accession. The accession
  is retained while asynchronous data reloads and resolved against the returned
  page by identity rather than row index. Reordered rows therefore restore
  exactly; a disappeared filing selects the first available row and discloses
  the fallback in the source-status panel. Provider page data and document URLs
  never enter the saved-view document. News owns its filter and selection schema:
  region, topic, symbol, unread-only, saved-only, Stories/Events subview, and
  optional provider story ID. Restore applies bounded filters first and then
  resolves the story only within the filtered result by identity, so feed
  reordering cannot silently change the selected headline. Removed stories
  degrade to the first visible result, or to a disclosed empty state when no
  result survives. Calendar rendering selects an explicit full or compact column
  schema before constructing its table; narrow detail panes therefore preserve
  time, region, importance, event, and survey instead of passing an
  overconstrained layout to the table solver. The in-terminal reader is a trapped
  transient modal rather than layout state; article bodies, publisher URLs, read
  history, and bookmarks remain in their owning News data/session boundary.
  Monitor owns a provider-neutral table schema over the same envelope:
  watchlist identity, sort field/direction, stable configured-column keys,
  active column preset, selected canonical instrument, and top-visible
  instrument. Restoration resolves the list through the Watchlist catalog,
  validates columns as a unique set that retains Symbol identity, sorts before
  rematching row and viewport anchors, and degrades retired identities
  independently. Rendering, mouse rows, spatial actions, and follow hints share
  one viewport window. Live quote and stream re-sorts preserve the selected
  instrument by identity and move the window only as needed to keep it visible;
  provider snapshots, trace samples, and stream status never cross into shell
  persistence.
  Portfolio owns a parallel eight-subview table schema. Its envelope stores only
  the active subview key plus stable selected-row and top-row anchors. Positions
  and calculated contribution rows use account/instrument/currency composites;
  activity, lot, realized-gain, and execution tables use feature-owned record
  IDs; performance uses currency. Restore loads the current snapshot and
  rematches both anchors by identity, degrading unavailable or malformed fields
  independently. Rendering, pointer hit testing, arrows, spatial actions, and
  follow hints consume one bounded viewport. Opaque row actions include an
  identity digest and fail closed after asynchronous replacement. Portfolio
  data, broker records, calculations, methodology, and source status remain
  behind the feature repository rather than entering shell persistence.
  Alerts is the asynchronous durable-table reference. Its view envelope stores
  only selected-rule and top-visible-rule IDs, never thresholds, observations,
  debounce/trigger state, delivery, or audit history from the independently
  persisted rule register. A restore that precedes asynchronous rule loading
  keeps bounded pending identities and resolves them when the register arrives.
  Live snapshot application preserves the selected and top rules by ID. One
  bounded viewport drives table rendering, mouse rows, arrow reveal, spatial
  actions, and follow hints; action activation still revalidates the exact rule
  ID after insertion or replacement.
  Unknown fields remain inert data, so a newer snapshot can degrade on an older
  binary without coupling migrations to the registry or panicking at startup.
  Session and saved-view documents remain independent failure domains.
  It snapshots its shell state through the persistence context's narrow
  repository ports.
- `features/<name>` is a bounded context. It owns its domain types, outbound
  query port, local UI state, and terminal workspace adapter.
  Launchpad follows this rule: its bounded tile aggregate owns validation,
  stable identity, revisioning, typed destination semantics, portable document
  merge/replace rules, and edit semantics. Its workspace maps tiles to opaque
  stale-safe actions and validated shell intents; its narrow state and portable
  file ports are implemented by separate local adapters. A capacity-one worker
  coalesces state edits so disk I/O never runs in the input or rendering path.
  Portable files deliberately omit machine-local identity and revision state.
  The shell accepts saved-layout intents through its saved-view service, while
  instrument, screen, portfolio, and sheet commands continue through the owning
  workspace registry. This preserves the feature boundary without making saved
  views a Launchpad dependency. See `docs/launchpad.md` for the routing and
  migration contract.
  Mission Control is an Overview-owned read model rather than a cross-feature
  domain import. The live infrastructure composer translates Markets,
  Portfolio, News, Alerts, and Launchpad snapshots into Overview DTOs with
  explicit quality, as-of, ranking rationale, and drill-down commands. Durable
  local documents are sampled during startup; render and input paths consume
  only memory. The News port exposes provider-backed events separately so the
  deterministic gallery calendar cannot appear in the live daily surface.
  Screening follows the same consumer-owned projection rule. Its domain owns
  universe snapshots, typed clauses, null policy, deterministic ranking,
  evidence, and saved-definition validation. A composition-root
  `MarketScreeningUniverseQuery` translates Watchlist membership plus one
  Market Data quote batch into Screening DTOs; Screening never imports either
  peer feature's types. The translation emits one immutable input version,
  provider set, as-of, field availability, and canonical identities without
  blending observations. Evaluation and definition persistence each use a
  separate capacity-one worker, so provider and disk I/O never enter input or
  rendering. The evaluation worker records successful live inputs through a
  separate `UniverseHistoryStore` port and can load an exact historical version
  without consulting the provider.
  Generation checks reject stale evaluations, while a failed refresh retains
  the explicitly labeled last valid result. History uses immutable snapshot
  documents plus a 32-entry manifest, snapshot-first publication, manifest-first
  eviction, and an independent content digest checked on replay. Recording
  failure degrades history without hiding a valid live result; missing or mutated
  historical payloads fail closed. Saved views contain only the
  definition and row-anchor identities; the versioned universe and rank
  evidence remain derived feature state. See `docs/screening.md`.
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
`CsvPortfolioRepository`, which owns the last successfully validated user
positions import. The repository emits a versioned, typed snapshot with
exact money, fixed-scale quantities, anonymized account identities,
per-currency reconciliation, and explicit unpriced holdings. Only the selected
CSV path is persisted through the Portfolio-owned state port; raw portfolio
contents remain at the user-selected location. Demo news and portfolio data are
wired only by `demo_app` for deterministic tests and gallery captures. Network
and filesystem formats remain outside the feature packages.

`PortfolioRiskQuery` is an infrastructure translation seam: it reads
Portfolio's public versioned snapshot and maps it into Risk-owned inputs. The
Risk feature imports neither Portfolio domain types nor its repository. Its
pure calculator first reconciles priced NAV, cash, and missing-price counts per
currency, then derives concentration and an explicit non-cash shock with exact
minor-unit arithmetic. This is the boundary downstream pricing and factor
engines can replace without gaining access to Portfolio storage.

`PortfolioAssistantContextQuery` applies the same consumer-owned contract to
the AI surface. Assistant owns a deliberately presentation-ready context model
and the narrow `AssistantContextQuery` port. The infrastructure translator reads
Portfolio snapshots, formats the permitted fields, and discards repository
capabilities before a request crosses into Assistant. Provider adapters can
therefore serialize portfolio context without importing Portfolio types, and
the model cannot use the Assistant request as an accidental path back to
portfolio storage.

Portfolio contribution is also a pure calculation boundary. Callers supply one
verified period of security-level beginning values, ending values, and
end-of-period external flows plus optional benchmark beginning and ending
values. The calculator reconciles exact gain/loss and additive contribution per
currency, returns active contribution when benchmark coverage is complete, and
reports centibasis-point rounding residuals rather than hiding them. It does not
read Portfolio storage, join unrelated snapshots, or convert currencies. The
bounded contribution CSV adapter validates a single period, anonymizes account
identifiers, requires complete benchmark evidence, and constructs this typed
input; the Portfolio workspace renders the resulting drill-down without taking
ownership of filesystem persistence.

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

Spreadsheet is the dense-grid action reference. A presentation-owned geometry
module partitions formula bar, grid, worksheet tabs, wrapped workflow controls,
and status, then supplies the same cell and control rectangles to rendering,
pointer hit testing, spatial focus, and follow hints. The registry emits the
selected cell, formula bar, tabs, and controls before the remainder of the visible
grid so bounded consumers cannot starve primary actions. Cell IDs include the
active worksheet digest and address; tab IDs include index and name digest.
Activation rechecks identity, viewport membership, edit state, clipboard source,
history availability, and selected-cell type before mutating or emitting a kernel
intent. Inline editing remains feature-owned and commits before a pointer routes
to a different target.

Spreadsheet also owns its saved-view field schema. A view records the workbook
document ID, worksheet name plus ordinal fallback, selected cell, and first
visible row and column. Restoration resolves the workbook through the
Spreadsheet-owned repository before applying presentation state; unavailable
documents and renamed tabs produce a degraded report while valid fields still
recover. Workbook cells remain in the versioned workbook document, while draft
edits, clipboard contents, and undo/redo stacks remain ephemeral. This keeps a
layout reference small and prevents restoring a view from overwriting newer
financial work.

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
   hotkey, and command aliases. Publish visible rows, tabs, and controls through
   `actions` and handle their opaque IDs through `activate_action` when the
   feature has local destinations that should participate in follow/spatial
   routing. Mark at most one natural restoration target as `preferred`; the
   shell chooses the first valid preference, then navigates non-wrapping action
   rectangles with lane-first distance and stable geometry/ID tie-breakers.
3. Add an infrastructure adapter for the feature-owned port.
4. Register the workspace in `bootstrap.rs`.
5. Run `cargo test --test architecture_boundaries`; CI rejects production
   feature-to-adapter imports, cross-feature imports, shell/rendering
   dependencies in domain and port layers, and dependency inversions in
   `foundation` or shared `ui`.

No root router match, shared screen state, or central data trait needs to be
edited. The registry validates duplicate IDs and hotkeys at startup. Follow
hints remain shell-owned, but their feature targets and activation semantics do
not leak into the shell; Portfolio's tabs, security rows, and reload control are
the initial leaf reference implementation. Desk is the composition reference:
it registers visible pane headers and namespaces child-workspace actions without
rewriting their body rectangles or activation semantics. Its bounded split
geometry is app-kernel presentation state rather than Monitor, Chart, or News
domain state. The same geometry snapshot drives pane rendering, pointer targets,
header resize controls, child body rectangles, follow badges, spatial focus
styling, arrow routing, activation revalidation, saved-view recovery, resize
recovery, and async-state recovery, so the shell never keeps a second geometry
model. Security Research is the richer table/action
reference: one shared layout supplies its mouse targets, research tabs, chart,
Form 4 and filing rows, peer links, refresh action, and responsive follow/spatial
rectangles. Activation rechecks the active view plus the symbol or accession
embedded in each opaque action ID before dispatching or opening a document.
Charting is the dense-control reference: one flow layout packs only complete
controls into the available three-row footer and supplies rendering, pointer hit
testing, spatial focus, and follow hints from those exact rectangles. Period IDs
encode the requested period and are parsed back into Charting-owned domain
values; stateful controls recheck comparison and inspection availability before
activation. The active period is the deterministic restoration target, while a
separate header action preserves the existing click-to-refresh surface.
News is the modal/action reference. Its shared responsive layout owns filter
controls, headline rows, selected-story commands, detail links, calendar events,
and refresh rectangles for rendering, pointer hit testing, spatial focus, and
follow hints. Opaque story action IDs include a stable content digest and are
revalidated against the live snapshot before activation. When a workspace reports
`is_modal_active`, the application kernel routes all keyboard and pointer input to
that workspace, excludes shell and navigation destinations from follow hints, and
publishes only the feature's modal actions. The reader therefore exposes close and
publisher-page destinations without allowing workspace hotkeys, navigation clicks,
or stale story identities to escape the modal.
Overview is the composed-dashboard reference. Its geometry module owns gallery
periods and cards as well as live holding rows, headline rows, and the common
Portfolio/Risk/News/refresh strip. Rendering, pointer hit testing, spatial focus,
and follow hints consume that one layout. Cross-context navigation remains an
application intent: Overview neither imports nor mutates Portfolio, Risk, News,
or Security internals. Activating a live row reloads the already-composed read
model and rechecks its row index plus symbol or content digest, so an asynchronous
portfolio/news replacement cannot route a stale dashboard target.

Alerts is the durable-mutation action reference. A feature-owned geometry module
partitions the register, audit panel, and complete footer controls and supplies
the exact rectangles used by rendering, pointer hit testing, spatial focus, and
follow hints. Row action IDs include both the visible index and the full domain
rule ID; activation rechecks both against the current register before changing
selection. Stateful controls re-evaluate lifecycle and trigger state immediately
before mutation, so an acknowledgement that became unavailable is rejected.
Security promotion emits an application intent only after re-reading the selected
rule, preserving bounded-context ownership while preventing stale-symbol routing.

## Capability completion evidence

`docs/openterminalui-parity-ledger.json` is the source of capability status.
Marking an `OTUI-*` item `covered` creates two fail-closed obligations. First,
`docs/capability-evidence.json` must resolve the capability to the live Help
catalog plus real implementation, semantic-golden, contract-test, data-source,
and performance evidence. Second, `docs/capability-gallery.json` must provide
exactly one loading, populated, empty, delayed, stale, denied, partial, and
failed frame. `tests/capability_gallery.rs` renders every frame at 80 × 24,
120 × 36, and 160 × 48 and locks the symbols, colors, and modifiers into a
reviewed aggregate hash.

The gallery distinguishes `rendered` from `not_applicable`. A synchronous local
capability must not manufacture provider loading, delay, staleness, permission,
or partial-data behavior merely to fill the matrix. Instead it renders a
high-contrast, non-color-only `NOT APPLICABLE` frame with a substantive reason.
Every covered capability still needs a real populated reference. Adding a new
covered ledger item or removing any state therefore fails CI until its evidence
and all three responsive frames are reviewed together.

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

The command bar also owns a consumer-side `CommandInference` port. Exact
built-ins and workspace aliases always resolve synchronously first. Only an
unmatched input is sent to the configured AI gateway on a background worker,
along with the active workspace and current command catalog. That inference
turn receives no portfolio snapshot and must select exactly one command through
the existing run-command tool. The shell then parses the result and requires
its function to resolve through `WorkspaceRegistry`; prose, multiple actions,
newlines, oversized output, and invented functions fail closed.

```text
typed input -> exact registry miss -> background AI command inference
            -> one RunCommand action -> exact registry validation -> dispatch
```

The allowed mutations are workspace focus, navigation promotion, existing
command dispatch, and default-order restoration. Unknown tools, malformed
arguments, unknown targets, and unknown commands are rejected. The Codex
adapter reuses the CLI's authentication cache without reading or copying its
tokens; API-provider credentials are read from the process environment. No
credential is stored in feature state, conversation history, logs, or model
context.

Alerts own a separate `AlertStateStore` port. The interactive composition root
wires it to the crash-safe feature-document repository, while the workspace
coalesces complete rule-register snapshots through a bounded background writer.
The persisted state includes lifecycle, debounce, last observation, processed
evaluation IDs, trigger/acknowledgement state, and bounded audit history; the
live quote adapter remains an independent read boundary. This keeps restart
idempotency inside Alerts without coupling the context to filesystem or market
data formats.

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
- preserve the completed typed saved-view identity and transient-data boundaries
  as future workspace schemas stabilize, and integrate saved objects into the
  unified discovery surface.

The current boundary is deliberately a modular monolith. It gives strong
ownership and test seams without paying the operational cost of services or a
large multi-crate graph before those costs are warranted.

These boundaries are executable, not solely diagrammed.
`tests/architecture_boundaries.rs` scans production Rust source (excluding
test-only modules) and fails on the dependency directions above. Unit tests may
compose deterministic concrete adapters, but production feature code must reach
them only through a feature-owned port wired in `bootstrap.rs`. Cross-context
reads require a consumer-owned DTO and an infrastructure translator, as shown
by Portfolio-to-Risk and Portfolio-to-Assistant. This gives a growing modular
monolith a cheap extraction test: a context should remain movable without
bringing another context's repository or domain graph with it.
