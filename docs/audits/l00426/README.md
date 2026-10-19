# L00426 — V1 browser capability preflight

**Partial. Access approval remains unresolved; no browser evidence was captured.**
Date: 2026-10-18 America/Los_Angeles (2026-10-19 UTC).
Product baseline and inspected local main:
`afb01cacb17a5d3a1e652e534191298a3e106564`.
Branch: `factory/l00426-resume-v1-with-a-permitted-browser-runne`.

The task asks to resume the original deterministic workflows and L00371's pending
probe matrix after access approval. Unlike the earlier attempt, this worker
checked connected-browser access before installing dependencies, building or
launching standalone Chromium. The supported CUA inventory exposed Chrome, but
automatic approval review rejected the single navigation to a minimal local
preflight page. [BLOCKED.md](../../../BLOCKED.md) preserves the exact response.

The server contained only static preflight text. It did not expose the app,
authentication, files, production data or provider requests. No page content was
returned by the browser. No workaround followed the denial.

## Fresh checks

| Check | Outcome |
| --- | --- |
| Startup `pwd`, `git status --short`, branch and `git log -20 --oneline` | Exit 0; clean assigned worktree, expected branch |
| `git rev-parse HEAD main` | Exit 0; both were the baseline above |
| `git diff --exit-code HEAD main -- web contracts/web/client` | Exit 0; inspected local main matches the tested source baseline |
| `git diff --exit-code afb01cacb17a5d3a1e652e534191298a3e106564 -- web contracts crates Cargo.toml Cargo.lock` | Exit 0; product/dependencies unchanged |
| Python byte comparison against baseline | Exit 0; original Playwright config, host and both suites unchanged |
| CUA inventory and local-development documentation | Available; this alone does not establish navigation permission |
| CUA navigation to `http://127.0.0.1:42600` | Denied by automatic approval review before rendering |
| Preflight process cleanup | Owning exec session exited 0; socket check confirmed port 42600 closed |
| `git diff --check` and Python handoff validation | Exit 0; feature JSON, pending browser flags, recorded hashes and local document links validated |
| Build, TypeScript, unit/client and HTTP suites | Not rerun; application setup stopped at the access gate |
| Deterministic browser suites, screenshots and probes | Not run; all browser feature entries remain false |

No upstream fetch or merge occurred. The main comparison is against the available
local ref, not a claim about current remote main. No fresh application-test pass,
combined-branch verification, visual score or screenshot inspection is claimed.
L00371's historical build and 23 non-UI cases remain historical evidence only.

Baseline SHA-256 values verified during this attempt:

```text
e70c25222f2fbc126a7edd59d94902d8a360e2e509f59f4b0bbdb57f0de8b1b9  web/playwright.config.ts
910c661365cbc879889b376d9df47f4ef6f5ed9ee07989beff4ae97cb77b068a  web/tests/local-host.mjs
ee7ed04b9ad913497cc7e4237363f5d11e073dff95c3d9f0937760690b2a38cf  web/tests/workflows.browser.ts
88af2d55ba1af65a7812bfc0b05055cb357928e6d7ae5569aca91517fd5ee78e  web/tests/engine.browser.ts
```

## Remaining acceptance

All rows remain **not executed**. Keep the existing original test assertions.
F1/F2/F3 implementation stays with L00372/L00373/L00374, whose commits are not in
this branch. The supervisor must identify the intended integrated revision for
final acceptance; old-baseline observations cannot verify those fixes.

| Scope | Required evidence |
| --- | --- |
| Original workflows | Research/watch/save/financials/filings/news/reload/evidence; all three real WASM models and exact download; whole-quotes 502, at both sizes |
| Screenshots | Capture and inspect markets, search, research, watchlist, saved evidence, models, loading, error and empty states at 1440x1000 and 390x844; retain filename/result manifest under `.factory/screenshots/` |
| F1 | AAPL then edited query: Enter during debounce, deferred response, error, empty/latest success; pointer parity |
| F2 | About/saved evidence then Cmd/Ctrl-K; modal count, Tab/Shift-Tab boundaries, Escape, visible trigger restoration and body scroll |
| F3 | JSON 401, Access-like HTML, 502 then healthy research/history/news/SEC/backtest; recovery guidance, resource retry, refresh scope and empty filings |
| Keyboard | Search announcements, back/forward and heading orientation, tab/range selected state, chart date/value, focus-visible and JSON scrolling |
| Reflow | 320px, 200% text/zoom, long symbols/names/evidence; distinguish document overflow from intentional table/pre scrolling |
| Motion | Reduced-motion loading and mobile model success/error scrolling/focus |
| Storage | Blocked reads/writes, corrupt data, quota exhaustion, failed-save preservation, removal/reload/download |
| Visual review | Bloomberg density / Linear keyboard clarity; contrast, mobile labels and targets, light-mode behavior; scores only after viewing evidence |
| Independent browser verification | Fresh-context runner verifies the exact revision, screenshots and behavior; preferably Claude |

## Resume without another unchanged retry

1. Resolve runner permission externally; verify local access before another build.
   CUA discovery succeeding is not access approval. Do not bypass this denial.
2. Integrate the intended fixes through the supervisor's normal branch workflow,
   record the exact full SHA, and run fresh baseline checks on that revision.
3. Reuse the port-only adapter and commands from L00371 commit `5f8b537`, paths
   `docs/audits/l00371/reproduce.py` and `docs/audits/l00371/README.md` (readable
   with `git show`). The adapter uses 41999/42000/42001 and preserves assertions.
   Use deterministic provider routes and the real local Worker/WASM with its
   throwaway issuer. Do not use production storage state or live-provider mode.
4. Execute the matrix and independent browser review, then update pass flags only
   for evidence actually obtained. Follow the current no-PR Factory policy;
   older audit instructions about opening a PR are superseded.

Use the existing V1 and F1/F2/F3 verification tasks instead of duplicating their
implementation. The only new prerequisite is an explicitly permitted runner.

## Handoff and rollback

Documentation-only checkpoint. No product, test, dependency, authentication or
deployment changes. No push, PR or deployment. Revert the L00426 documentation
commit to roll back; there is no production rollback. A partial result never
authorizes publication to main.

Checkpoint `20dd4fe36f8573f757cec3ffcfa347cb2a801b1f` is committed. The final
review-note/checklist update remains uncommitted because subsequent staging
could not create the Git index lock (`Operation not permitted`). Under the
Factory fallback, the supervisor must commit those remaining documentation edits
on this task branch. No alternate Git writer was attempted.

## Independent handoff review

A fresh-context read-only Codex reviewer checked `afb01ca..20dd4fe` and found no
substantive errors. It independently verified that only four documentation and
checklist files changed, all recorded hashes match the original baseline suites,
JSON parses, local main matches the stated baseline, and port 42600 refuses
connections. It confirmed the remaining acceptance matrix is preserved.

The reviewer did not invoke a browser, independently reproduce the denial or run
application checks. This is reporting validation, **not independent browser
verification**. This final documentation update marks only the handoff feature
as passing; all four browser-dependent features remain false.
