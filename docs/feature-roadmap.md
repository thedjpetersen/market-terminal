# Feature Roadmap

This roadmap turns the product surface of a professional financial workstation
into independently owned bounded contexts. It is a clean-room plan: the goal is
comparable workflow coverage and information density, not compatibility with a
proprietary terminal's code, command names, visual assets, datasets, or network
protocols.

The roadmap assumes the modular-monolith rules in
[`architecture.md`](architecture.md): each feature owns its domain model and
ports, the application kernel knows only the `Workspace` contract, shared UI is
domain-free, and `bootstrap.rs` is the composition root. A bounded context may
move to its own crate later without changing its public vocabulary.

## Delivery status

- **Complete:** workspace registry, exact command aliases, input capture,
  package-by-feature boundaries, feature-owned ports, and the native spreadsheet
  foundation.
- **Complete:** first AI command-plane slice with an OpenRouter adapter,
  non-blocking requests, conversation workspace, and a closed set of validated
  UI intents for focus, navigation promotion, command dispatch, and layout reset.
- **Complete:** first Instrument Master/Search slice with canonical IDs, ranked
  symbol and company lookup, typed instrument kinds, and navigation from search
  results into Security Research.
- **Complete:** bounded kernel event bus with typed envelopes, topic filters,
  queue limits, cancellation, lag metrics, and a foundation-owned
  `InstrumentId`.
- **Complete:** first Market Data/Watchlist slice with typed quote/history
  contracts, explicit quality and entitlement states, configurable monitor
  columns and sorting, security drill-through, and deterministic replay.
- **Complete:** Charting slice with price/volume series, width-aware OHLC
  candlesticks, line comparisons, periods, normalization, SMA/EMA, Wilder RSI,
  inspection, and deterministic provider history.
- **Complete:** Spreadsheet multi-sheet management, atomic undo/redo, multi-cell
  edits, and deterministic formula-preserving CSV import/export.
- **Complete:** first Alerts slice with price/move rules, deterministic replay,
  consecutive-match debouncing, idempotency, enable/disable, acknowledgement,
  audit state, and explicitly simulated local delivery.
- **Complete:** bounded typed commands with quoting, escaping, structured long
  options, strict alias resolution, and size/token limits.
- **Complete:** crash-safe local persistence contracts and adapter with schema
  migration, previous-valid-generation recovery, bounded payloads, safe feature
  document identities, and durable shell layout/recent-command restoration.
- **Complete:** incremental spreadsheet recalculation with dependency and
  reverse-dependency indexes plus comparison, conditional, logical, text,
  counting, rounding, and exact lookup functions.
- **Complete:** first resilient quote-streaming slice with bounded per-instrument
  coalescing, cancellation, drop metrics, provenance, freshness/LKG caching,
  retry/rate-limit policies, and monitor fallback behavior.
- **Complete:** selectable no-key Yahoo Finance delayed chart adapter with
  bounded responses/cache and explicit unofficial-interface attribution,
  alongside documented Alpha Vantage and Alpaca adapters.
- **Complete:** optional official Finnhub real-time US quote adapter with
  header-only credentials and explicitly session-derived, non-provider chart
  marks when premium candles are unavailable.
- **Complete:** expanded Security Research and News/Events workflows with
  canonical instrument links, financial/estimate/ownership/filing/peer views,
  raw SEC Form 4 non-derivative transactions with official filing links,
  loaded-sample log-value/weekly activity visualization,
  filters, read/bookmark state, story detail, economic calendar, and validated
  cross-workspace intents.
- **Complete:** workbook-scoped spreadsheet evaluation with qualified and quoted
  cross-sheet references, cross-sheet cycle detection, mixed absolute axes, and
  atomic translated copy, paste, and directional fill controls.
- **Complete:** toggleable AI drawer with immediate input focus, a warm reusable
  Codex app-server worker, and bounded, transient, on-demand News article
  reading with an explicit publisher-page fallback.
