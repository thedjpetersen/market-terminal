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
only through `Number.MAX_SAFE_INTEGER`. The TypeScript package calls these
fields `JsonInteger` and requires clients to reject any decoded value for which
`Number.isSafeInteger` is false. Silent rounding of money, timestamps, digests'
source values, quantities, or comparison evidence is a contract violation.

Before expanding valid engine bounds beyond the safe JSON range, introduce a
new wire schema that encodes affected integers as decimal strings and supplies
an explicit `bigint` decoder. Do not change v1 field representation in place.

## Consumer boundary

The TypeScript file describes transport data only. It does not contain bearer
credentials, session storage, HTTP retry behavior, authorization decisions, or
provider access. A future web SDK should wrap it with runtime validation against
the checked-in fixtures and preserve server-owned tenant identity.
