# Screening contract

Screening is a bounded context that evaluates saved, typed definitions against
one immutable point-in-time universe snapshot. It does not import Market Data or
Watchlist domain types. The composition root supplies a
`ScreeningUniverseQuery` adapter that translates a Watchlist-owned membership
set and one Market Data quote batch into Screening-owned values.

## Current P2/P3 slice

- Universe membership is capped at 2,000 unique canonical instrument IDs.
- Each snapshot carries a stable universe ID, deterministic input version,
  observation time, source/provider set, label, and member-level quality.
- Supported numeric fields are last price, percent change, volume, bid/ask
  spread in basis points, and intraday range as a percent of last price.
- A definition contains one to eight typed clauses combined with `AND`, one
  explicit sort field and direction, and a result limit from 1 to 200.
- Missing predicate or sort values fail closed. Coverage and every rejected
  member remain explicit; no value is filled, blended, or inferred.
- Ranking is deterministic. Equal sort values use canonical instrument identity
  as the final ascending tie-breaker.
- Every accepted row retains the actual value and pass/fail evidence for every
  clause. The selected row's evidence is visible in the terminal.
- Built-in screen IDs are protected. Up to 64 custom definitions are stored in
  a schema-versioned, crash-safe, private feature document.
- Evaluations run through a capacity-one worker. New requests supersede stale
  generations; the last valid result remains visible after a refresh failure.

This is an intentionally bounded first slice. It does not yet provide `OR` or
nested expression groups, dimension/unit inference beyond the closed numeric
field catalog, fundamental/factor fields, historical point-in-time storage,
whole-result atomic promotion, or a direct Chart action. Those remain P2/P3
roadmap work.

## Commands

Run a built-in or saved definition:

```text
SCREEN
SCREEN momentum
SCREEN RUN tight-spread
SCREEN LIST
SCREEN NEXT
SCREEN PREV
```

Create or replace a custom definition:

```text
SCREEN SAVE liquid-gainers core change_pct >= 1 AND volume >= 1000000 SORT change_pct DESC LIMIT 25
```

The grammar is deliberately closed:

```text
SCREEN SAVE <id> <universe> <field> <op> <number>
  [AND <field> <op> <number>]...
  [SORT <field> <ASC|DESC>]
  [LIMIT <1..200>]
```

Fields accept `last`, `change_pct`, `volume`, `spread_bps`, and
`day_range_pct`; comparisons accept `>`, `>=`, `<`, `<=`, and `=`. Delete only
custom definitions with `SCREEN DELETE <id>`.

## Terminal interaction

- `Up`/`Down` or `k`/`j`: move the stable row selection.
- `Enter` or `s`: open the selected instrument in Security.
- `a`: insert the selected symbol into Spreadsheet.
- `m`: open the source universe in Monitor.
- `[` / `]`: run the previous or next definition.
- `r`: refresh from a new versioned snapshot.
- `Esc`, arrows, `Enter`, mouse, and `F` hints use the same row/control geometry
  and revalidate the exact instrument identity before activation.

Saved views contain only the screen ID plus optional selected and top-visible
instrument IDs. They do not persist universe members, quotes, evaluation rows,
provider responses, or rank evidence. Restoration loads a new snapshot and
rematches those identities, reporting unavailable definitions or rows as
degraded instead of targeting a different instrument.

## Verification boundary

Deterministic tests cover stable tie-breaking, null failure, truncation,
multi-clause parsing, protected built-ins, stale actions, asynchronous saved-view
recovery, adapter translation, private persistence, and semantic frames at
80×24, 120×36, and 160×48. The release performance gate separately evaluates a
2,000-member, two-clause universe and enforces the repository-wide 50 ms p95
budget.