- **Complete:** Stage 0 platform foundation for the modular-monolith baseline:
  production-published kernel events, opt-in structured tracing, crash-safe
  feature documents, deterministic semantic frame goldens at 80 x 24,
  120 x 36, and 160 x 48, and a CI-enforced update latency budget.
- **Complete:** Stage 1 research desktop and Spreadsheet MVP: 27 pure functions,
  composable `PX_LAST`, `PX_CHANGE`, `HISTORY`, and `FUNDAMENTAL`, typed async
  states and provenance, five-sheet/10,000-cell performance coverage, durable
  workbook save/load/autosave, and intent-based selection exchange with FIND,
  MON, SEC, CHART, and NEWS.
- **Complete:** official production Spreadsheet adapters for scalar Alpha
  Vantage daily history and SEC EDGAR annual Company Facts, composed behind the
  Spreadsheet-owned batch port with explicit provenance, delay, unavailable,
  and entitlement states.
- **Next:** the Stage 2 portfolio, risk, screening, and restart-safe alerts
  program. Provider availability extends the deployment surface; deterministic
  fixtures remain the Stage 1 acceptance baseline and opt-in live contracts
  verify real provider behavior.

## Product principles

- **Keyboard first.** Every workflow has a short, documented command and can be
  completed without a pointer.
- **One canonical instrument identity.** Quotes, news, charts, portfolio rows,
  alerts, and sheets refer to an internal `InstrumentId`, never a provider's raw
  ticker alone.
- **Feature-owned ports.** No universal data service and no shared bag of market
  structs. A context requests only the capabilities it needs.
- **Read before write.** Data exploration and paper workflows precede brokerage,
  messaging, publishing, or other externally consequential actions.
- **Deterministic core.** Formulas, analytics, and portfolio calculations are
  pure and replayable. Clocks, data feeds, storage, and execution are adapters.
- **Entitlements are domain inputs.** A view must distinguish delayed, stale,
  derived, unavailable, and permission-denied data rather than silently filling
  gaps.

## Context map

| Bounded context | Owns | Depends on contracts from | Publishes |
| --- | --- | --- | --- |
| Instrument Master | symbology, listings, venues, currencies, corporate-action identity | reference-data adapters | `InstrumentResolved`, `InstrumentChanged` |
| Market Data | quote snapshots, time series, depth, rates, FX, commodities | Instrument Master, feed adapters | `QuoteUpdated`, `BarClosed`, `MarketStatusChanged` |
| Spreadsheet | workbook, sheet, cell, formula AST, dependency graph, recalculation | Instrument Master and query ports selected by functions | `CellChanged`, `WorkbookRecalculated` |
| Search & Discovery | command palette, instrument lookup, function catalog, recent items | Instrument Master, feature catalog | navigation intents |
| Watchlists & Monitors | user-defined lists, columns, sorting, color rules | Instrument Master, Market Data, persistence | `WatchlistChanged` |
| Charting | chart specifications, studies, comparisons, annotations | Market Data | saved chart specs |
| Security Research | profile, fundamentals, estimates, ownership, filings, relative value | Instrument Master, fundamental/filing adapters | research navigation intents |
| News & Events | stories, topics, transcripts, calendars, event links | Instrument Master, news/event adapters | `StoryArrived`, `EventUpdated` |
| Portfolio & Risk | books, positions, lots, P&L, attribution, exposures, scenarios | Instrument Master, Market Data, pricing/risk engines | `PositionChanged`, `RiskCalculated` |
| Screening & Analytics | universes, filters, rankings, comparables, reusable studies | Instrument Master, Market Data, fundamentals | saved screens and result sets |
| Alerts | alert rules, schedules, delivery state, acknowledgement | events exposed by other contexts, notification adapters | `AlertTriggered` |
| Trading & Orders | order intent, validation, routing, fills, allocations | Instrument Master, Market Data, Portfolio, broker adapters | `OrderStateChanged`, `FillReceived` |
| Collaboration & Export | notes, snapshots, reports, CSV/JSON export | read models from other contexts | exported artifact metadata |
| Identity, Settings & Entitlements | user profile, layouts, permissions, provider terms | auth, secret-store, persistence adapters | `EntitlementsChanged` |

