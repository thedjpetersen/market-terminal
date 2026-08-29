# Fixed-income contract

`BOND`, `FI`, and `FIXEDINCOME` open a deterministic fixed-rate bullet-bond
reference model. The context owns its inputs, valuation engine, cash-flow
schedule, sensitivity measures, shock artifact, terminal presentation, and
saved-view schema. It does not import an equity quote model or imply that a
provider bond, curve, calendar, spread, or credit dataset is loaded.

## Command and conventions

```text
BOND <ID> <CCY> <FACE> <COUPON%> <YIELD%> <YEARS> <ANNUAL|SEMI|QUARTER> <ACCRUED%>
BOND UST-5Y-REFERENCE USD 100 4.5 4.25 5 SEMI 0
```

Inputs are bounded and atomic. Face value uses integer millionths internally;
coupon and nominal annual yield use integer basis points. Maturity is a positive
whole number of model years. Coupons are annual, semiannual, or quarterly. Yield
is nominal with compounding at the coupon frequency. `ACCRUED%` is the explicit
fraction of one coupon period already elapsed, from 0 up to but excluding 100.
The initial model therefore needs no hidden settlement date or day-count rule.

The model constructs every contractual coupon and terminal principal payment,
then discounts it at the periodic yield. Dirty price is the present value of the
remaining cash flows; accrued interest is coupon payment times the explicit
period fraction; clean price is dirty price less accrued interest. The artifact
also reports current yield, Macaulay and modified duration, convexity, and DV01.
A bounded bisection solver round-trips clean price to yield in domain tests.

The shock view recomputes clean and dirty prices at `-200`, `-100`, `-50`, `0`,
`+50`, `+100`, and `+200` basis points. These are deterministic parallel changes
to the single model yield, not a claim that a Treasury curve was observed.
Every result carries the model version, methodology, disclosures, and a digest
of all conventions-bearing inputs.

## State and failure behavior

Commands validate completely before replacing the last valid artifact. Saved
views retain typed model and presentation inputs, not derived cash flows or
analytics. Invalid, retired, or future saved fields degrade explicitly without
partially mutating a valid model. Mouse, keyboard, spatial focus, and follow
hints share the same tab and frequency geometry.

## Deliberate exclusions

This slice does not model dated schedules, settlement, holidays, business-day
rolls, ex-coupon rules, actual/actual or 30/360 day counts, irregular stubs,
floating coupons, calls, puts, sinks, defaults, taxes, inflation linkage, or
credit migration. It does not construct a live Treasury curve, calculate a
provider spread, or fetch a market price. Those require licensed reference and
market-data ports with stale, partial, unavailable, and entitlement states.

The remaining roadmap work is dated convention-aware schedules; price-input
yield workflows; Treasury and credit-curve construction; spread measures;
historical/inversion analysis; callable and floating-rate models; provider
provenance; and portfolio scenario links.
