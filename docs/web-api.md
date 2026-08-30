# Market Terminal Web API

`market-terminal-api` is the first web host for the extracted analytical engine.
It is a separate Cargo package that enters through
`market-terminal-application`, the host-neutral tenant/capability/budget and
read-only artifact-query layer,
which alone dispatches `market-terminal-engine`. Neither crate links the native
terminal, feature workspaces, provider adapters, or local persistence.

## Run

Production hosts load a bounded private catalog of digest-only credentials:

```bash
export MARKET_TERMINAL_API_CREDENTIALS_FILE=/etc/market-terminal/credentials.json
cargo run -p market-terminal-api
```

See [`credentials.md`](credentials.md) for the schema, private-file contract,
token issuance, validity windows, and rotation semantics. The legacy
`MARKET_TERMINAL_API_TOKEN` mode remains available for a single local
development actor. It cannot be combined with a catalog.

The default listener is `127.0.0.1:8080`. The process refuses a non-loopback
address unless `MARKET_TERMINAL_API_ALLOW_REMOTE=1` is explicit. Remote binding
does not add TLS: deploy it only behind a trusted TLS reverse proxy that does not
log the `Authorization` header.

Optional configuration:

| Variable | Default | Contract |
|---|---:|---|
| `MARKET_TERMINAL_API_BIND` | `127.0.0.1:8080` | Socket address; remote addresses require explicit opt-in. |
| `MARKET_TERMINAL_API_MAX_BODY_BYTES` | `4194304` | Inclusive range 1024-8388608; enforced before JSON deserialization. |
| `MARKET_TERMINAL_API_CREDENTIALS_FILE` | unset | Private, regular, symlink-free digest-only credential catalog. When set, legacy actor variables are rejected. |
| `MARKET_TERMINAL_API_ARTIFACT_ROOT` | unset | Private local research-artifact directory. Routes are absent when unset; catalog records independently grant access. |
| `RUST_LOG` | subscriber default | Standard tracing filter; request bodies and bearer tokens are never logged. |

Single-credential development mode instead requires
`MARKET_TERMINAL_API_TOKEN`; it may optionally use
`MARKET_TERMINAL_API_TENANT`, `MARKET_TERMINAL_API_PRINCIPAL`,
`MARKET_TERMINAL_API_OPERATIONS`, `MARKET_TERMINAL_API_MAX_BACKTEST_BARS`, and
`MARKET_TERMINAL_API_MAX_COMPARISON_POINTS`. Its artifact root additionally
requires `MARKET_TERMINAL_API_ARTIFACT_READ=1`. These actor-policy variables
fail startup in catalog mode so there is only one authority for each request.

The process handles Ctrl-C and SIGTERM with graceful Axum shutdown.

## Routes

### `GET /healthz`

Public and intentionally minimal:

```json
{"status":"ok","api_schema_version":2,"application_schema_version":2,"engine_schema_version":1}
```

### `GET /v1/capabilities`

Requires `Authorization: Bearer <token>`. The injected resolver selects one
credential record and returns the API, application, and engine schemas; its
server-owned tenant/principal identity; exact enabled
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
a missing ID. In catalog mode the production binary composes the local read-only
adapter when `MARKET_TERMINAL_API_ARTIFACT_ROOT` is set, while each credential's
`artifact_read` value independently gates access. Otherwise these routes do not
exist. The adapter uses private, symlink-free, hex-keyed tenant directories and
bounded documents as specified in [`artifact-store.md`](artifact-store.md).

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
| 503 | The configured credential resolver or artifact adapter is unavailable. |

Responses set `Cache-Control: no-store`, `X-Content-Type-Options: nosniff`, and
`Referrer-Policy: no-referrer`. Unauthorized responses additionally advertise a
Bearer challenge. CORS is absent by design; a browser deployment must add an
explicit origin allowlist at a reviewed gateway or future host middleware.

## Boundary and remaining work

The host can run only the four closed deterministic engine operations. Its
reusable router depends on the host-neutral `CredentialResolver`, not a concrete
store. Bearer authentication resolves to a validated server-owned
tenant/principal context;
clients cannot submit or replace that identity. Application services reject
missing capabilities and over-budget work before engine dispatch. The default
binary cannot execute terminal commands, read environment-selected provider
credentials, inspect portfolios, or mutate external state. With default
configuration it cannot load user artifacts; with the explicit local adapter it
can only list and retrieve the authenticated tenant's documents. The reusable
API library remains adapter-injected and cannot save or delete. The binary's
concrete store is selected only at the composition root and refuses insecure
Unix roots or symlinked catalog entries. `tests/architecture_boundaries.rs`
enforces `API library -> auth/application ports <- adapters` and rejects native
package, feature, provider-client, terminal, runtime, and network boundary
violations in reusable layers.

Before a browser launch, add interactive password/OIDC and cookie-session
issuance, CSRF/origin policy, and a service-backed resolver with hot revocation;
the catalog is deliberately a restart-applied machine-credential snapshot. Also
add transactional ingestion and service-backed tenant storage for horizontal
deployment, aggregate rate accounting, deadlines, metrics/distributed tracing,
audited read-only provider/persistence services, TLS termination, and
cross-language contract fixtures. Those belong around the engine, never inside
its deterministic domains.
