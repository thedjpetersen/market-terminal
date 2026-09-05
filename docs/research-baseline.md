# Application assessment and research reliability milestone

Assessment date: 2026-09-05, before the Cloudflare companion was built.

This is the historical assessment and verification record. The public,
device-local browser companion subsequently delivered the research workflow;
see [the current web host](../web/README.md) for its scope and limitations.
Authenticated tenant synchronization remains separate work. The original test
totals below include pre-existing local native edits; the delivery handoff
validates the committed source independently.

The next substantial goal was to make the existing research workflows reliable
before extending them to a browser. The app has a broad native product surface
and a useful deterministic analytical engine. The HTTP host currently exposes
four engine operations and optional read-only research artifact queries; it is
not yet a backend for the complete native product. At assessment time there was no browser UI.

## Assessment coverage

The review covered the eight Rust crates, composition roots, feature boundaries,
persistence adapters, provider integrations, HTTP contracts and CI. Hands-on
work used the actual terminal binary in a pseudo-terminal with isolated state,
public Yahoo/SEC/news inputs, and synthetic portfolio data. The HTTP service was
run on loopback with ephemeral credentials. No private brokerage data, outbound
chat messages, trades or published deployments were needed.

| Area | Current strength | Remaining product gap |
| --- | --- | --- |
| Find, Monitor, Markets, Charting, Security | Canonical instruments, dated quotes, multi-series charts, SEC fundamentals/filings | Durable named watchlists, comparable financial periods and metric-level provenance, estimates/peers, complete source details at narrow widths |
| News and calendar | Symbol feeds, local filters, source-linked article reader | Durable bookmarks/read state, live event calendar and research notes |
| Portfolio, Risk, Overview | Position ingestion, exact calculations, explicit missing historical inputs | Cohesive import/reconciliation flow, refreshable overview projections, configurable saved scenarios |
| Screening | Predicate evidence and versioned inputs | Whole-result export/insertion, broader universe management, reusable candidate baskets |
| Backtesting | Deterministic fills, execution costs, saved artifacts and paired comparisons | Explicit period/warm-up, retained bars for offline reruns, benchmark and out-of-sample research |
| Options and Fixed Income | Shared deterministic valuation/scenario engines | Reconciled portfolio scenarios and editable proposed hedges; browser model workspaces |
| Spreadsheet | Feature-owned evaluation, qualified formulas, durable workbooks, undo/redo | Browser workbook editing/storage and bulk evidence transfer from research tools |
| Alerts | Local rule lifecycle, audit and durable state | Server-owned evaluation and notification delivery when no client is running |
| Desk, Launchpad, saved views | Composable workspaces and typed saved-view restoration | Instrument link groups, active destination visibility, unsaved-session restoration |
| Assistant and Chat | Existing native integrations and constrained commands | Host-neutral identity, browser streaming/action flows and integration-specific verification |
| HTTP and web contracts | Authenticated, budgeted engine execution and artifact reads | Browser identity/session lifecycle and most native application operations |

Authenticated AI/IRC services and provider contracts requiring unavailable keys
were not exercised end to end. Existing ignored live tests remain explicit
opt-in checks. A working gallery fixture is not evidence of a working provider.

## Delivered reliability changes

- SEC annual facts merge supported replacement tags by reporting period and
  choose the latest filing for duplicates. Equal filing dates retain documented
  tag precedence. Missing metrics stay unavailable. The running app now shows
  Apple FY2025–FY2023 rather than FY2018–FY2016.
- The native host supplies actual UTC time. The standard shell says `LOCAL` or
  `DEMO`; provider freshness remains feature-owned. Fabricated index prices and
  the unconditional `LIVE`/fixed New York time are removed. Offline captures use
  an explicit unavailable clock.
- Alerts schedule observations while the application runs, including when a
  different workspace is active. They use `MARKET_TERMINAL_QUOTE_REFRESH_SECS`
  (default 60 seconds, configured range 5–3600), measured after a response.
  One request runs at a time and manual requests coalesce. Distinct provider
  observations still enforce debounce and duplicate suppression. Trigger-only
  transitions now enter the durable write queue and survive restart.