Dependencies shown here are semantic dependencies, not permission to import
another feature's private domain module. Cross-context reads go through explicit
ports or stable event/read-model contracts. The event bus belongs to the app
kernel; event schemas belong to their publishing context.

## Command and workflow model

Commands are product API, not loose substring aliases. Before Stage 1 closes,
replace ambiguous matching with a parser that produces a typed `Command`:

```text
<function> [<subject>] [<qualifier>...] [--option <value>]
```

Recommended public vocabulary (subject to collision checks in the registry):

| Command | Result |
| --- | --- |
| `GO` | Overview workspace |
| `FIND <query>` | Resolve an instrument, function, list, or saved object |
| `SHEET [workbook]` | Open or create a workbook |
| `MON <watchlist>` | Open a live market monitor |
| `SEC <instrument>` | Open security research |
| `CHART <instrument> [COMPARE <instrument>...]` | Open charting |
| `NEWS [instrument|topic]` | Open filtered news |
| `PORT <portfolio>` | Open positions and performance |
| `RISK <portfolio>` | Open exposure and scenario analysis |
| `SCREEN <saved-screen>` | Run an instrument screen |
| `ALERT <instrument|expression>` | Create or inspect an alert |
| `ORDER <instrument>` | Open a staged order ticket; never submits immediately |

A context can define subcommands, but registration must reject duplicate exact
aliases and the parser must preserve the subject as structured input. Typical
workflow composition should be possible through intents rather than feature
imports. For example:

```text
FIND MSFT -> SEC -> CHART -> add comparison -> export snapshot
MON MACRO -> open selected instrument -> NEWS -> create ALERT
PORT LONG_ONLY -> RISK -> open sector contribution -> SHEET
SHEET valuation -> select instrument column -> SEC
```

The command parser, navigation intents, selection context, undo/redo actions,
and durable workspace layout are kernel capabilities. Quote lookup, formulas,
portfolio calculations, and news filtering remain feature capabilities.

## Stage 0 — Platform foundation

**Outcome:** make parallel feature development safe before expanding the
surface area.

### Work

- Stabilize `Workspace`, `WorkspaceDescriptor`, typed commands, navigation
  intents, focus handling, and feature lifecycle hooks.
- Introduce an internal event bus with bounded queues, cancellation, and
  observable lag; do not expose infrastructure feed types directly.
- Define shared value objects in narrowly scoped foundation modules:
  `InstrumentId`, `Currency`, `Money`, `Price`, `Quantity`, `UtcTimestamp`, and
  `DataQuality`. Avoid a generic `common::models` package.
- Split `DemoData` behind reference, quote, history, fundamentals, news, and
  portfolio adapters. Add deterministic fixtures and fake clocks.
- Add persistence ports for settings, layouts, recent commands, and feature
  documents, with an initial local adapter.
- Add structured logs, metrics, tracing spans, error taxonomy, retries, cache
  policy, rate limits, and cancellation at adapter boundaries.
- Establish contract tests, golden terminal snapshots at several sizes, and
  performance budgets for render and update loops.

### Exit criteria

- Adding a workspace touches its feature package, an adapter if needed, and
  `bootstrap.rs`; it does not add branches to app input or rendering.
- Duplicate commands, hotkeys, and workspace IDs fail fast with useful errors.
- Slow or failed data adapters cannot freeze input or corrupt the last known
  good view.
- A deterministic integration test can replay quotes and user keys to the same
  rendered buffer on every run.

**Status: complete.** The current baseline enforces these criteria in CI through
typed command/registry tests, bounded worker and replay tests, semantic terminal
goldens, and the release-mode performance gate.

## Stage 1 — Research desktop and Spreadsheet

**Outcome:** a coherent daily research loop built from reusable primitives,
with Spreadsheet as the user-composable surface.

### Bounded contexts and dependencies

1. **Instrument Master + Search & Discovery**
   - Exact and fuzzy lookup, venue/currency disambiguation, recent instruments,
     function discovery, and provider-symbol mapping.
   - Foundation for every instrument-centered context.
