# L00371 — desktop/mobile and keyboard verification

## Scope and decisions

- Source baseline: `afb01cacb17a5d3a1e652e534191298a3e106564`.
- Branch: `factory/l00371-v1-complete-permitted-desktop-mobile-and`.
- Read the completed L00309 audit from Git commit
  `8b223c9dd85acf030de2cbf4a1859483376c7220`; it is not in this branch's ancestry.
- Execute the unchanged deterministic workflows, preserving assertions. Temporary
  copies may adjust ports to 41999–42001 to obey Factory policy.
- Record rendered evidence and focused probes; product fixes for F1/F2/F3 belong
  to L00372/L00373/L00374. No live provider, production account or deployment use.
- Use L00309's Bloomberg density / Linear keyboard clarity criteria in DESIGN.md.

## Done

- Read repository instructions, READMEs, browser harness and upstream audit.
- Confirmed clean assigned worktree and identified missing upstream audit docs.

## Next

- Install locked dependencies, build WASM/frontend, run ordered baseline checks.
- Run deterministic browser workflows and focused desktop/mobile probes.
- Retain evidence, obtain independent verification, commit/push/open a PR.

## Surprises

- PROGRESS.md and features.json were absent; this task creates its own checklist.
- The original browser host uses ports outside the Factory allowed range.
