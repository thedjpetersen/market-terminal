# Local Research Artifact Store

`market-terminal-artifact-store` is the first concrete implementation of the
application-owned `ResearchArtifactQuery` port. It is intentionally read-only:
the API can list and retrieve verified research documents, but it cannot create,
replace, or delete them.

## Ownership and layout

The configured root is a private deployment directory, not a user-selected
path. On Unix it must be a real directory with no group or other permissions;
`0700` is the expected mode. The adapter canonicalizes it once at startup and
rejects root, tenant-directory, or document symlinks.

Raw tenant and artifact identities never become path components. Their UTF-8
bytes are lower-case hex encoded into this schema-v1 layout:

```text
<root>/
└── tenant-<hex tenant id>/
    └── artifact-<hex artifact id>.json
```

Each file is one `ResearchArtifactDocument` JSON object from
`market-terminal-application`. The document's tenant and artifact identities
must agree with its canonical directory and filename. A tenant catalog is
bounded at 4,096 JSON files, an individual document at 1 MiB, a page at 100
summaries, and cursors are the last visible artifact ID. Listing is
deterministic by artifact ID and remains stable if the cursor artifact is later
removed.

Non-JSON files are ignored so operators can keep filesystem metadata beside the
catalog. A malformed, oversized, misnamed, cross-tenant, or symlinked JSON entry
fails closed as a repository contract violation; it is never silently omitted.
I/O and permission failures map to service unavailable. Application services
then independently revalidate schema, ownership, requested kind, labels, digest
envelope, and response bounds before the HTTP layer can serialize a result.

This adapter assumes a single-owner local filesystem. A shared or hostile mount
needs an `openat`-style or database-backed adapter with equivalent tenant tests;
the port permits replacing this crate without changing application or engine
code.

## Production composition

Artifact routes remain absent by default. To mount them in the API binary,
configure both the private root and the exact read-only capability:

```bash
install -d -m 0700 /var/lib/market-terminal/artifacts
export MARKET_TERMINAL_API_ARTIFACT_ROOT=/var/lib/market-terminal/artifacts
export MARKET_TERMINAL_API_ARTIFACT_READ=1
cargo run -p market-terminal-api
```

Supplying only one variable, using a read flag other than `1`, pointing at a
symlink or non-directory, or exposing the Unix root to group/other users aborts
startup. The API logs only whether artifact reads are enabled, never the root or
document contents.

Document creation is deliberately outside the web process. A future ingestion
worker must validate and atomically promote complete artifacts into the same
layout; it must not add mutation routes to this read host.