2. **Market Data + Watchlists**
   - Snapshot and replayed streaming quotes, market status, time series,
     configurable monitor columns, sorting, and stale-data markers.
   - Depends on Instrument Master; provides query/event contracts consumed by
     Spreadsheet, Charting, Portfolio, and Alerts.
3. **Spreadsheet**
   - Multiple sheets, typed cells, formulas, ranges, references, dependency
     graph, cycle errors, incremental recalculation, copy/paste, fill, undo/redo,
     CSV import/export, and local save/load.
   - Initial pure functions: arithmetic, comparison, aggregation, conditional,
     text/date functions, and lookup functions over local ranges.
   - Initial financial functions consume feature ports, for example
     `PX_LAST(instrument)`, `PX_CHANGE(instrument, period)`,
     `HISTORY(instrument, field, start, end)`, and
     `FUNDAMENTAL(instrument, field, period)`. Names are project-owned and may be
     revised before a compatibility promise is made.
   - Async cells show `loading`, `stale`, `permission denied`, or a typed error;
     external results are cached with provenance and observation time.
4. **Charting + Security Research**
   - Price/volume charts, comparisons, moving averages, normalized performance,
     profile, key fundamentals, estimates, peers, and linked news.
   - Depends on Instrument Master plus dedicated market/fundamental ports.
5. **News & Events**
   - Headline list/detail, instrument/topic filtering, source and timestamp,
     event calendar, unread state, and links back to instruments.

### Why Spreadsheet is Stage 1

Spreadsheet is not an accessory; it is the first composition layer. A fixed
terminal screen can answer only the question anticipated by its author, while a
sheet lets users join quotes, history, fundamentals, and their own assumptions
without waiting for a new workspace. Building it early also forces the platform
to solve the hardest cross-cutting requirements while the codebase is still
small: canonical instrument identity, typed values, asynchronous data, caching,
provenance, deterministic recalculation, persistence, entitlements, export, and
performance under fan-out. Those contracts then make every later analytical
feature cheaper and safer. Delaying the sheet would invite one-off grids and
incompatible calculation logic across features.

### Spreadsheet workflow

```text
SHEET -> create workbook -> enter/import instruments
      -> add formulas and financial functions
      -> inspect cell provenance or typed errors
      -> chart selected range or open selected instrument
      -> save workbook -> export CSV
```

### MVP acceptance criteria

- `SHEET` opens a keyboard-navigable 100 x 26 grid at 80 x 24 and larger
  terminals; editing, selection, scrolling, and the command bar remain usable.
- A workbook supports at least five sheets and 10,000 populated cells without
  input latency above 50 ms at the 95th percentile on the supported baseline.
- Formula parsing uses a documented grammar and produces a typed AST. Literal,
  range, cross-sheet, relative, and absolute references are covered by tests.
- The engine detects direct and indirect cycles, recalculates only affected
  dependents, and produces deterministic results for a fixed input snapshot and
  clock.
- At least 20 pure functions and the four initial financial functions work with
  deterministic fixtures. A failed lookup affects its dependent cells, not the
  workbook process.
- Every external-data cell exposes provider/source, instrument, field,
  observation time, receive time, quality/delay state, and entitlement outcome.
- Copy/paste, range fill, undo/redo, CSV import/export, local save/load, and
  crash-safe recovery have integration tests.
- `FIND`, `MON`, `SEC`, `CHART`, and `NEWS` accept a canonical selection from a
  sheet, and results can be inserted back into a sheet without feature packages
  importing one another.
- Golden snapshots cover grid, formula editor, help, errors, loading, stale
  values, narrow terminals, and high-density layouts. No glow effects or
  browser assets are introduced.

**Status: complete.** The supported MVP and formula grammar are documented in
[`spreadsheet.md`](spreadsheet.md). Official Alpha Vantage daily `HISTORY` and
SEC EDGAR annual `FUNDAMENTAL` adapters now implement those unchanged workbook
and formula contracts; other providers retain explicit entitlement or
unavailable outcomes.

## Stage 2 — Portfolio, risk, screening, and alerts

