# L00490 — search screenshot acceptance handoff

**Partial: 0/6 screenshots captured, 0/6 reviewed.** Run date: 2026-10-18 PDT.
The browser never loaded the application. No visual, keyboard or pointer pass
is inferred from failed launches or from the passing component tests.

## Source and worktree

- Repository: `thedjpetersen/market-terminal`.
- Branch: `factory/l00490-complete-l00372-screenshot-acceptance-wi`.
- Starting/current HEAD and local main:
  `afb01cacb17a5d3a1e652e534191298a3e106564`.
- Required L00372 snapshot: `fde8fe3a5387de397697091797672f1a7f76303d`.
- L00371 audit/design reference read from:
  `5f8b537ac038d28718ba290515e5f9390617c6a0`.
- Runtime: Node `v26.7.0`, npm `11.19.0`, macOS arm64.

The assigned branch did not contain `test:search`, DESIGN.md, PROGRESS.md or
features.json. An attempted fast-forward failed when Git could not create
`/Users/thedjpetersen/code/market-terminal/.git/worktrees/L00490/ORIG_HEAD.lock`.
Per the Factory fallback, L00372's exact diff was applied to this worktree
without updating Git metadata. The source fix, package/lockfile, tests, design
and false checklist entry are from upstream, not new L00490 implementation.
Only PROGRESS.md, BLOCKED.md and this report differ from the upstream snapshot.

The supervisor should preserve L00372's existing ancestry when integrating;
do not treat the uncommitted upstream snapshot as an independent replacement
implementation. No L00371 source merge was needed: its changes are audit docs
and a port adapter for the broader suite. This task does not complete V1.

## Fresh evidence

| Check | Exit/result | Evidence |
| --- | --- | --- |
| Initial `npm run test:search --prefix web` at main | 1: script absent | Script present only in L00372 |
| `git merge --ff-only factory/l00372-f1-prevent-stale-search-result-activatio` | 128: `ORIG_HEAD.lock` denied | Git ancestry unchanged |
| `npm ci --prefix web` | 0 | `.factory/logs/npm-ci-web.log` |
| `npm ci --prefix contracts/web/client` | 0 | `.factory/logs/npm-ci-client.log` |
| `npm run test:search --prefix web` with upstream files | 1: both projects failed at Chromium launch | `.factory/logs/search-acceptance.log` |
| Connected Chrome local navigation | Approval rejected: permission declined | `.factory/logs/browser-approval-denial.txt` |
| `cd web && ./node_modules/.bin/vitest run tests/search.test.tsx` | 0: 11/11 passed | `.factory/logs/search-unit.log` |
| Upstream source/design/checklist comparison | 8/8 byte-identical | `.factory/source-provenance.json` |
| Screenshots in `web/test-results/` | 0 PNG files | `.factory/screenshots/README.md` |
| Local server cleanup | No listener on 41772 | `.factory/logs/cleanup.txt` |

The first suite attempt also reported unresolved client `lossless-json`, because
the separate client install had not yet run. That install then passed, and Vite
started normally for the connected-browser capability check. The subsequent
approval denial stopped browser work; the suite was not retried through another
route. Browser failure is an environment result, not an application assertion.

Chromium's failure was:

```text
FATAL:base/apple/mach_port_rendezvous_mac.cc:159
bootstrap_check_in org.chromium.Chromium.MachPortRendezvousServer:
Permission denied (1100)
```

Connected Chrome navigation to `http://127.0.0.1:41772/?view=watchlist` was
rejected by automatic approval because permission was declined. The rejection
explicitly prohibited indirect execution, raw CDP, alternate browser surfaces,
and policy circumvention. No workaround was attempted after that denial.

The full build, full web/client suites and architecture checks were not rerun
after the baseline browser blocker. No production, provider or deployment
checks were attempted. Prior upstream/CI passes are not claimed as current
combined-branch verification. No independent verification occurred in L00490.

## Resume only after browser capability is confirmed

Use a runner explicitly permitted to launch Chromium and reach its local Vite
server. Keep the L00372 tests and assertions unchanged. Integrate the upstream
branch with current main under the supervisor's normal history-preserving
workflow, then run the documented setup and acceptance:

```sh
npm ci --prefix contracts/web/client
npm ci --prefix web
npm run build --prefix web
npm run check --prefix web
npm test --prefix web
npm run test:search --prefix web
```

The search suite uses port 41772 and both existing projects: desktop 1440×1000,
mobile 390×844. It requires no credentials, live providers or production access.
Review each `search-ready.png`, `search-pending.png` and `search-error.png` in
the two project output folders against [DESIGN.md](../../../DESIGN.md). Copy
them into `.factory/screenshots/` with desktop/mobile prefixes. Record each
image's dimensions, hash and observed result, then score design quality,
originality, craft and functionality 1–5; any score below 3 fails. Keep L00371's
broader modal/reflow/recovery acceptance separate. Only flip L00372's `passes`
after its six images and interaction checks pass.

## Handoff and rollback

All changes are uncommitted because Git metadata writes are blocked. The
supervisor owns integration and publication; no PR was created, no branch
was pushed, no deployment happened, and no task server remains. This partial
result does not authorize a push to main.

Before committing, rollback means removing only L00490's uncommitted snapshot
and report changes, after retaining this evidence. After integration, revert
only the L00490 handoff commit to roll back this task; L00372's implementation
has its own upstream history. No deployment or data rollback is needed.
