# Market Terminal Web API

`market-terminal-api` is the first web host for the extracted analytical engine.
It is a separate Cargo package that depends directly on
`market-terminal-engine`; it does not link the native terminal, feature
workspaces, provider adapters, or local persistence.

## Run locally

Generate a unique secret of at least 32 visible ASCII characters, then start the
host:

```bash
export MARKET_TERMINAL_API_TOKEN="replace-with-a-random-token-at-least-32-characters"
cargo run -p market-terminal-api
```

The default listener is `127.0.0.1:8080`. The process refuses a non-loopback
address unless `MARKET_TERMINAL_API_ALLOW_REMOTE=1` is explicit. Remote binding
does not add TLS: deploy it only behind a trusted TLS reverse proxy that does not
log the `Authorization` header.

Optional configuration:

| Variable | Default | Contract |
|---|---:|---|
| `MARKET_TERMINAL_API_BIND` | `127.0.0.1:8080` | Socket address; remote addresses require explicit opt-in. |
| `MARKET_TERMINAL_API_MAX_BODY_BYTES` | `4194304` | Inclusive range 1024-8388608; enforced before JSON deserialization. |
| `MARKET_TERMINAL_API_OPERATIONS` | all four | Comma-separated exact names from `run_backtest`, `compare_backtests`, `price_option`, and `analyze_bond`. At least one is required. |
| `RUST_LOG` | subscriber default | Standard tracing filter; request bodies and bearer tokens are never logged. |

The process handles Ctrl-C and SIGTERM with graceful Axum shutdown.

## Routes

### `GET /healthz`

Public and intentionally minimal:

```json
{"status":"ok","api_schema_version":1,"engine_schema_version":1}
```

### `GET /v1/capabilities`

Requires `Authorization: Bearer <token>`. Returns the engine schema, exact
enabled operation names, and request-body limit. It discloses no secret,
provider, account, or persistence state.

### `POST /v1/engine`

Requires the bearer header and `Content-Type: application/json`. Example:

```json
{
  "schema_version": 1,
  "request_id": "web:options:42",
  "operation": "price_option",
  "input": {
    "symbol": "AAPL",
    "right": "call",
    "spot_micros": 190000000,
    "strike_micros": 200000000,
    "days_to_expiry": 30,
    "volatility_bps": 2500,
    "risk_free_rate_bps": 500,
    "dividend_yield_bps": 0,
    "contract_multiplier": 100
  }
}
```

A valid response retains the request ID in both JSON and `x-request-id`, and
flattens the tagged engine result:

```json
{
  "schema_version": 1,
  "request_id": "web:options:42",
  "status": "ok",
  "result_type": "option_analytics",
  "data": {"model_version":"BLACK-SCHOLES-EUROPEAN-V1"}
}
```

The abbreviated `data` above illustrates the envelope; production responses
contain the complete typed analytic artifact and disclosures.

## Failure contract

| Status | Meaning |
|---:|---|
| 400 | Malformed JSON, unsupported engine schema, invalid request identity, or invalid provenance envelope. |
| 401 | Missing or incorrect bearer token. |
| 403 | Operation is valid but disabled by deployment policy. |
| 404 | Unknown route. |
| 413 | Body exceeded the configured limit before deserialization. |
| 415 | Content type is not JSON. |
| 422 | A syntactically valid operation failed domain validation. |

Responses set `Cache-Control: no-store`, `X-Content-Type-Options: nosniff`, and
`Referrer-Policy: no-referrer`. Unauthorized responses additionally advertise a
Bearer challenge. CORS is absent by design; a browser deployment must add an
explicit origin allowlist at a reviewed gateway or future host middleware.

## Boundary and remaining work

The host can run only the four closed deterministic engine operations. It cannot
execute terminal commands, read environment-selected provider credentials,
load or save user artifacts, inspect portfolios, or mutate external state.
`tests/architecture_boundaries.rs` rejects imports from the native package,
feature contexts, infrastructure adapters, provider clients, and terminal UI.

Before a multi-user web launch, add a tenant-aware identity and authorization
service, per-principal quotas, deadlines, rate limiting, metrics/distributed
tracing, audited provider/persistence application services, TLS termination, and
cross-language contract fixtures. Those belong around the engine, never inside
its deterministic domains.