- Worksheet renames update parsed sheet-qualified references across the whole
  workbook, including ranges, quoted names and absolute addresses. String
  literals and unrelated formulas are preserved. Rename plus reference updates
  is one undoable operation; invalid names leave the workbook intact.
- News keeps a consistent feed snapshot and follows selected story IDs through
  refresh. An open reader retains its article even if it leaves the feed or is
  removed by the unread filter. Body extraction updates can refresh that same
  article without switching the reader to another story.
- Risk positions, Backtest fills and Fixed Income cash flows/shocks scroll to
  the selected record. Rendering and mouse selection share viewport geometry.
  Backtest and Fixed Income mouse fallbacks no longer recurse. Unsupported Risk
  arguments produce a visible usage error and preserve the current view.
- The supported JavaScript client preserves exact integers using `bigint` and
  rejects unsafe `number` input. The existing HTTP wire schema is unchanged.
  A Rust-generated large-option fixture and actual-host JavaScript replay cover
  the previously rounded result. See [Web Client Contracts](web-contracts.md).
- HTTP correlation headers and trace fields accept only bounded request IDs.
  Invalid IDs retain the engine's typed error without entering headers/logs.
  Artifact listing retains at most `limit + 1` summaries plus the document
  currently being validated, rather than all matching document bodies. It
  still validates the entire bounded catalog and preserves sort/filter/cursor
  behavior.
- CI's existing Clippy failures are fixed. JavaScript type checking, fixture
  replay and release-host integration now run in CI. Visual hashes were reviewed
  and updated for truthful shell chrome and visible Risk status.

## Verification

Regression coverage includes trigger persistence/restart and scheduled debounce,
sheet rename/undo/redo, feed reorder/removal while reading, long-table rendering
and mouse targeting, invalid Risk syntax, oversized correlation IDs, and ordered
pagination over large artifact bodies. The client tests cover all engine result
variants, signed/unsigned 64-bit limits, unsafe inputs and host versus engine
errors.

Hands-on checks confirmed current Apple financial periods, real UTC time,
the 400th quarterly cash flow with principal repayment, and a workbook formula
that remains `20` after its input sheet is renamed. The actual HTTP host matched
all five Rust fixtures through the JavaScript client, including the exact
`9998990001009999`-micros option result.
The release TUI also restored the live alert's saved `1/2` debounce state and
advanced its snapshot sequence automatically. The unchanged delayed quote was
correctly treated as a duplicate, with no fabricated second confirmation.

Completed checks: 638 workspace tests passed, including all 10 architecture
boundary checks and semantic/capability galleries; 22 live tests remain ignored.
Clippy passes for all targets/features with warnings denied. TypeScript checking,
eight client codec tests, fixture export consistency and real HTTP-host replay
pass. All workspace release targets build. All 11 performance scenarios pass
the 50 ms gate; the largest measured p95 was 5.253 ms (2,000-story sentiment
enrichment). These measurements cover the deterministic harness, not network
latency or unavailable authenticated providers.

## Next product milestone

Build one complete browser research workflow: **find an instrument → inspect
dated price/history and SEC research → open linked news → save and reopen the
research state**. This connects existing capabilities around a task instead of
creating a dashboard with disconnected demonstrations.

1. Define browser authentication/session ownership and tenant storage; preserve
   server-owned tenant identity and existing admission/work budgets.
2. Extract host-neutral operations and ports for this workflow under their owning
   bounded contexts. Keep deterministic calculations in the engine. Native and
   HTTP hosts select adapters at their composition roots; neither imports the
   other host's presentation.
3. Add authenticated search, quote/history, security and news read operations
   with explicit source/as-of/quality/error states and runtime wire validation.
4. Build browser navigation and the linked research workspace using the exact
   integer client. Add typed saved state and restart/reopen behavior on both
   hosts, with identical domain evidence for identical inputs.
5. Verify equivalent user scenarios in both hosts, including unavailable data,
   stale responses, denied access, narrow layouts and keyboard navigation.

Then extend parity through Portfolio/Risk, Screening, Backtesting and Spreadsheet
as complete workflows. Browser alerts require a durable worker lifecycle;
desktop polling does not provide evaluation after the app exits. Also retain
the backlog for linked Desk instruments, synchronous import/session persistence,
partial Security failures and startup-only Overview projections. This milestone
does not claim those gaps are solved.
