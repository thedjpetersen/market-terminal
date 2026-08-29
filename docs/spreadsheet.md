# Spreadsheet contract

Spreadsheet is the Stage 1 composition surface. It is a native Rust/Ratatui
workspace; calculation is deterministic and external I/O stays behind
Spreadsheet-owned ports and bounded workers.

## Supported limits

- 26 columns by 100 rows per sheet
- at least five sheets and 10,000 populated cells per persisted workbook
- 64 sheets as the document safety ceiling
- 10 MB per imported CSV
- 100 undo snapshots
- release-mode common-edit p95 below 50 ms at 10,000 populated cells

The CI performance gate seeds five 2,000-cell sheets, records 100 edits after a
warm-up, and fails when the measured 95th percentile exceeds the budget.

## Formula grammar

```text
formula      = ["="] expression
expression   = comparison
comparison   = additive { ("=" | "<>" | "<" | "<=" | ">" | ">=") additive }
additive     = product { ("+" | "-") product }
product      = unary { ("*" | "/") unary }
unary        = ["+" | "-"] primary
primary      = number | text | reference | range | function | "(" expression ")"
reference    = [sheet "!"] ["$"] column ["$"] row
range        = reference ":" reference
function     = name "(" [expression {"," expression}] ")"
text         = '"' { character | '""' } '"'
```

Sheet names containing spaces or punctuation use single quotes, for example
`='Base Case'!$B2`. Copy and fill translate relative axes and preserve `$`
absolute axes.

## Pure functions

The deterministic function set contains 27 functions:

```text
SUM AVERAGE MIN MAX COUNT COUNTA
IF IFERROR AND OR NOT
CONCAT LEN LOWER UPPER TRIM LEFT RIGHT
ABS ROUND POWER SQRT
XLOOKUP
DATE YEAR MONTH DAY
```

`DATE` returns an ISO `YYYY-MM-DD` text value; `YEAR`, `MONTH`, and `DAY` extract
parts from that stable representation. There is intentionally no wall-clock
function in the pure engine.

## Financial functions

```text
PX_LAST(instrument)
PX_CHANGE(instrument, period)
HISTORY(instrument, field, start, end)
FUNDAMENTAL(instrument, field, period)
```

Financial calls can be nested inside arithmetic, conditional, text, and lookup
formulas. The worker deduplicates requests, resolves them in a bounded batch,
and recalculates from a substituted AST without overwriting raw formula source.
Deterministic gallery fixtures cover all four contracts. In the interactive
app, `FUNDAMENTAL` always routes to official SEC Company Facts while quote and
history calls route to the selected market-data provider. Set
`MARKET_TERMINAL_MARKET_DATA_PROVIDER=alpha-vantage` for official daily
`HISTORY`; `ALPHA_VANTAGE_API_KEY` selects the operator's entitlement and the
documented demo access remains IBM-only.

`HISTORY` is a scalar function: it returns the latest daily observation inside
the inclusive `start` / `end` interval. Alpha Vantage supports `PX_OPEN`,
`PX_HIGH`, `PX_LOW`, `PX_LAST`, and `VOLUME`, with ISO `YYYY-MM-DD` dates.
`FUNDAMENTAL` supports SEC-reported `REVENUE`, `OPERATING_INCOME`, `NET_INCOME`,
and `DILUTED_EPS` for `FY####` or `FY####A`; currency facts remain raw USD and
EPS remains USD/share. Missing observations are unavailable rather than
forward-filled or synthesized. Other live adapters may return unavailable or
permission denied when a field is not licensed.

Every external input retains instrument, field, provider, observation time,
receive time, and quality. Cells distinguish loading, stale, unavailable, and
permission-denied states; failures propagate only to dependent cells.

## Workbook and composition commands

```text
SHEET ADD <sheet-name>       SHEET RENAME <sheet-name>
SHEET SELECT <sheet-name>    SHEET DELETE
SHEET IMPORT <file.csv>      SHEET EXPORT[!] <file.csv>
SHEET SAVE [workbook-id]     SHEET LOAD [workbook-id]
SHEET LIST                   SHEET DROP <workbook-id>
SHEET FIND|MON|SEC|CHART|NEWS
SHEET INSERT <value>
```

Persistent mode loads `default` at startup and autosaves successful mutations.
Workbook payloads are schema-versioned, bounded, written atomically, and retain
a previous valid generation for recovery.

`VIEW SAVE <name>` captures Spreadsheet navigation separately from that workbook
payload: workbook ID, worksheet name and ordinal, selected cell, and the first
visible row and column. `VIEW RESTORE <name>` can therefore reopen a different
persisted workbook and return to the same editing location after restart. The
worksheet name is authoritative; its saved ordinal is an explicit degraded
fallback when a tab was renamed. A missing workbook, removed tab, invalid cell,
future field, or out-of-bounds viewport is reported as `DEGRADED`, and valid
fields continue restoring. Saved views never embed cell contents, an uncommitted
formula draft, clipboard data, or undo/redo history.

The research commands send the selected non-formula text cell through
`AppIntent::DispatchCommand`. Find, Monitor, Security, Chart, and News send a
selected instrument back with `SHEET INSERT` when the user presses `A`. This is
kernel routing rather than direct feature-to-feature coupling.

## Interaction contract

`Esc` lifts focus to the selected cell. Spatial arrows and `Enter`, or `F` and a
generated one- or two-letter label, address visible cells and row headers, the
formula bar, complete worksheet tabs, and the workflow controls. The two-row
control pack wraps only whole controls at narrow sizes and includes edit, clear,
copy/paste, fill down/right, undo/redo, Security, Chart, News, and financial
refresh. Disabled operations are visible but excluded from shell routing.

Rendering, pointer input, focus, and follow hints use one geometry model. Cell
actions include the active worksheet digest and address; worksheet actions carry
their current index and name digest. Activation revalidates those identities and
the current viewport, edit mode, clipboard sheet, history stacks, source cells,
and selected instrument. Stale actions therefore fail closed instead of applying
to a renamed sheet or a cell that moved outside the viewport.
