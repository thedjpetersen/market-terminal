# Market Terminal Engine

`market-terminal-engine` is the reusable analytical core shared by the native
terminal and future web, API, worker, queue, and WebAssembly hosts. It is a leaf
crate: hosts depend on it, while it never depends on a host.

## Extracted surface

The first extraction contains three stable deterministic domains:

- Backtesting: validated replay, evidence-bound artifacts, and exact-input
  paired comparison.
- Options: European Black-Scholes reference price, Greeks, and scenarios.
- Fixed Income: fixed-rate bullet cash flows, price/yield, accrued interest,
  duration, convexity, DV01, and parallel shocks.

The native feature paths remain source compatible through the
`market-terminal-tui` package's `market_terminal` library. For example,
`market_terminal::features::options::OptionModelInput` is the same type owned by
`market_terminal_engine::options::OptionModelInput`; it is re-exported rather
than copied or translated.

## Host-neutral API

The crate exposes direct typed functions and a transport-neutral command API:

```rust
use market_terminal_engine::{execute, EngineOperation, EngineRequest};
use market_terminal_engine::options::OptionModelInput;

let response = execute(EngineRequest {
    schema_version: 1,
    request_id: "web:options:42".to_owned(),
    operation: EngineOperation::PriceOption(OptionModelInput::default()),
});
```

`EngineRequest` is serde-deserializable. `EngineResponse` is serde-serializable
and returns a tagged typed result or a stable error code. Schema version and
request identity are validated before dispatch. The operation set is closed:
arbitrary commands, file paths, provider names, shell intents, and executable
code cannot cross this boundary.

The current operations are:

- `run_backtest`
- `compare_backtests`
- `price_option`
- `analyze_bond`

All execution is synchronous, deterministic, and side-effect free. A host owns
concurrency, cancellation, authentication, authorization, request deadlines,
rate limits, tracing, caching, and transport status mapping.

## Dependency and security rules

The engine may use deterministic host-neutral libraries such as serde. It may
not import:

- terminal or rendering libraries;
- async runtimes or HTTP clients;
- filesystems, sockets, process environment, or clocks;
- native application-shell state;
- concrete market-data or persistence adapters.

The API validates domain bounds, but an HTTP host must reject oversized bodies
before JSON deserialization. Provider retrieval and durable artifact access stay
behind feature-owned ports. The host passes already-authorized, versioned inputs
into the engine and persists only validated outputs. This prevents a future web
router from becoming a business-logic or data-access bypass.

`crates/market-terminal-tui/tests/architecture_boundaries.rs` enforces the
dependency rules and verifies that native Backtesting, Options, and Fixed
Income domains remain thin engine facades. Engine tests lock request round
trips, serialized typed responses, schema rejection, bounded
identities/provenance, and fail-closed domain errors.

## Web expansion sequence

1. **Delivered:** `market-terminal-application` is the sole analytical use-case
   boundary. It maps validated tenant/principal identity and exact capabilities
   to the closed operation enum, applies per-principal backtest/comparison
   budgets, and then dispatches the engine without owning I/O.
2. **Partially delivered:** `market-terminal-api` authenticates independently
   scoped actors through an injected host-neutral resolver and owns
   pre-deserialization body limits, typed status mapping, structured
   actor/request logging, safe response headers, loopback defaults, and graceful
   shutdown. A private digest-only catalog is the first resolver adapter. Add
   interactive sessions and hot revocation, shared admission, metrics, and
   distributed traces before horizontally scaled browser deployment. The first
   process-local aggregate admission, response deadlines, and bounded blocking
   execution now live in the HTTP host rather than this engine.
3. Expose provider and persistence capabilities through application services,
   never by teaching the engine to perform I/O.
4. Keep the checked-in TypeScript request/response package and exact
   cross-language golden fixtures synchronized with the compiler-visible engine
   registry. A future SDK adds runtime decoding without moving transport policy
   into this crate.
5. Extract another domain only after its vocabulary and invariants are stable;
   keep presentation state, local adapters, and shell navigation in their hosts.

This sequence preserves a modular monolith while making the core genuinely
multi-host. A web product can therefore reuse the same evidence digests,
validation, calculations, and disclosures without coupling its deployment model
to the terminal.
