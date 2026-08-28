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
- **Complete:** AI-backed inference for unmatched command-bar text, with exact
  commands taking precedence, background execution, command-only structured
  output, no portfolio context, and a second exact-registry validation before
  dispatch.
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
- **Complete:** shell-level keyboard accessibility with an explicit panel-focus
  state: `Esc` lifts focus to a preferred feature action, bare arrows navigate
  registered rectangles with deterministic lane-first, non-wrapping selection,
  and `Enter` revalidates and activates the target. Workspaces without feature
  actions fall back to workspace traversal. Vimium-style `F`
  follow hints assign prefix-free one- or two-letter labels to visible workspace
  routes and shell actions, with bounded prefix matching and cancellation.
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
- **Complete:** first Stage 2 Portfolio ingestion slice with exact money and
  fixed-scale quantities, anonymized account separation, honest per-currency
  reconciliation, retained unpriced positions, deterministic input versions,
  methodology/disclosures, and crash-safe import-path restoration.
- **Complete:** first Stage 2 activity-ledger slice with explicit cash/broker CSV
  import, exact per-currency inflow/outflow/net/dividend/interest/fee
  reconciliation, retained non-cash splits, anonymized accounts, independent
  crash-safe restoration, clickable/Vim-navigable views, bounded AI access, and
  a contract test against an actual local cash export.
- **Complete:** restart-safe local Alert register with bounded rule and audit
  retention, full lifecycle/debounce/acknowledgement persistence, asynchronous
  crash-safe writes, and live-provider idempotency verification across restart.
- **Complete:** first Stage 2 Risk slice over the versioned Portfolio boundary,
  with exact per-currency reconciliation, concentration, an explicit non-cash
  shock, missing-price disclosures, clickable drill-through, and actual
  configured-portfolio render verification.
- **Complete:** first historical Risk package over Portfolio's independent dated
  valuation boundary: end-of-period flow-adjusted observations, actual-day
  annualized sample and EWMA volatility, wealth-index drawdown/recovery,
  historical and Gaussian VaR/CVaR, Sharpe/Sortino, and complete-benchmark beta,
  correlation, tracking error, and information ratio. Every result remains per
  currency and carries period, sample count, median interval, annualization,
  confidence, lambda, risk-free rate, input version, methodology, and low-sample
  or missing-benchmark disclosures.
- **Complete:** first Stage 2 Performance slice with bounded dated-valuation
  CSV import, exact flow-adjusted TWR, optional benchmark and active return,
  strict per-currency separation, versioned methodology/disclosures, and
  independent crash-safe import-path restoration.
- **Complete:** first Stage 2 tax-lot slice with bounded open-lot CSV import,
  strict acquired-date and positive-quantity validation, exact per-currency
  basis/value/unrealized-gain reconciliation, explicit unpriced and unknown-term
  states, anonymized accounts, Security drill-through, and independent
  crash-safe import-path restoration.
- **Complete:** first Stage 2 closed-lot slice with bounded broker CSV import,
  strict acquisition/disposal chronology, exact per-currency proceeds, basis,
  realized-gain and provider-term reconciliation, reported-gain validation,
  anonymized accounts, Security drill-through, and independent crash-safe
  import-path restoration.
- **Complete:** first Stage 2 order/fill slice with bounded broker execution
  CSV import, exact six-decimal quantities and prices, per-currency gross,
  fee, and signed-net reconciliation, UTC precision disclosures, anonymized
  accounts and orders, Security drill-through, and independent crash-safe
  import-path restoration. The workflow is strictly read-only.
- **Complete:** first Stage 2 contribution calculation slice as a pure
  Portfolio-owned boundary: exact single-period gain/loss, additive security
  contribution, optional benchmark and active contribution, strict per-currency
  reconciliation, typed metadata, complete-benchmark validation, and explicit
  centibasis-point rounding residuals.
- **Complete:** bounded contribution CSV import and terminal drill-down with
  strict single-period evidence, complete paired benchmarks, account
  anonymization, independent crash-safe path persistence, Security routing,
  methodology/version disclosure, and explicit rounding residuals.
- **Complete:** pure multi-period Portfolio attribution boundary using
  order-dependent Frongello linking over verified contiguous periods, with
  geometric portfolio and benchmark returns, changing security membership,
  strict value/currency/benchmark continuity, and explicit centibasis-point
  linking residuals.
- **Complete:** bounded multi-period attribution CSV import and terminal
  drill-down with stable history-wide account anonymization, ordered-period and
  aggregate valuation continuity, independent crash-safe path persistence,
  Security routing, and complete methodology/version/residual disclosure.
- **Complete:** first P0 feature-action routing slice with bounded stable action
  IDs, duplicate/disabled/off-screen rejection, viewport-aware one- or two-letter
  follow hints, activation revalidation, and Portfolio tabs, security-capable
  rows, and reload controls as the initial leaf adopter.
- **Complete:** first composed P0 action-routing slice for Desk: visible Monitor,
  Chart, and News pane headers support spatial focus and follow hints, hidden
  responsive panes are excluded, and nested child actions retain their rendered
  rectangles behind collision-free pane namespaces.
- **Complete:** Monitor action routing from a shared render/mouse/focus geometry:
  visible instrument rows open Security, sort field and direction are distinct,
  columns and refresh are addressable, narrow controls are viewport-clipped, and
  every action composes through Desk with stale identity revalidation.
- **Complete:** Security Research action routing from a single render/mouse/focus
  layout: responsive research tabs, the live chart, Form 4 source rows,
  regulatory filing links, peer-security links, and refresh/retry states all
  participate in spatial focus and follow hints. Activations fail closed when
  the current view, symbol, row index, accession, or peer identity has changed,
  and an application-level test routes real follow labels through Security into
  Chart.
