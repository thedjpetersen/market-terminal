# Web Client Contracts

The browser boundary is versioned independently from the native terminal. The
checked-in contract package at `contracts/web/v3/` contains:

- `market-terminal-api.ts`: dependency-free TypeScript types for health,
  capabilities, problems, engine requests/results, and research artifacts;
- `engine-fixtures.json`: exact Rust-generated request/response pairs for every
  compiler-visible engine operation and result variant.

The fixtures are evidence, not illustrative snippets. Each request is parsed
back into `EngineRequest`, executed through the deterministic engine, and
compared field-for-field with its checked-in response. CI also compares the
operation, result, and HTTP problem-code registries with TypeScript
discriminators. Adding or renaming a Rust variant therefore fails until the web
contract is reviewed and intentionally updated.

## Review workflow

Check the current package:

```bash
cargo run -p market-terminal-tui --example export_web_contracts -- --check
cargo test -p market-terminal-tui --test web_contracts
```

After an intentional Rust wire change, regenerate the fixture corpus:

```bash
cargo run -p market-terminal-tui --example export_web_contracts -- --write
git diff -- contracts/web/v3
```

Never regenerate merely to make CI green. Review discriminator names, schema
versions, units, nullability, exact digest/provenance fields, and whether the
change is backward compatible. Breaking changes require a new API or engine
schema directory; existing versioned packages remain immutable for supported
clients.

## Numeric safety

Rust uses exact signed and unsigned integers throughout analytical contracts.
JSON represents them as numbers, while ordinary JavaScript numbers are exact
only through `Number.MAX_SAFE_INTEGER`. Valid engine results already exceed
that range: the `contract:option-large-integer` fixture produces a contract value
of `9998990001009999` micros. Checking after ordinary `JSON.parse` detects an
unsafe value but cannot recover the lost digits.

Use the supported client in `contracts/web/client/`. It reads response text
through the pinned `lossless-json` parser and represents **all** integers as
`bigint`, including schema versions and small counts. `Lossless<T>` maps the
v3 transport declarations into those runtime types. Its serializer emits exact
JSON integer tokens; the HTTP and engine wire versions remain unchanged.
Numeric strings remain strings. Fractional/exponent number tokens are rejected
because the research contracts contain integers, and unsafe JavaScript `number`
inputs are rejected before sending. Construct exact values using bigint literals
or `BigInt` of the original decimal string, never an already-rounded number.

```ts
import { executeEngine, type ResearchRequest } from "../contracts/web/client/index.ts";

const request: ResearchRequest = {
  schema_version: 1n,
  request_id: "research:option:1",
  operation: "price_option",
  input: {
    symbol: "AAPL", right: "call", spot_micros: 190000000n,
    strike_micros: 200000000n, days_to_expiry: 30n,
    volatility_bps: 2500n, risk_free_rate_bps: 500n,
    dividend_yield_bps: 0n, contract_multiplier: 100n,
  },
};
const result = await executeEngine("/v1/engine", request, {
  headers: { authorization: `Bearer ${token}` }, signal,
});
```

`executeEngine` checks schema, request correlation and result discriminators.
It returns typed engine errors and throws `ResearchHttpError` for host problems.
It does not validate every analytical field or replace server domain validation.
Credentials, cancellation and endpoint selection belong to the caller; the client
stores no credentials and performs no retries. Generic artifact consumers can use
`parseResearchJson`, which returns `unknown` for their own shape validation.

The client requires bigint support and a TypeScript-aware browser bundler. It has
no dependency on recent `JSON.parse` reviver extensions. Parser behavior is
documented in the [upstream lossless-json package](https://github.com/josdejong/lossless-json).

```bash
npm ci --prefix contracts/web/client
npm run typecheck --prefix contracts/web/client
npm test --prefix contracts/web/client
cargo build -p market-terminal-api
npm run test:http --prefix contracts/web/client
```

The last test launches an isolated loopback HTTP host with ephemeral credentials
and replays every Rust fixture through the JavaScript client. CI runs it against
the release binary. A future decimal-string wire representation requires a new
schema; do not change v1 field representation in place.

## Consumer boundary

The versioned TypeScript file describes transport data only. Browser sessions,
authorization decisions, provider access and full product workflows remain
separate work. The client preserves the server-owned tenant boundary.
