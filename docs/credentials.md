# API Credential Catalog

The production API can resolve multiple independently authorized actors from a
private, read-only JSON catalog. The catalog contains SHA-256 token digests,
never bearer-token plaintext. Authentication produces the same validated
tenant, principal, capability, and budget context used by application services;
the engine never sees credentials.

## Catalog contract

Set `MARKET_TERMINAL_API_CREDENTIALS_FILE` to a regular file owned by the API
operator. On Unix the file must not grant group or other permissions (mode
`0600` is recommended), and symbolic links are rejected. The startup snapshot
is bounded to 1 MiB and 256 records.

```json
{
  "schema_version": 1,
  "credentials": [
    {
      "credential_id": "research-web-1",
      "token_sha256": "2f1f7b6efc0323cddef999b051d568791f554c1f3c9ff12ccbc475e695f091f0",
      "tenant_id": "tenant-acme",
      "principal_id": "analyst-jules",
      "status": "active",
      "not_before_epoch_seconds": null,
      "expires_at_epoch_seconds": 1819670400,
      "operations": ["run_backtest", "compare_backtests"],
      "artifact_read": true,
      "max_backtest_bars": 10000,
      "max_comparison_points": 50000
    }
  ]
}
```

Generate a random bearer token, keep that token in the client secret store, and
put only its lowercase digest in the catalog. For example:

```bash
TOKEN="$(openssl rand -base64 48 | tr -d '\n')"
printf '%s' "$TOKEN" | sha256sum
chmod 600 /etc/market-terminal/credentials.json
```

Record IDs, tenant IDs, and principal IDs are bounded stable identities.
`operations` must contain one or more exact engine operation names. Budgets must
remain within the application limits. Unknown fields, duplicate IDs, duplicate
digests, uppercase or malformed digests, invalid identities, empty capabilities,
and invalid validity windows fail process startup.

`status` is either `active` or `revoked`. Optional validity timestamps are Unix
epoch seconds; `not_before_epoch_seconds` is inclusive and
`expires_at_epoch_seconds` is exclusive. Unknown, revoked, not-yet-valid, and
expired tokens all produce the same `401` response. The resolver scans the
bounded catalog and compares fixed-length digests without an early return.

The catalog is an immutable startup snapshot. Issuance, policy changes, and
revocation take effect after a controlled restart or rolling replacement. This
keeps request authentication read-only and deterministic; service-backed hot
revocation and interactive browser sessions remain separate future adapters.

## Production composition

```bash
MARKET_TERMINAL_API_CREDENTIALS_FILE=/etc/market-terminal/credentials.json \
MARKET_TERMINAL_API_ARTIFACT_ROOT=/srv/market-terminal/artifacts \
cargo run -p market-terminal-api --release
```

In catalog mode, each record owns its `artifact_read` capability. Supplying the
legacy global `MARKET_TERMINAL_API_ARTIFACT_READ` flag or any single-credential
identity, operation, or budget variable is an ambiguous configuration and fails
startup. Artifact routes are mounted only when an artifact root is configured;
the application layer still checks each resolved actor independently.

The legacy `MARKET_TERMINAL_API_TOKEN` mode remains for one-process local
development. It stores the token only in memory, maps it to one actor, and must
not be combined with the credential catalog.
