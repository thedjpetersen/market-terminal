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
- Committed initial scope/checklist as `6b15498`.
- Locked installs, production WASM/frontend build, ordered web/client TypeScript,
  9 web tests, 8 client tests and 6 HTTP-only Playwright checks passed.
- Added [port-only reproduction helper](docs/audits/l00371/reproduce.py) and
  verified copies preserve original tests, fixtures and assertions.
- Recorded exact browser restrictions in [BLOCKED.md](BLOCKED.md) and the
  pending acceptance matrix in [the handoff](docs/audits/l00371/README.md).
- Retained command logs in [checks.txt](docs/audits/l00371/evidence/checks.txt).
- Playwright stopped its host; no listeners remained on 41999–42001.
- Fresh-context read-only verification found no substantive corrections. It
  checked unchanged originals, port copies, evidence hashes/logs and the partial
  status. It did not run a browser or independently reproduce the restrictions.

## Next

- Supervisor: commit the remaining files, push the task branch and open a PR
  using `.factory/PR_BODY.md`; final staging was sandbox-blocked.
- Supervisor: resolve browser permission and dispatch a runner with confirmed
  browser capability; V1 still requires workflows, screenshots and focused probes.
- Continue existing L00372/L00373/L00374 separately; this task implements no fixes.

## Surprises

- PROGRESS.md and features.json were absent; this task creates its own checklist.
- The original browser host uses ports outside the Factory allowed range.
- Standalone Chromium failed on macOS Mach-port permission before navigation.
- Automatic approval denied connected-browser local navigation and prohibited
  workarounds. No other browser route was attempted after the denial.
- No screenshots or runtime interaction results exist; browser checklist entries
  remain false. The task must be reported partial, not done.
- Web install reports 5 high dependency advisories. Remediation remains the
  upstream F8 follow-up; lockfiles are unchanged.
- Final `git add` failed creating the worktree index lock with “Operation not
  permitted.” Per Factory policy the report/helper/evidence remain uncommitted;
  no push or PR creation occurred. Initial commit `6b15498` is the current head.

## Final handoff

- Repository: `thedjpetersen/market-terminal`.
- Current full SHA: `6b15498478b5110f427e6c095e795703162169d1` (initial checkpoint only).
- Substantive report/helper/evidence files remain uncommitted after the index-lock
  denial; no PR or push, no deployment, owner merges.
- Generated test/config/host copies were removed; the checked-in reproduction
  helper recreates them. Final listener check found no task servers.
- Build/typechecks and 23 non-UI test cases passed. Browser workflows, screenshots
  and focused probes remain incomplete. See BLOCKED.md and the audit handoff.