**Outcome:** turn research data into repeatable decision workflows.

### Bounded contexts and dependencies

- **Portfolio & Performance:** books, accounts, positions, tax lots, cash,
  transactions, benchmarks, time-weighted return, allocation, contribution,
  attribution, and drill-down. Depends on Instrument Master and Market Data.
- **Risk:** factor/sector/country/currency exposure, concentration, drawdown,
  historical scenarios, shock scenarios, and contribution to risk. Consumes a
  versioned Portfolio snapshot and pricing inputs; it must not reach into
  Portfolio storage.
- **Screening & Relative Value:** saved universes, typed filters, ranks,
  comparables, column expressions, and export to Spreadsheet/Watchlists.
- **Alerts:** price, percent move, news/topic, portfolio threshold, calendar,
  and spreadsheet-expression rules with acknowledgement and local delivery.
- **Economics & Calendars:** macro releases, central-bank calendar, rates curves,
  and surprise history through properly licensed adapters.

### Commands and workflows

```text
PORT core -> select position -> contribution -> SEC
RISK core -> SHOCK rates +100bp -> send result table to SHEET
SCREEN quality_value -> rank -> add results to MON candidates
ALERT core.drawdown > 8% -> preview -> enable
```

### Exit criteria

- Portfolio totals reconcile to transaction and cash fixtures, including
  multiple currencies, splits, dividends, and missing prices.
- Performance and risk outputs include valuation time, input version, currency,
  methodology, and missing-data disclosures.
- A saved screen produces the same ordered result from the same versioned input;
  users can promote results to a watchlist or spreadsheet range.
- Alerts are idempotent, debounced, auditable, restart-safe, and visibly marked
  as simulated/local until an external notification channel is configured.

## Stage 3 — Advanced analytics and paper workflows

**Outcome:** cover deeper professional analysis without prematurely assuming
regulated execution responsibilities.

### Bounded contexts and dependencies

- **Fixed Income:** instruments, calendars, cash flows, yield/price, duration,
  convexity, curves, spreads, and scenario ladders.
- **Derivatives:** option chains, volatility surface, Greeks, payoff/scenario
  analysis, and strategy comparison. Start with delayed/reference inputs.
- **FX & Commodities:** spot/forward curves, carry, crosses, futures curves,
  rolls, and curve spreads.
- **Corporate Actions & Filings:** event normalization, document metadata,
  extracted facts with source links, and portfolio impact previews.
- **Paper Orders:** staged tickets, validation, simulated fills, blotter,
  allocations, and pre/post-trade audit trail. It depends on explicit interfaces
  from Instrument Master, Market Data, Portfolio, Identity, and entitlements.
- **Reporting & Collaboration:** saved layouts, notes, printable reports,
  snapshots, and reproducible research packages.

### Commands and workflows

```text
BOND <instrument> -> cash flows -> curve shock -> SHEET
OPTIONS <instrument> -> choose expiry -> strategy -> scenario
ORDER <instrument> -> validate -> preview -> PAPER SUBMIT -> blotter
REPORT <portfolio> -> choose sections -> preview -> export
```

### Exit criteria

- Pricing engines have independent reference cases, property tests, units, day
  count/calendar conventions, and model/version metadata.
- Paper orders cannot be mistaken for live orders in UI, storage, logs, or
  exports; submission requires an explicit preview and confirmation.
- Research reports retain input timestamps, versions, methodologies, and links
  to permitted source material.

## Stage 4 — Extensible workstation and controlled live integrations

**Outcome:** enable an ecosystem and, only where justified, production-grade
external actions.

- Split contexts into crates based on compile time and ownership evidence, not
  fashion. Publish stable SDK types separately from internal domain types.
- Add a capability-scoped plug-in manifest, versioned command API, sandboxed
  calculation extensions, resource budgets, and signed packages.
- Add provider routing, health-aware failover, entitlement-aware caching, and
  offline/replay modes.
- Add multi-user workspace sync and collaboration through explicit document
  ownership and conflict-resolution semantics.