- **Complete:** Chart action routing from a single responsive control registry:
  direct `1D`/`1M`/`6M`/`YTD`/`1Y`/`5Y` destinations, normalization, moving
  averages, SMA/EMA, RSI, volume, comparisons, bidirectional inspection, latest,
  chart and line modes, Spreadsheet promotion, and both footer/header refresh
  surfaces share render, mouse, spatial-focus, and follow-hint geometry. The
  registry packs only whole controls, prefers the active period, excludes
  unavailable inspection directions, and revalidates parsed periods and mutable
  comparison/cursor state before activation. A shell-level test selects a period
  and normalization through real generated follow labels.
- **Complete:** News action and modal routing from one responsive geometry:
  filters, headline rows, selected-story operations, detail links, calendar
  events, and refresh participate in mouse, spatial-focus, and follow navigation.
  Story identities are revalidated across asynchronous feed replacement. The
  full-screen reader traps keyboard and pointer input, limits follow hints to
  close and available publisher actions, handles a removed selected story, and
  has semantic goldens at all three supported terminal sizes.
- **Complete:** source-derived OpenTerminalUI capability ledger pinned to an
  exact upstream tree, with stable `OTUI-*` IDs, implementation-maturity labels,
  repository-relative evidence, owners, priorities, gaps, acceptance-test IDs,
  and a CI-executed schema/invariant test.
- **Complete:** executable modular-monolith dependency enforcement. CI scans
  production feature source and rejects direct infrastructure imports,
  cross-bounded-context imports, domain/port dependencies on shell or rendering,
  and dependency inversions in Foundation or shared UI. Assistant now owns its
  context DTO and query port; the composition root supplies a
  Portfolio-to-Assistant infrastructure translator instead of exposing
  Portfolio domain types or repository access. Watchlist also consumes the
  foundation-owned canonical `InstrumentId` directly rather than Market Data's
  compatibility alias.
- **Next:** execute the OpenTerminalUI parity track below, beginning with
  shell/workspace completion, then finishing the remaining
  Stage 2 risk, screening, and alert-rule families. Provider availability
  extends the deployment surface; deterministic fixtures remain the acceptance
  baseline and opt-in live contracts verify real provider behavior.

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

## OpenTerminalUI feature-parity track

### Baseline, scope, and definition of parity

