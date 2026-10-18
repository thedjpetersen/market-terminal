# L00371 — desktop/mobile verification follow-up

Date: 2026-10-18. Product baseline:
`afb01cacb17a5d3a1e652e534191298a3e106564`.
Branch: `factory/l00371-v1-complete-permitted-desktop-mobile-and`.

**Status: partial. V1's browser acceptance remains unmet.** Standalone Chromium
cannot launch under this macOS sandbox, and automatic approval review denied
connected-browser access to the local host. No screenshots were captured or
inspected. No interaction, accessibility or visual-design pass is claimed.
[BLOCKED.md](../../../BLOCKED.md) records the denial and exact unblock request.

The [L00309 audit](https://github.com/thedjpetersen/market-terminal/blob/8b223c9dd85acf030de2cbf4a1859483376c7220/docs/audits/l00309/README.md)
was read from its completed branch because it is absent from this branch's
ancestry. This follow-up changes documentation and adds a reproducible port-only
adapter. Product code, dependencies, original acceptance tests, providers,
credentials and deployment settings are unchanged. The existing F1/F2/F3 work
belongs to L00372/L00373/L00374.

## Executed checks

Check results are recorded in [evidence/checks.txt](evidence/checks.txt).
Full local command logs are under `.factory/logs/`. Browser launch failure is an
environment restriction, not an application assertion failure. HTTP test project
names do not imply a rendered desktop or mobile browser.

| Check | Exit | Result |
| --- | ---: | --- |
| Locked client/web installs | 0 | Completed; web install reported 5 high advisory entries, unchanged from L00309 |
| Production WASM / TypeScript / Vite build | 0 | Passed |
| Ordered web and client TypeScript | 0 | Passed |
| Web unit checks | 0 | 9/9 passed |
| Exact-integer client checks | 0 | 8/8 passed |
| Authenticated HTTP / real WASM checks | 0 | 6/6 passed; request fixtures only |
| Port adapter equality and original-file diff | 0 | Original tests, fixtures, assertions and product files unchanged |
| Chromium launch smoke | 1 | macOS Mach-port permission denial before navigation |
| Connected-browser navigation | Denied | Automatic approval rejected local access |
| Screenshots and focused probes | — | Not executed |

Playwright stopped its local host; no listeners remained on 41999–42001. The
Worker reported that its asset watcher was disabled by a platform limit, which
did not affect the completed static-build HTTP checks. Node 24 CI parity, Rust
architecture tests and Wrangler packaging were not rerun: this task changes no
product code, dependencies, module boundaries or deployment configuration.

## Remaining acceptance matrix

Every row below is **not executed**, not passed. Use synthetic provider fixtures
and the real local Worker/WASM with its throwaway issuer. Do not use production
storage state, real credentials or `MARKET_LIVE=1`.

| Scope | Required observations / evidence |
| --- | --- |
| Existing workflows | Research → watch → save → financials/filings/news → reload → saved evidence; all three models plus exact download; whole-quotes 502. Run each at 1440×1000 and 390×844 without changing assertions. |
| Screenshots | At both sizes capture markets, search, research, watchlist, saved evidence, model results, loading, error and empty states into `.factory/screenshots/`; inspect every image and retain a filename/result manifest. |
| F1 / L00372 | Load AAPL, edit query, press Enter during the 200ms debounce, a deferred request and an error; old AAPL must not activate. Check empty/latest successful responses and pointer parity. |
| F2 / L00373 | Open About or saved evidence, press Cmd/Ctrl-K, Tab and Shift-Tab through boundaries, then Escape; record modal count, active focus, visible trigger restoration and body scrolling. |
| F3 / L00374 | Inject JSON 401, Access-like HTML and 502, then healthy replies for research/history/news/SEC/backtest; record guidance, resource retry, refresh scope and empty filings. |
| Keyboard orientation | Search selection announcements, route back/forward and heading orientation, tab/range selected state, chart slider date/value, focus-visible and modal JSON scrolling. |
| Reflow | 320px width, 200% text/zoom, long supported symbols/names and long evidence; record document overflow separately from intentional table/pre scrolling. |
| Reduced motion | Emulate reduced motion; inspect loading animations and mobile model success/error behavior, including scripted result scrolling and keyboard focus. |
| Device storage | Block reads/writes, inject corrupt data, simulate quota exhaustion; distinguish empty data from unavailable storage, preserve existing items on failed saves, verify removals/reload/download. |
| Visual assessment | Use [DESIGN.md](../../../DESIGN.md): score design quality, originality, craft and functionality 1–5 only after inspection. Check contrast, 8–10px mobile labels, chart targets and light-mode behavior. |

No regression assertions are added for F1/F2/F3 here because no browser
reproduction ran. Their implementation tasks should add behavioral regressions;
this handoff does not turn source inspection into runtime evidence.

## Reproduce in an authorized environment

From the repository root, install and build in order:

```sh
npm ci --prefix contracts/web/client
npm ci --prefix web
npm run build --prefix web
npm run check --prefix web
npm test --prefix web
npm run typecheck --prefix contracts/web/client
npm test --prefix contracts/web/client
python3 docs/audits/l00371/reproduce.py
```

The adapter copies the original config, host and both suites to ignored
`web/.l00371-*` and `web/tests/l00371-*` paths. It substitutes only ports
8899→41999, 8900→42000, inspector port 42001, and copied filenames. Viewports,
test bodies, fixtures and assertions remain unchanged. The host stays loopback
only and keeps real signature verification. `reuseExistingServer` stays false
so another process cannot silently satisfy readiness. The helper does not grant
browser permission or remove the current blocker.

An authorized runner can execute all 12 existing project checks:

```sh
cd web
WRANGLER_SEND_METRICS=false XDG_CONFIG_HOME="$PWD/../.factory/config" \
  ./node_modules/.bin/playwright test --config .l00371-playwright.config.ts \
  --workers=1
```

For the six HTTP-only checks, append `l00371-engine` to that command. They use
Playwright's request fixture and do not launch Chromium. The current blocked
session runs only this subset. Screenshots and the focused probes above still
require a separately authorized browser runner.

When finished, remove only the generated copies, stop processes started for this
task, and confirm no task listeners remain on 41999–42001. Do not bypass an
approval denial with another browser surface or a proxy.

## Handoff and rollback

Final Git staging was sandbox-blocked after initial checkpoint `6b15498`.
Substantive report/helper/evidence files remain uncommitted under the Factory
fallback. The supervisor must commit them, push the task branch and open a PR;
`.factory/PR_BODY.md` is prepared. No push or PR creation occurred.

No deployment occurred, so there is no preview URL or promote command.
Rollback is to revert this task's documentation/helper commits through a PR.
The owner retains merge authority. Browser approval must be resolved outside
this worker session; avoid dispatching another identical sandbox attempt without
changing that capability. Do not mark V1 complete until the remaining evidence
has been captured and independently reviewed.

## Independent verification

A fresh-context Codex verifier reviewed the diff, brief, acceptance matrix,
original suites, generated copies and retained logs read-only. It found no
substantive correction: product/tests are unchanged, port copies preserve
assertions, hashes and logs match, screenshots are absent, and browser checklist
entries remain false. It did not rerun checks, launch a browser, view the UI,
access production or independently reproduce either permission denial. This
verifies the accuracy of the partial handoff, not V1 browser acceptance.
