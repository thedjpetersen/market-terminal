# Backtesting contract

The current `BACKTEST` workspace is the first bounded P6 vertical slice. It is a
research replay engine and artifact boundary, not a claim that the full P6
roadmap or a production execution simulator is complete.

## Timing and accounting

- The only current template is a long-only simple-moving-average crossover.
- A signal uses closes available through observation `t`; it is recorded at
  that bar's timestamp and may execute only at the next bar's open timestamp.
- Warm-up requires the complete slow window. There is no same-bar execution,
  forced final liquidation, shorting, leverage, fractional share, or future-bar
  access.
- Prices and cash are integer millionths of the reporting currency. Positions
  are whole shares. Each fill applies a symmetric all-in basis-point execution
  penalty plus a fixed commission, then cash and marked equity reconcile exactly.
- Open positions remain marked at the last close. Total return, maximum drawdown,
  turnover, trades, and the equity curve derive from the same ledger.

## Reproducibility and provenance

Backtesting owns `BacktestHistoryQuery`; the composition root translates the
Chart history port into Backtesting's own immutable bars. The feature never
imports Charting's model. Translation validates identity and finite positive
OHLC values, converts prices once, preserves source and quality, and creates a
content-derived input version.

Every successful artifact contains:

- schema and methodology version;
- canonical instrument ID and display symbol;
- source, quality, input version, observation bounds, and bar count;
- independent configuration and data digests;
- a run digest covering configuration, data, input version, final equity, and
  the ordered fill ledger;
- a second artifact digest covering the complete typed configuration, result
  metrics, decisions, fills, equity curve, methodology, and disclosures;
- every decision's observation and next execution timestamp;
- every fill's reference price, costed price, quantity, and commission;
- explicit omissions and a permanent research-only disclosure.

The engine is deterministic and has no clock, random generator, filesystem,
network, broker, portfolio, or shell dependency. A capacity-one workspace worker
coalesces requests, rejects stale generations, and retains the last valid artifact
when a provider or calculation fails.

Persistent deployments expose an independent immutable artifact port. `SAVE`
uses the run digest as the stable document identity, accepts an identical repeat
idempotently, and rejects different content under the same identity. Loading
revalidates schema, dimensions, configuration evidence, chronology, and the
complete artifact digest before exposing any result. The catalog is capped at 64
runs, each durable document remains within the platform's 1 MiB feature-document
limit, and deletion is always explicit. Larger valid in-memory runs remain
exportable but fail closed on `SAVE`. The catalog is separate from saved views
and from provider history storage.

## Commands and interaction

```text
BACKTEST <symbol> [FAST <2..499>] [SLOW <3..500>]
                   [COST <0..1000 bps>] [COMMISSION <0..10000>]
BT <symbol> ...
BACKTEST SAVE
BACKTEST LIST
BACKTEST OPEN <RUN-DIGEST>
BACKTEST DELETE <RUN-DIGEST>
BACKTEST EXPORT <PATH>
BACKTEST EXPORT! <PATH>
```

`EXPORT` writes deterministic, schema-versioned JSON with a trailing newline.
It preserves the complete verified artifact, uses private file permissions, and
refuses replacement; `EXPORT!` opts into crash-safe atomic replacement. Portable
JSON is capped at 8 MiB. Export
never fetches newer market data or silently reruns the strategy.

Use `1`/`2` or `Tab` for Summary and Trades, arrows or `j`/`k` in the fill audit,
`r`/`F9` to reproduce the current run, and `c` to open the source instrument in
Chart. Mouse, spatial focus, and follow hints expose the same tabs, rerun, and
Chart actions. Saved views retain only the instrument identity, bounded template
parameters, active subview, and selected audit row—not bars, trades, or results.

## Verification boundary

The checked-in suite proves that future-bar mutation cannot change earlier
decisions, signals precede fills, costed equity is below an otherwise identical
cost-free run, malformed histories fail closed, repeated inputs reproduce the
whole artifact, the composition translator is stable, saved views round-trip,
immutable storage is idempotent and corruption-detecting, portable export is
private and explicit-overwrite-only, and all three terminal sizes render. The
release performance gate runs 5,000 bars
100 times and verifies the same run digest under the global 50 ms p95 budget.

Still required for P6 includes session calendars, corporate actions, universe
membership, partial fills and impact, additional templates, benchmarks and tear
sheets, richer experiment tracking, sweeps, walk-forward/purged validation,
robustness, statistical research, governance, paper promotion, and input-bar
retention for exact offline reruns.