This track compares Market Terminal after commit
[`448e2b2`](https://github.com/thedjpetersen/market-terminal/commit/448e2b2925472242789cfc6569cb16a28645ab42)
with OpenTerminalUI at commit
[`fc16fd6`](https://github.com/Hitheshkaranth/OpenTerminalUI/commit/fc16fd646405aec7a5525387be89c0cb376137c5),
the OpenTerminalUI `main` head inspected on 2026-08-28. The reference surface
comes from its README feature inventory, React route registry, navigation rail,
FastAPI router composition, representative domain tests, and CI workflow. Pinning
the commit prevents a changing upstream menu from silently changing this plan.
The review evidence is the pinned
[`README.md`](https://github.com/Hitheshkaranth/OpenTerminalUI/blob/fc16fd646405aec7a5525387be89c0cb376137c5/README.md),
[`App.tsx`](https://github.com/Hitheshkaranth/OpenTerminalUI/blob/fc16fd646405aec7a5525387be89c0cb376137c5/frontend/src/App.tsx),
[`Sidebar.tsx`](https://github.com/Hitheshkaranth/OpenTerminalUI/blob/fc16fd646405aec7a5525387be89c0cb376137c5/frontend/src/components/layout/Sidebar.tsx),
[`IconRail.tsx`](https://github.com/Hitheshkaranth/OpenTerminalUI/blob/fc16fd646405aec7a5525387be89c0cb376137c5/frontend/src/components/layout/IconRail.tsx),
[`router.py`](https://github.com/Hitheshkaranth/OpenTerminalUI/blob/fc16fd646405aec7a5525387be89c0cb376137c5/backend/api/router.py),
and
[`ci.yml`](https://github.com/Hitheshkaranth/OpenTerminalUI/blob/fc16fd646405aec7a5525387be89c0cb376137c5/.github/workflows/ci.yml).
The machine-readable
[`openterminalui-parity-ledger.json`](openterminalui-parity-ledger.json) expands
that source audit into 40 stable capabilities. At the pinned tree, the reference
contains 1,532 tracked files, 574 frontend source files, 672 backend files, 115
React route declarations, 44 mounted API routers, 168 backend test files, and 29
browser E2E specs. Those counts are discovery evidence, not completion claims:
the ledger records implementation maturity separately because the source itself
labels one mounted router group as stubs and several routes are thin wrappers.

Parity means equivalent user outcomes, input coverage, disclosures, and failure
behavior in a native terminal. It does **not** mean React/FastAPI compatibility,
matching URLs, a browser/mobile layout, copying visual assets, or duplicating a
route whose only implementation is a placeholder. A reference feature counts as
matched only when Market Terminal has a real bounded context, keyboard and mouse
routing, deterministic fixtures, typed unavailable/entitlement states, persistence
where user state is involved, and the verification evidence defined below.

OpenTerminalUI is MIT-licensed, but this remains a behavior-level comparison.
Do not copy its source, screenshots, branding, prose, fixture data, command names,
or visual design without a deliberate dependency decision, preservation of the
MIT notice, and a license/data review. Market Terminal's Rust/terminal architecture,
command vocabulary, themes, spreadsheet, and stricter calculation evidence remain
product-owned differentiators.

### Current parity assessment

Status uses four values: **covered** means the core user outcome exists now;
**partial** means a useful vertical slice exists but the reference surface is
broader; **missing** means no production user workflow exists; **divergent** means
the browser-specific mechanism needs a terminal-native equivalent. “Covered beyond
reference” marks a product-owned capability that should remain while parity work
continues.

| Reference capability | Status | Market Terminal evidence | Gap required for parity |
| --- | --- | --- | --- |
| GO bar, command palette, function shortcuts | Covered | Typed command parser, exact registry, command history, AI fallback, Help | Add fuzzy discovery over every new parity command and action as those commands land. |
| Mission Control, launchpad, ticker tape | Partial | Overview, Desk, Markets, persisted active workspace | Add configurable launch tiles, live pulse strip, priority stack, and saved role presets. |
| Keyboard navigation and icon rail | Partial | `Esc` feature focus, deterministic spatial arrows, Enter activation, workspace fallback, tmux prefix, remappable keys, shell-level `F` hints, Portfolio tabs/rows/reload, composed Desk panes, Monitor rows/controls, Security tabs/chart/Form 4/filing/peer/retry actions, Chart periods/studies/comparisons/inspection/modes/promotion/refresh, and modal-safe News filters/headlines/story/calendar/reader actions | Adopt the action contract in remaining Overview, Alerts, and Spreadsheet rows, buttons, controls, and tables. |
| Saved views and workspace presets | Partial | Workspace order, active workspace, layout and command history persist | Persist/restore view parameters, filters, selected instrument, table columns, chart state, pane geometry, and Trader/Quant/PM/Risk/Ops presets. |
| Themes and responsive shell | Covered | Nine themes and semantic goldens at three terminal sizes | Add contrast assertions and parity-feature narrow-layout goldens; browser/mobile rendering is out of scope. |
| Accounts, authentication, and roles | Missing | Local single-user configuration and secret-presence display only | Add optional local profiles, encrypted credentials, session locking, role/capability policy, and audit actor identity before any shared or consequential workflow. |
| Snapshot/streaming data and provider fallback | Partial | Yahoo, Alpha Vantage, Alpaca, Finnhub, bounded workers, coalescing, LKG cache, replay | Add capability-aware provider waterfall, durable bar cache, session calendars, health routing, and cross-provider provenance. |
| DOM, time and sales, hotlists, heatmaps | Missing | Listed-instrument monitor and quote stream only | Add entitlement-aware depth/tape models, movers, breadth, sector/market heatmaps, and replay fixtures without synthesizing unavailable order-book data. |
| Multi-panel technical chart workstation | Partial | OHLC/line charts, comparisons, volume, SMA/EMA, RSI, periods, cursor | Add up to nine linked panes, multi-timeframe layouts, indicator registry, annotations, volume profile, historical replay, alternate chart types, and image/data export. |
| Security Hub and equity research | Partial | Profile, financials, filings, SEC Form 4, news, basic peer/estimate states | Add statements/trends, estimates/revisions, earnings, ESG, corporate actions, dividends, shareholding history, richer peers, and multi-market identity. |
| Advanced screener and factor dashboard | Missing | Search and spreadsheet formulas are reusable foundations | Add versioned universes, typed filters/formulas, stable ranking, factor composites, why-ranked evidence, saved screens, and promotion to Monitor/Sheet. |
| Tool-using AI research | Partial | Codex/OpenRouter chat, validated UI intents, bounded article reading | Add screen-aware read tools, research retrieval, cited artifacts, provider routing, debate, bounded strategy research, and local-model fallback. |
| Futures and options suite | Missing | Stage 3 domain placeholder only | Add chains, Greeks/IV, OI/PCR/flow, strategies, futures basis/curve, expiry calendar, scenario tools, and delayed/entitled data states. |
| Portfolio accounting and attribution | Partial | Exact positions, cash/activity, valuations/TWR, lots, realized gains, fills, single/multi-period attribution | Add portfolio CRUD/transaction truth, allocation views, benchmark history, dividends, multi-portfolio comparison, and rebalance evidence. |
| Risk, stress, and correlation | Partial | Concentration and explicit non-cash shock plus flow-adjusted historical/EWMA volatility, drawdown/recovery, historical/Gaussian VaR/CVaR, Sharpe/Sortino, beta, correlation, tracking error, and information ratio over versioned per-currency valuations | Add marginal/component risk, rolling correlation, PCA/factor exposure, scenario library, Monte Carlo, clustering, and cross-asset dependency views. |
| Paper trading, journal, TCA, position sizing | Missing | Verified broker executions are strictly read-only | Add visibly simulated orders/fills, sizing, journal and behavior analytics, execution-cost models, TCA, approvals, and immutable paper audit state. Live routing remains excluded. |
| Backtesting, Model Lab, robustness | Missing | Deterministic market replay is infrastructure, not a strategy backtester | Add look-ahead-safe engine, costs/fills, templates, walk-forward and parameter sweeps, robustness, experiment tracking, tear sheets, and governance. |
| Portfolio Lab and optimizer | Missing | Performance/attribution calculators cover realized portfolios only | Add portfolio backtests, weighting/rebalancing, strategy blends, optimizer constraints, correlation, attribution, and reproducible run comparison. |
| Cockpit and intelligence timeline | Partial | Overview composes positions and news | Add ranked portfolio risks, catalysts, alerts, movers and model signals plus a source-linked chronological event timeline. |
| Cross-asset and macro workspaces | Missing | Markets explicitly marks unsupported datasets unavailable | Add FX, commodities, crypto, ETF, mutual-fund, bond, yield-curve, economics, and sector-rotation contexts through licensed adapters. |
| Compound alerts and delivery | Partial | Restart-safe price/move rules, debounce, acknowledgement, audit, local simulation | Add compound technical/news/portfolio/calendar/sheet rules, cooldown/expiry/limits, breakout scans, and opt-in external channels with delivery audit. |
| Operations, data quality, and governance | Partial | Structured tracing, lag/drop metrics, provider quality in feature views | Add consolidated health, cache/feed status, data-quality incidents, kill switches, restricted-list policy, model registry/approval, and operator audit views. |
| Plug-ins, scripting, and external tool API | Missing | Feature-owned Rust modules only | Add capability-scoped plug-in manifest, sandboxed calculations, budgets, signing, versioned scripting, and read-only MCP tools. |
| Spreadsheet composition | Covered beyond reference | Native durable workbook, 27 pure functions and four financial functions | Preserve as a differentiator and make every new parity result promotable to a typed sheet range. |
| Terminal chat | Covered beyond reference | TLS IRC rooms, presence, reconnect, bounded queues | Preserve independently; it is not a prerequisite for OpenTerminalUI parity. |

The assessment intentionally does not infer correctness from route count. For
example, the pinned OpenTerminalUI router itself labels one group as “stubs”; this
track targets the documented, user-observable behavior and sets Market Terminal's
own evidence bar.

### Parity-wide definition of done

Every item below must satisfy all applicable criteria before its status changes
to complete:

1. **Domain and boundary:** the feature owns its vocabulary and consumer-side
   ports; composition happens in `bootstrap.rs`; other features exchange stable
   intents, events, or versioned read models rather than importing internals.
2. **Truthfulness:** provider, source/as-of/receive time, delay, derivation,
   entitlement, currency, units, methodology, input version, and missing data are
   visible wherever they affect interpretation. Gallery fixtures never replace a
   failed live source in the interactive app.
3. **Interaction:** all visible actions are reachable by documented keys, mouse,
   spatial panel focus, and `F` follow hints. Focus survives refresh and layout
   changes; disabled actions say why; no workflow relies on color alone.
4. **Responsive behavior:** semantic goldens cover 80 x 24, 120 x 36, and
   160 x 48. A narrow layout may collapse into tabs or drill-downs but may not
   truncate controls, hide provenance, or create unreachable state.
5. **Determinism and correctness:** pure calculations have reference cases,
   property/invariant tests, explicit units and rounding, fixed clocks, stable
   ordering, and look-ahead checks. A fixed input/version must produce the same
   output and export.
6. **Asynchrony and resilience:** the render/input thread performs no I/O.
   Requests are bounded and cancellable; streams coalesce or backpressure;
   retries/rate limits/cache freshness/LKG behavior are observable; partial
   provider failure cannot erase the last valid view.
7. **Persistence and migration:** saved user state is bounded, schema-versioned,
   crash-safe, migratable, and recoverable from a previous valid generation.
   Secrets and licensed payloads are not stored in feature documents.
8. **Performance:** relevant interaction remains below the CI 50 ms p95 gate on
   supported fixtures. Large universes, histories, grids, simulations, and panes
   receive separate time and memory budgets before release.
9. **Export and audit:** exports carry inputs, versions, timestamps, methodology,
   quality, and license-appropriate attribution. Mutations record actor, intent,
   prior/new state, confirmation where applicable, and result.
10. **Live contract:** deterministic fixtures are mandatory; any live adapter also
    has an opt-in contract test using a documented permitted source. Lack of keys
    produces an explicit unavailable/permission state, not invented data.

### P0 — Parity ledger, navigation contract, and evidence harness

**Priority: immediate. Dependencies: Stage 0.** This package prevents a broad
feature list from becoming a collection of menu placeholders.

- Maintain the versioned parity ledger with stable `OTUI-*` capability IDs, owner
  context, current/target status, reference source link, dependencies, adapter
  requirements, legal review state, and acceptance-test IDs. Update the pinned
  upstream commit only through a reviewed diff.
- Extend the workspace action registry so a feature exposes actionable rows,
  tabs, controls, table links, and pane targets without shell code knowing its
  domain. Generate conflict-free one- or two-letter follow labels for the current
  viewport and preserve prefix-free matching after scrolling or resizing.
- Implement spatial arrow routing from registered rectangles, including split
  panes and nested controls. Define deterministic tie-breaking, wrapping policy,
  disabled/hidden actions, modal trapping, Escape hierarchy, and focus restoration.
- Add a parity gallery that renders every capability's loading, populated, empty,
  delayed, stale, denied, partial, and failed states at all three supported sizes.
- Add CI checks that a capability cannot be marked complete without its command,
  Help entry, semantic goldens, deterministic contract test, data-source register
  entry, and performance case.
- Keep package-by-feature ownership executable as the parity surface expands:
  production contexts cannot import concrete adapters or peer contexts;
  consumer-owned ports and DTO translators are required for composed reads;
  domain and port layers remain independent of shell and rendering code.

**Exit evidence:** shell and feature action registries have duplicate/conflict
tests; follow hints route every visible action in representative Overview, Desk,
Security, Portfolio, Chart, News, Alerts, and Spreadsheet frames; spatial focus
has geometry/property tests; the ledger is pinned to an exact upstream commit and
CI rejects incomplete evidence links.

**Status: in progress.** The checked-in source ledger now covers 40 capabilities
and is pinned, uniquely identified, source-linked, maturity-qualified, owned,
prioritized, and validated by `tests/parity_ledger.rs`. The bounded feature-action
contract now filters invalid, disabled, duplicate, off-screen, and excess actions,
assigns at most two-letter codes, renders badges at feature-owned rectangles, and
revalidates activation.
The shell now restores a feature-preferred action, navigates validated rectangles
with lane-first distance and stable tie-breakers, refuses edge wrapping, falls
back to workspace traversal when no local actions exist, revalidates on Enter,
and refreshes focus after resize or asynchronous state changes. Portfolio supplies
visible tabs, security-capable rows, and reload as the leaf reference. Desk now
supplies visible split-pane headers and safely namespaced nested child actions,
with responsive hidden-pane exclusion. Monitor now supplies its visible rows and
discrete footer controls from the same geometry used by rendering and mouse input,
including real nested routing inside Desk. Remaining workspace adopters, gallery
states, and enforcement that completed capabilities link real implementation
evidence remain. Security now contributes its responsive tabs, live chart,
Form 4 and regulatory filing rows, peer links, and retry/refresh states from one
shared geometry model, with stale view/symbol/accession validation and a
shell-level follow-routing test. Chart now supplies direct period destinations
and every visible study, comparison, inspection, mode, Spreadsheet, and refresh
control through one responsive render/mouse/action geometry. Period and mutable
state are revalidated on activation, disabled cursor directions disappear from
the shell registry, the active period restores focus, and an application test
routes generated labels through both a period and a stateful control. Overview,
Alerts, and Spreadsheet remain to adopt the contract. News now derives filters,
visible headline rows, selected-story commands, detail links, calendar events,
and refresh from a shared responsive geometry. Story identities are revalidated
against the latest feed before activation. Its full-screen reader is the first
modal reference: shell/workspace actions and navigation clicks are suppressed,
follow hints contain only modal destinations, and close restores normal focus
routing. Populated News and reader frames are locked at all three standard sizes.
`tests/architecture_boundaries.rs` now enforces
dependency direction on every CI run. The first remediation moved Assistant's portfolio
context behind an Assistant-owned port and composition-root translator, and
removed Watchlist's dependency on a Market Data identity alias. This closes the
known production cross-context leaks; future composed reads must use the same
consumer-owned contract pattern.

### P1 — Mission Control, launchpad, and saved workspaces

**Priority: immediate. Dependencies: P0.** Match the reference's daily entry and
workspace recovery experience while retaining the terminal shell.

- Turn Overview into Mission Control with a live market pulse, portfolio summary,
  upcoming events, source health, recent/saved work, and ranked actionable items.
  Every card must drill into the owning context and retain its as-of/quality state.
- Add Launchpad as a user-editable grid of commands, instruments, saved screens,
  portfolios, sheets, and layouts. Support reorder, remove, rename, keyboard move,
  import/export, and crash-safe persistence.
- Add Trader, Quant, PM, Risk, and Ops presets as versioned seeds, not hard-coded
  special cases. Applying a preset previews the workspace order/panes and never
  destroys custom views without explicit confirmation.
- Expand saved views to capture workspace, canonical instrument, tabs, filters,
  sort, columns, chart panes/studies/periods, selected row, scroll position where
  meaningful, and split geometry. Define migration and graceful degradation when
  a command, provider, or instrument disappears.
- Unify command discovery across exact functions, saved objects, instruments,
  actions, settings, and Help. Preserve exact-command precedence and expose why a
  fuzzy or AI result was chosen before dispatch.

**Exit evidence:** a keyboard-only test creates a multi-pane view, saves it,
restarts, restores the same semantic frame, switches presets, and returns without
state loss. Mission Control remains useful with every external provider offline
and labels each unavailable card rather than substituting fixture data.

### P2 — Market-data fabric, microstructure, and chart workstation

**Priority: high. Dependencies: P0-P1 and Instrument Master.** This is the shared
data foundation for screening, derivatives, risk, and backtesting.

- Add a capability-aware provider router keyed by instrument/venue/asset class,
  field, frequency, depth, delay, entitlement, and permitted retention. Define
  primary/fallback order, circuit state, rate-limit budgets, cache policy, symbol
  mapping, and provenance for each hop; never silently join incompatible sources.
- Add bounded durable bar storage with schema/version, corporate-action policy,
  session calendar, timezone, gap detection, repair audit, checksum, retention,
  and cache-health metrics. Keep in-memory quote LKG separate from historical
  truth and support deterministic offline/replay mode.
- Extend canonical identity to options, futures, indices, rates, FX pairs,
  commodities, crypto, ETFs, funds, and bonds, including venue, currency,
  multiplier, expiry/strike/right, calendars, rolls, and provider mappings.
- Add entitlement-aware depth of market and time-and-sales contexts with price
  levels, sequence/gap detection, aggressor/condition state where licensed,
  bounded aggregation, pause/replay, and explicit unsupported states.
- Add hotlists, breadth, movers, unusual volume, sector/market heatmaps, and
  normalized split comparison over versioned universes. Results must expose
  universe time, coverage, exclusions, and stable ranking.
- Evolve Chart into a one-to-nine-pane workstation with linked crosshairs,
  canonical selection, independent/synchronized periods, multi-timeframe layouts,
  pane focus, saved layouts, and graceful narrow-terminal tabbing.
- Introduce a typed indicator registry with at least the reference's major
  families: trend, momentum, volatility, volume, breadth, and profile. Start with
  MACD, Bollinger, ATR, VWAP, OBV, stochastic, ADX, Donchian, Keltner, Supertrend,
  CMF, CCI, and volume profile; specify warm-up, nulls, adjustments, units, and
  reference vectors for each rather than chasing an indicator count.
- Add persistent annotations, bar-by-bar historical replay, Renko/Kagi/point-and-
  figure/line-break transformations, and terminal-appropriate PNG/SVG/CSV export.
  Scripted indicators are deferred to P8's sandbox.

**Exit evidence:** provider-failover tests prove field-level provenance and no
source blending; quote/depth sequence faults are visible; a nine-pane replay stays
within budgets; chart calculations match independent vectors; saved pane layouts
round-trip; exports reproduce the displayed input version.

### P3 — Research, screening, factors, news, and alerts

**Priority: high. Dependencies: P2.** Complete the reference's idea-discovery
loop before adding more asset classes.

- Expand Security Research into explicit Overview, Financials, Chart, News/
  Sentiment, Ownership, Estimates, Peers, ESG, Earnings, and Corporate Actions
  views. Preserve filing/source links and per-field availability rather than
  merging unlike provider periods or estimates.
- Add multi-period income statement, balance sheet, and cash-flow normalization;
  guidance/earnings surprises; analyst revisions/targets; dividend/split/rights/
  bonus timelines; shareholding history; insider buyer/seller and cluster views;
  and sector-relative peer matrices.
- Build Screening as a bounded context with versioned universes, a typed formula
  AST, field catalog, unit checking, null policy, deterministic filters/sorts/
  ranks, point-in-time inputs, saved presets, and explainable rejection/rank
  evidence. Results route to Security/Chart and promote atomically to Monitor or
  a typed Spreadsheet range.
- Add factor research for value, momentum, quality, size, and low-volatility with
  winsorization, sector neutralization, z-scores, weights, coverage, exposure,
  return history, information coefficient, turnover, and why-ranked components.
  A later Alpha Zoo may add formulaic factors only after look-ahead and multiple-
  testing controls exist.
- Add relative-strength, sector-rotation, dividend, insider, catalyst, and
  intelligence-timeline views over the same versioned research/event contracts.
- Add licensed ticker-scoped and market news sentiment with lexical/model source,
  confidence/calibration, observation time, and fallback disclosure. AI emotion
  is optional and may not replace missing articles or claim objective truth.
- Generalize Alerts to typed AND/OR expression trees over price, move, volume,
  indicators, news/topic, portfolio thresholds, calendar events, and spreadsheet
  expressions. Add preview, cooldown, expiry, maximum triggers, schedules,
  deduplication keys, breakout-pattern evidence, and per-condition audit.
- Add opt-in desktop, email, webhook, Slack, and Telegram adapters only after
  secret storage, destination verification, rate limits, redaction, retry/dead-
  letter state, test delivery, and user-visible delivery audit exist.

**Exit evidence:** a point-in-time screen yields the same ordered rows and reasons
after restart; factor results reconcile from raw observations; stale/missing fields
cannot pass a filter accidentally; compound alerts replay idempotently across a
restart; delivery failures do not duplicate triggers; every research field links
to permitted source evidence.

### P4 — Portfolio, risk, optimizer, cockpit, and paper workflows

**Priority: high. Dependencies: P2-P3 and current Stage 2 calculators.** Finish
Stage 2 and match the reference's portfolio decision surface.

- Promote imports into multi-portfolio CRUD with immutable transaction truth,
  account/book hierarchy, cash and corporate actions, reconciliation exceptions,
  tax-lot policy, dividend income/calendar, benchmarks, valuation schedules, and
  reproducible position snapshots. Editing source transactions produces a new
  version instead of rewriting calculation history.
- Add allocation and relative-performance views across asset, sector, country,
  currency, factor, account, and custom classifications. Link every total through
  contribution/attribution to positions, lots, executions, and Security.
- Add historical volatility, EWMA, beta, tracking error, information/Sharpe/
  Sortino ratios, max drawdown/recovery, historical and parametric VaR/CVaR,
  marginal/component risk, rolling correlation, PCA/factor exposure, and explicit
  confidence/window/sample/annualization/missing-data metadata.
  - **Delivered foundation:** Portfolio's bounded dated-valuation history now
    feeds a storage-independent Risk calculator. It computes exact
    flow-adjusted observations; annualized sample and zero-mean EWMA volatility;
    wealth-index peak, trough, and recovery; empirical loss-tail and Gaussian
    VaR/CVaR; Sharpe/Sortino; and, with a complete benchmark, beta, correlation,
    tracking error, and information ratio. `RISK HISTORY` exposes per-currency
    results and the confidence, lambda, risk-free, sample, interval,
    annualization, version, methodology, and disclosure evidence.
  - **Still required:** security/asset contribution and marginal risk, rolling
    windows and pairwise sample policy, correlation matrices and regime views,
    PCA and classified factor exposures, missing-data windows, and independent
    external reference vectors for each estimator.
- Add scenario libraries for historical crises and explicit shocks plus a custom
  scenario builder. Scenarios identify repricing assumptions, unsupported assets,
  linear/nonlinear treatment, currency conversion, and coverage; Monte Carlo
  identifies model/distribution/seed and is never presented as a forecast.
- Add correlation matrix, rolling/regime views, hierarchical clustering, and
  cross-asset dependency drill-down with pairwise sample counts and PSD/missing-
  data policy.
- Add portfolio optimizer with constrained mean-variance, minimum variance,
  volatility target, risk parity, momentum/equal-weight baselines, turnover/cost
  limits, bounds/groups, infeasibility explanations, frontier sensitivity, and a
  preview-only rebalance proposal.
- Add Portfolio Lab for multi-asset backtests, rebalance schedules, strategy
  blends, run comparison, contribution, drawdowns, and correlation. It must reuse
  P6's backtest contracts rather than create a second execution simulator; this
  sub-slice therefore begins only after that boundary is stable.
- Add Cockpit as a deterministic priority stack over risk breaches, alerts,
  catalysts, movers, data incidents, and model signals. Ranking inputs/weights and
  dismissed/acknowledged state are inspectable and auditable.
- Add position sizing, paper order tickets, deterministic simulated fills,
  commission/slippage/partial-fill/latency/impact models, blotter, journal, TCA,
  and behavioral review. Every screen and export says `PAPER`; preview and explicit
  confirmation precede submission; no adapter can route a live order.

**Exit evidence:** transaction fixtures reconcile through cash, lots, valuation,
performance, attribution, and risk; independent references cover each risk model;
optimizer constraints and infeasibility have property tests; scenario totals drill
to positions; paper replay is deterministic and cannot call a live endpoint; the
Cockpit priority order reproduces from a versioned snapshot.

### P5 — Derivatives, fixed income, cross-asset, and macro

**Priority: medium. Dependencies: P2 and P4 risk conventions.** Match the
reference's multi-market breadth through separate asset-class contexts rather
than a universal instrument bag.

- Add option chains with expiries/strikes, bid/ask/OI/volume/IV, calls/puts,
  contract identity, quote quality, and venue calendars. Add Black-Scholes as an
  initial transparent reference model, then dividends/rates/early-exercise models
  only with independent cases; show model Greeks separately from provider Greeks.
- Add IV term structure/skew, Greeks, OI build-up, PCR, unusual flow, strike
  heatmaps, expiry/roll calendar, and a multi-leg strategy builder with payoff,
  scenario, max gain/loss caveats, breakevens, net premium, and contract multipliers.
- Add futures reference data, continuous-series/roll policy, basis, carry, term
  structure, specifications, expiry, seasonality, and portfolio scenario links.
- Add fixed-income cash-flow schedules, price/yield, accrued interest, day counts,
  calendars, duration/convexity, spread measures, Treasury curve construction,
  historical comparison, inversion signals, and curve shocks with convention data.
- Add FX spot/cross/forward/carry and central-bank calendars; commodity curves,
  rolls/spreads/seasonality; crypto spot/derivatives/sector/DeFi views; ETF holdings,
  flows and overlap; and mutual-fund search, rolling returns, category ranks, SIP
  calculator, and overlap. Each gets asset-specific identity and license policy.
- Add economics with release calendar, prior/consensus/actual/revision, impact,
  surprise history, macro-series transformations, source revisions, and links into
  Alerts, Chart, Risk, and Spreadsheet.

**Exit evidence:** pricing/reference cases document conventions and units; chain
and curve fixtures include stale/partial/entitlement failures; continuous futures
and fund returns expose methodology; no cross-asset workspace fabricates a common
field; scenarios and exports preserve multipliers, currencies, calendars, and
source terms.

### P6 — Backtesting, statistical research, and model governance

**Priority: medium. Dependencies: P2 histories, P3 factors, P4 paper execution.**
Match the reference's research lifecycle with stronger reproducibility controls.

- Build a look-ahead-safe event/vector backtest boundary with point-in-time data,
  session calendars, corporate actions, signal timing, universe membership,
  warm-up, order lifecycle, partial fills, commissions, spread/slippage/latency/
  impact, cash, borrow, leverage, and deterministic seeded execution.
- Ship representative trend, mean-reversion, breakout, momentum, RSI/MACD/
  Bollinger/VWAP and allocation templates as tested examples, not performance
  promises. Every template identifies assumptions and has a no-leakage test.
- Add equity/drawdown/monthly-return/rolling-risk views, benchmark comparison,
  trades, turnover, capacity/liquidity, parameter surfaces, Monte Carlo resampling,
  and versioned HTML/terminal/CSV tear sheets.
- Add experiment tracking with immutable code/config/data hashes, environment and
  seed, queued/running/cancelled/failed states, parameter sweeps, walk-forward and
  purged out-of-sample validation, run comparison, tags/notes, and reproducible
  promotion to paper only.
- Add robustness statistics including bootstrap intervals, probabilistic/deflated
  Sharpe, minimum track record, multiple-testing correction, rolling stability,
  sensitivity and a plain-language verdict that can reject a result.
- Add Statistical Lab and pair trading with stationarity/cointegration tests,
  hedge ratio, residual diagnostics, half-life, spread z-score, structural-break
  warnings, costs/borrow, formation/trading split, and basket extension.
- Add Algorithm Framework contracts for universe, alpha, portfolio construction,
  risk, execution, and result modules; Alpha Zoo factors implement these contracts
  with point-in-time operators and IC/IR/turnover/correlation diagnostics.
- Add model registry/governance with versions, owners, validation evidence,
  limitations, approvals, risk limits, retirement, audit history, and paper-only
  promotion. Add strategy export only for an explicitly supported subset, with
  semantic-difference warnings and golden compilation fixtures for target syntax.

**Exit evidence:** adversarial fixtures prove no future data reaches a decision;
runs reproduce from hashes and seed; cost-free and costed results reconcile;
walk-forward boundaries are visible; robustness can fail weak strategies; model
promotion requires recorded evidence and cannot enable live trading.

### P7 — AI research, research library, and intelligence automation

**Priority: medium. Dependencies: P3 and P6 versioned research artifacts.** Keep
the AI plane read-only and evidence-bound while matching the reference workflow.

- Add a screen-aware context envelope containing canonical selection, workspace,
  visible input versions, and entitlement-filtered summaries. Users can inspect
  and remove context before sending; portfolio/account data is opt-in and bounded.
- Expose read-only tools for instrument lookup, quotes/history, Security fields,
  news/events, screening, compare, portfolio/risk snapshots, backtests, and the
  research library. Validate typed inputs/outputs, cap rows/tokens/time, attach
  provenance, and re-check every navigation intent through the command registry.
- Add a local research library for notes and permitted documents with source,
  author/time, checksum, chunk/version, citations, deletion, and access policy.
  Retrieval output distinguishes source text from model inference and defends
  against document prompt injection.
- Add provider routing across Codex/OpenRouter and optional local OpenAI-compatible
  models with capability/health policy, explicit model/version, redacted logs,
  cancellation, cost/token limits, and honest fallback banners.
- Add optional fundamental/technical/sentiment analyst views and bull/bear debate.
  Each claim cites a tool result; disagreement and missing evidence remain visible;
  any BUY/HOLD/SELL-like text is labeled research output, never an executable order.
- Add a bounded Strategy Lab/Research Autopilot loop: hypothesis, signal, backtest,
  one-variable iteration, mandatory out-of-sample/robustness, attribution, verdict,
  saved artifact. Cap rounds and time; preserve failed hypotheses; prohibit order
  submission or silent parameter search.
- Add AI insight cards and sentiment/emotion summaries only where the deterministic
  underlying facts stay visible and the card has model/source/age/fallback state.
  Add a unified intelligence timeline over source events, not generated events.
- Expose the same read-only tools over an optional local MCP server with explicit
  capabilities, authentication, row/rate limits, audit, and no mutation methods.

**Exit evidence:** tool calls and citations replay against fixed snapshots; prompt-
injection fixtures cannot invoke hidden tools or actions; screen context never leaks
unselected accounts; provider failure degrades visibly; Autopilot rejects a known
curve-fit fixture; MCP and in-app tools share contracts and remain read-only.

### P8 — Plug-ins, operations, profiles, and controlled deployment

**Priority: later. Dependencies: P0-P7 contracts.** Match the reference platform
surface without weakening the modular monolith or creating an unsafe live OMS.

- Add a versioned capability-scoped plug-in manifest for commands, calculations,
  indicators, screens, and read-only data adapters. Require declared permissions,
  compatible API range, resource budgets, cancellation, provenance, package
  signature/checksum, install/disable/update/remove lifecycle, and crash isolation.
- Add sandboxed scripting for calculations/indicators with a small deterministic
  API, no ambient filesystem/network/secrets, memory/time/output limits, versioned
  dependencies, reproducible tests, and a reviewable library. Sharing never grants
  execution implicitly.
- Add consolidated Ops/Data Quality views for adapter health, circuit/rate state,
  stream lag/drops/gaps, cache coverage/age, background jobs, persistence recovery,
  schema migrations, alert delivery, model runs, incidents, and safe per-capability
  kill switches.
- Add optional profiles and shared deployment only with encrypted secret storage,
  password/session policy, local recovery, RBAC/capabilities, tenant document
  ownership, audit actor, backup/restore, conflict semantics, CSRF/origin policy
  where applicable, and privacy/retention controls.
- Add restricted-list, approval, and surveillance-style policy only to paper and
  research workflows. Call it OMS/compliance only when its scope is explicit; do
  not imply regulated compliance or production certification.
- Add install/upgrade/rollback/diagnostics commands, platform packages, health
  checks, signed releases/SBOM, migration dry-run, backup verification, offline
  mode, and operator runbooks. A Redis/Postgres/browser stack is not required for
  parity unless measured multi-user scale justifies a service deployment.
- Live order routing remains outside feature parity. It requires separate legal
  review, threat model, broker sandbox contracts, reconciliation, approvals, kill
  switches, immutable audit retention, incident drills, and explicit user scope.

**Exit evidence:** malicious plug-in/script fixtures cannot escape capabilities;
kill switches stop bounded work without corrupting state; backup/restore and
rollback drills pass; RBAC tests prevent cross-profile reads; audit exports identify
actor and version; no executable path reaches a live broker.

### Dependency order and parity completion report

The critical path is `P0 -> P1 -> P2 -> P3 -> P4`, which completes the daily
research, screening, portfolio, risk, and paper loop. `P5` and `P6` may proceed in
parallel after P2, but P6 reuses P4's simulated execution. P7 consumes the stable
read-only artifacts from P3/P6. P8 is last because plug-in and multi-user contracts
must wrap proven capabilities rather than freeze premature APIs.

Parity is complete only when the ledger has no unexplained `partial` or `missing`
rows; every intentional divergence identifies the terminal-native outcome and
evidence; the full formatting, lint, test, semantic-golden, release-build, live-
contract, and performance suites pass; and a published report links each `OTUI-*`
capability to implementation, docs, source register, and acceptance evidence.
OpenTerminalUI changes after the pinned commit become a new reviewed baseline,
not an automatic expansion of this commitment.

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

**Status: in progress.** Position-snapshot ingestion, the first exact
cash/activity ledger, dated per-currency valuations, and broker open-tax-lot
exports supply separate versioned Portfolio inputs. The first Performance slice
calculates flow-adjusted TWR plus optional benchmark and active return, and the
first Risk slice consumes positions without storage access. Risk now also
consumes the separate dated-valuation boundary to calculate flow-adjusted
historical/EWMA volatility, drawdown/recovery, empirical and Gaussian VaR/CVaR,
Sharpe/Sortino, and optional benchmark-relative beta, correlation, tracking
error, and information ratio with explicit estimator metadata. A pure Portfolio
calculator plus a bounded CSV adapter and terminal drill-down now reconcile
single-period security contribution and optional benchmark-active attribution;
the multi-period boundary plus its bounded CSV adapter and terminal drill-down
now link ordered security, benchmark, and active contributions. Component,
rolling-correlation, factor/PCA, scenario, and simulation risk libraries remain.
Screening and the additional
news/topic, portfolio-threshold, calendar, and spreadsheet-expression alert
rule families remain separate Stage 2 slices.

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
