# Options reference-model contract

`OPTIONS` is the first bounded P5 derivatives slice. It is a transparent,
user-input European option calculator and deterministic scenario surface. It is
not an option-chain feed, executable strategy ticket, provider quote, or trading
recommendation.

```text
OPTIONS AAPL CALL 190 200 30 25 5 0 100
OPTIONS MSFT PUT 420 400 45 30 4.5 0.8 100
```

Arguments are symbol, right, spot, strike, calendar days, annual volatility
percent, continuously compounded annual risk-free-rate percent, continuously
compounded annual dividend-yield percent, and optional contract multiplier.
Inputs are validated and applied atomically. Prices use integer millionths at
the workspace boundary; the pure model uses floating-point transcendental math
internally and rounds once on publication.

The model is `BLACK-SCHOLES-EUROPEAN-V1`, with ACT/365 time and continuous rates
and dividends. It publishes model price, intrinsic and time value, delta, gamma,
vega per one volatility percentage point, theta per calendar day, and rho per
one rate percentage point. Expiry is an explicit intrinsic-value boundary.
Independent reference values, put-call parity, expiry behavior, malformed input,
multiplier preservation, and deterministic replay are locked by tests.

The scenario view is a 5×3 grid of spot shocks (-20%, -10%, 0%, +10%, +20%) and
volatility shifts (-5, 0, +5 percentage points). It holds time, rates, and
dividends constant and preserves the entered contract multiplier. Every result
has a model version and deterministic input digest. Saved views contain only the
typed user inputs, active subview, and selected scenario.

The terminal deliberately says `MODEL ONLY · NO CHAIN`. Provider price, bid/ask,
open interest, volume, implied volatility, venue/calendar data, and provider
Greeks are absent rather than synthesized. European Black-Scholes does not model
early exercise or discrete dividends. P5 still requires contract identity,
licensed chain adapters and degraded states, provider/model Greek separation,
volatility surfaces, flow/OI analytics, and multi-leg payoff construction.