- Consider live broker adapters only after legal review, threat modeling,
  secrets management, reconciliation, kill switches, approvals, surveillance,
  and immutable audit retention are implemented.
- Any automated or AI-assisted research ships with source citations,
  uncertainty, permission filtering, prompt-injection defenses, and a clear
  boundary between summarization and investment action.

Stage 4 is complete for a given integration only when operational runbooks,
failure drills, migration/rollback, support ownership, data retention, and
contract tests against the provider's sandbox are in place.

## Cross-stage technical gates

Every context must meet these gates before being considered production-ready:

1. **Boundary:** private domain types do not leak; ports live with the consumer;
   adapters are selected only at the composition root.
2. **Correctness:** domain invariants, units, rounding, calendars, timestamps,
   and error states are explicit and tested.
3. **Performance:** the UI thread never performs I/O; streams are bounded and
   coalesced; renders, memory, and recalculation have budgets.
4. **Resilience:** cancellation, retries with jitter, circuit breaking, offline
   behavior, cache freshness, and last-known-good states are observable.
5. **Security:** secrets never enter feature state or logs; inputs are bounded;
   persisted artifacts are versioned and migratable.
6. **Accessibility:** commands have help, focus is visible, color is not the sole
   status signal, and layouts remain usable on supported terminal sizes.
7. **Auditability:** derived values identify inputs and methodology; external
   writes identify actor, intent, confirmation, adapter response, and time.

## Legal and data boundaries

This project should maintain a written source register for every adapter and
fixture. The following are release blockers, not documentation niceties:

- Do not copy Bloomberg source code, command mnemonics as a compatibility set,
  screenshots, fonts, icons, proprietary color specifications, help text,
  screen layouts pixel-for-pixel, protocol behavior, or scraped datasets.
- Do not use Bloomberg trademarks in the product name or imply affiliation,
  certification, data equivalence, or drop-in compatibility. “Inspired by a
  professional financial terminal” is the safer product description.
- Market prices, fundamentals, estimates, identifiers, news, transcripts,
  filings, research, and exchange data each have distinct licenses. An API being
  technically reachable does not grant display, caching, derivation,
  redistribution, benchmarking, or open-source fixture rights.
- Adapters must encode attribution, display delay, cache/retention limits,
  geography, user entitlements, derived-data restrictions, and redistribution
  permissions. The UI must not erase these distinctions.
- Use synthetic or explicitly redistributable data in tests, screenshots,
  examples, releases, and CI. Keep licensed production data out of the repo,
  telemetry, bug reports, and golden snapshots.
- Respect news copyright: store/display only what the license permits, preserve
  source attribution and links, and avoid bundling article bodies in fixtures.
- Live trading, personalized recommendations, communications retention, market
  data display, and user analytics may trigger broker-dealer, investment-adviser,
  exchange, privacy, sanctions, accessibility, and records obligations. Obtain
  qualified legal review before enabling them for real users or jurisdictions.
- Exports and spreadsheet formulas must carry data-quality and as-of metadata so
  delayed or stale values are not presented as current facts. Never describe
  demo, delayed, estimated, or paper results as executable market prices.
- Provider credentials belong in an OS secret store or injected runtime secret,
  never workbook files, configuration committed to Git, logs, or diagnostics.

## Delivery order and team seams

Within a stage, deliver vertical slices rather than completing all domain models
before UI. A slice includes domain behavior, consumer-owned port, fake adapter,
workspace interaction, observability, tests, and documentation. The preferred
order is:

1. Instrument identity and typed command/navigation foundations.
2. Deterministic quote/history fixtures and streaming/replay infrastructure.
3. Spreadsheet kernel, then financial functions through explicit ports.
4. Watchlist, security, chart, and news slices that reuse the same identity and
   data-quality contracts.
5. Persistence, export, golden snapshots, and performance hardening before
   Stage 2 begins.

This order creates stable seams for parallel teams: one can own the spreadsheet
calculation engine, another terminal grid interaction, another instrument/data
contracts and adapters, and another research workspaces. They integrate through
versioned ports and fixtures rather than by editing shared feature internals.
