# Launchpad contract

Launchpad is a bounded, durable routing surface. It does not own instruments,
screens, portfolios, workbooks, or layouts; it stores the minimum typed identity
needed to route to the context that owns each object. The owning context still
revalidates the destination at activation time.

## Target types

| Type | Stored identity | Generated route | Failure boundary |
| --- | --- | --- | --- |
| `COMMAND` | Exact validated command | The command unchanged | Workspace command registry |
| `INSTRUMENT` | Canonical ID, display symbol, workspace command | `<WORKSPACE> <SYMBOL>` | Instrument parser and destination workspace |
| `SCREEN` | Stable screen ID and exact command | The screen command | Screen-owning workspace |
| `PORTFOLIO` | Stable portfolio ID and optional view | `PORT [VIEW]` | Portfolio workspace; the current product has one imported portfolio context |
| `SHEET` | Durable workbook ID | `SHEET LOAD "<ID>"` | Spreadsheet workbook repository |
| `LAYOUT` | Saved-view ID or name | `VIEW RESTORE "<REFERENCE>"` | Shell saved-view catalog and feature-owned restore contract |

Arguments that may contain spaces are quoted and escaped by the domain model;
tiles do not concatenate unvalidated shell fragments. A tile action carries a
digest of the complete typed target, not only its generated command, so changing
canonical metadata invalidates an already rendered mouse, arrow-focus, or
follow-hint destination.

## Editing commands

```text
LAUNCH ADD <LABEL> <COMMAND...>
LAUNCH ADD COMMAND <LABEL> <COMMAND...>
LAUNCH ADD INSTRUMENT <LABEL> <CANONICAL_ID> <SYMBOL> <WORKSPACE>
LAUNCH ADD SCREEN <LABEL> <SCREEN_ID> <COMMAND...>
LAUNCH ADD PORTFOLIO <LABEL> <PORTFOLIO_ID> [VIEW]
LAUNCH ADD SHEET <LABEL> <WORKBOOK_ID>
LAUNCH ADD LAYOUT <LABEL> <SAVED_VIEW_ID_OR_NAME>
LAUNCH RENAME <TILE_NUMBER> <LABEL>
LAUNCH MOVE <FROM> <TO>
LAUNCH REMOVE <TILE_NUMBER>
LAUNCH RESET CONFIRM
```

The live state is limited to 24 tiles. Labels are limited to 48 bytes, commands
to 512 bytes, and object identities to 128 bytes. IDs are local, positive,
monotonic values; order changes and renames preserve them. Every successful
mutation increments the document revision and queues a capacity-one background
save. Persistence never runs on the input or render path.

## Portable JSON

```text
LAUNCH EXPORT <PATH>
LAUNCH EXPORT! <PATH>
LAUNCH IMPORT <PATH>
LAUNCH IMPORT! <PATH>
```

The portable document uses schema `market-terminal.launchpad`, version `1`, and
a 64 KiB limit. It includes only ordered labels and typed targets. Machine-local
tile IDs, `next_id`, and document revision are deliberately omitted.

`EXPORT` uses create-new semantics and refuses to overwrite. `EXPORT!` writes a
private temporary file, synchronizes it, and atomically replaces the requested
file. Both forms create private Unix files (`0600`). `~` expands to the user's
home directory; no provider or network access is involved.

`IMPORT` validates the complete document before mutation, preserves the current
order, appends new definitions, and skips exact label-plus-target duplicates.
The capacity check is atomic: an over-capacity merge leaves the live document
unchanged. Repeating the same merge is idempotent. `IMPORT!` replaces the order
and contents only after full validation; exact matching definitions retain their
local IDs, while new definitions receive fresh monotonic IDs. Empty replacement
is valid and explicit.

## Persistence and migration

Local state schema version 2 stores typed targets. The decoder accepts schema
version 1 command-only tiles and translates each command into a `COMMAND` target
without changing its ID, label, order, revision, or `next_id`. Unsupported future
versions, duplicate IDs, invalid text, and corrupt JSON fail closed. Startup
discloses the failure and uses versioned seeds rather than partially applying a
document.

Portable files and local state are separate failure domains: importing never
changes the configured persistence path, exporting never exposes local identity,
and a failed file operation does not mutate Launchpad state.

## Verification contract

Tests lock all six target routes, v1 migration, bounded atomic merge,
idempotency, explicit replace, stable matching IDs, private create-new and atomic
overwrite behavior, durable restart, stale-action rejection, keyboard and `F`
routing, and saved-layout restoration through the shell router. The semantic
gallery covers the typed grid at 80×24, 120×36, and 160×48; the screenshot
gallery includes a full-color 160×48 Launchpad frame.
