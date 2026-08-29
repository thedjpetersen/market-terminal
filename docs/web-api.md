# Market Terminal Web API

`market-terminal-api` is the first web host for the extracted analytical engine.
It is a separate Cargo package that enters through
`market-terminal-application`, the host-neutral tenant/capability/budget and
read-only artifact-query layer,
which alone dispatches `market-terminal-engine`. Neither crate links the native
terminal, feature workspaces, provider adapters, or local persistence.

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
| `MARKET_TERMINAL_API_TENANT` | `local` | Stable 1-64 character tenant identity assigned by the server to this credential. |
| `MARKET_TERMINAL_API_PRINCIPAL` | `api` | Stable 1-64 character audit actor assigned by the server to this credential. |
| `MARKET_TERMINAL_API_OPERATIONS` | all four | Comma-separated exact names from `run_backtest`, `compare_backtests`, `price_option`, and `analyze_bond`. At least one is required. |
| `MARKET_TERMINAL_API_MAX_BACKTEST_BARS` | `20000` | Per-principal bar ceiling, inclusive range 1-20000. |
| `MARKET_TERMINAL_API_MAX_COMPARISON_POINTS` | `120000` | Per-principal combined decision/trade/equity ceiling, inclusive range 1-120000. |
| `RUST_LOG` | subscriber default | Standard tracing filter; request bodies and bearer tokens are never logged. |

The process handles Ctrl-C and SIGTERM with graceful Axum shutdown.

## Routes

### `GET /healthz`

Public and intentionally minimal:

```json
{"status":"ok","api_schema_version":2,"application_schema_version":2,"engine_schema_version":1}
```

### `GET /v1/capabilities`

Requires `Authorization: Bearer <token>`. Returns the API, application, and
engine schemas; the server-owned tenant/principal identity; exact enabled
operation names; request-body limit; and per-principal analytical workload
ceilings. `artifact_operations` is empty unless a host explicitly grants
`read_research_artifacts`. It discloses no secret, provider, account, or
persistence state.

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

### Injectable read-only artifact routes

`router_with_artifact_query` lets a web composition inject an implementation of
the application-owned `ResearchArtifactQuery` port. It adds authenticated
`GET /v1/artifacts?kind=<kind>&cursor=<last_artifact_id>&limit=<1-100>` and
`GET /v1/artifacts/{artifact_id}` routes. Supported kinds are `backtest_run`,
`backtest_comparison`, `screen_result`, `news_snapshot`, and
`security_research`.

The request never contains a tenant field. Both repository keys are constructed
from the server-owned authenticated context, and returned pages/documents are
revalidated for tenant, schema, kind, bounded identity, provenance, digest
envelope, and size. A cross-tenant ID therefore returns the same generic 404 as
a missing ID. The default binary uses `router`, so these routes do not exist
until a reviewed adapter and explicit read capability are composed by a host.

## Failure contract

| Status | Meaning |
|---:|---|
| 400 | Malformed JSON, unsupported engine schema, invalid request identity/provenance, or invalid artifact query bounds. |
| 401 | Missing or incorrect bearer token. |
| 403 | Operation is valid but the authenticated principal lacks its capability. |
| 404 | Unknown route or unavailable artifact; cross-tenant ownership is never disclosed. |
| 413 | Body exceeded the configured limit before deserialization. |
| 415 | Content type is not JSON. |
| 422 | A syntactically valid operation failed its principal workload budget or domain validation. |
| 502 | An artifact adapter violated the application contract; its conflicting data is not returned. |
| 503 | The configured artifact adapter is unavailable. |

Responses set `Cache-Control: no-store`, `X-Content-Type-Options: nosniff`, and
`Referrer-Policy: no-referrer`. Unauthorized responses additionally advertise a
Bearer challenge. CORS is absent by design; a browser deployment must add an
explicit origin allowlist at a reviewed gateway or future host middleware.

## Boundary and remaining work

The host can run only the four closed deterministic engine operations. Bearer
authentication resolves to a validated server-owned tenant/principal context;
clients cannot submit or replace that identity. Application services reject
missing capabilities and over-budget work before engine dispatch. The default
binary cannot execute terminal commands, read environment-selected provider
credentials, load or save user artifacts, inspect portfolios, or mutate external
state. The API library's optional artifact surface is read-only and adapter-
injected; it cannot save or delete. `tests/architecture_boundaries.rs` enforces
`API -> application -> engine` and rejects native package, feature,
infrastructure, provider-client, terminal,
runtime, clock, environment, filesystem, and network boundary violations.

Before a multi-user web launch, replace the single configured credential mapping
with an encrypted credential/session store, implement tenant-owned repository
adapters with integration isolation tests, and add aggregate
rate accounting, deadlines, metrics/distributed tracing, audited read-only
provider/persistence services, TLS termination, and cross-language contract
fixtures. Those belong around the engine, never inside its deterministic domains.
