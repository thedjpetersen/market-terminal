# Unified discovery contract

Unified discovery is the shell-owned, read-only directory of executable terminal
destinations. `DISCOVER [QUERY]` opens it in search mode; `HELP` and `F1` open
the same inventory in browse mode. The active workspace is not replaced until
the user inspects and explicitly runs a destination.

## Indexed destination classes

The directory merges four bounded projections:

| Kind | Authority | Executed through |
| --- | --- | --- |
| Workspace | Live `WorkspaceRegistry` descriptors | The workspace's canonical command |
| Command | Shell and feature Help catalogs | Exact `CommandInvocation` routing |
| Saved view | Shell-owned saved-view catalog | `VIEW RESTORE` with a quoted label |
| Launchpad object | Launchpad's read-only `Workspace::discovery_items` contribution | The tile's validated typed route |

The shell never imports Launchpad domain types. A workspace contribution contains
only a stable opaque ID, kind, display metadata, exact command, optional aliases,
and optional local identity/revision. The registry rejects empty or oversized
fields, commands that do not parse exactly, duplicate IDs, and inventory beyond
the global bound. Adding another feature-owned object class therefore does not
require a central enum of that feature's domain objects.

Provider observations, article bodies, portfolio values, workbook contents,
alert predicates, screen results, credentials, and terminal frames are not
indexed. Discovery reads already-loaded descriptors and local metadata and
performs no network or filesystem I/O while rendering or handling input.

## Matching and ordering

Queries are at most 128 UTF-8 bytes and split on whitespace. Matching is
case-insensitive and literal: every token must occur in at least one searchable
field. There is no edit-distance correction, hidden synonym service, or AI
dispatch. This keeps an incomplete or mistyped query from silently selecting an
unrelated command.

Each token is scored by its strongest match:

1. exact field match;
2. field prefix;
3. word prefix;
4. substring.

Labels and canonical command text outrank aliases, owner labels, and keywords.
For command destinations, an exact canonical command has no field penalty. Ties
break by destination kind, case-insensitive label, then stable ID. Empty queries
use the same deterministic kind/label/ID order. Results are capped at 128 from
a validated 256-item inventory.

This is ranking for presentation only. `Enter` executes the selected item's
stored exact command through the existing parser and workspace registry; search
text is never interpreted as a command.

## Interaction and mutation safety

- `/` enters search; `Backspace` edits and `Ctrl+U` clears the query.
- Arrows, `J`/`K`, Page Up/Down, Home/End, and the mouse wheel move within the
  bounded result set.
- The first `Enter` opens destination information. A second `Enter` executes the
  exact command. `Esc` returns from details, exits search, or closes the overlay
  one level at a time.
- `X` is available only for a selected saved view. The first press arms the
  exact saved-view ID and revision. The second press re-reads the current
  catalog and deletes only if that same pair still exists. A changed revision,
  changed selection, query edit, or navigation clears or replaces the arm.

Saved-view deletion uses the existing crash-safe catalog repository. It does not
delete workspace data, Launchpad objects, portfolio records, provider data, or
workbook content.

## Verification boundary

The release gate locks:

- deterministic ranking and all-token matching;
- UTF-8-safe query bounds and inventory/result limits;
- workspace contribution validation and deduplication;
- all four destination classes in one query surface;
- exact saved-view restore through the normal command router;
- durable two-step deletion and revision-crossing rejection;
- responsive semantic rendering at the standard terminal sizes; and
- an independent discovery-search p95 budget.

Any new discoverable feature object must publish a bounded, parseable projection
and add contract evidence. It must not widen the shell into that feature's domain
model or introduce provider work into the input/render path.
