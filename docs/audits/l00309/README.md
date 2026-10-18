# Web companion readiness and accessibility audit

Date: 2026-10-18. Baseline: `afb01cacb17a5d3a1e652e534191298a3e106564`.
Task branch: `factory/l00309-market-frodojo-com-web-companion-readine`.
Publication: final Git staging was sandbox-blocked; no PR was created. The
supervisor handoff and prepared PR body are recorded in [BLOCKED.md](../../../BLOCKED.md).

**Assessment: the local build and analytical/authentication checks pass, but
browser readiness remains unverified.** Source inspection identifies specific
keyboard, state-recovery and provenance gaps. This is an audit package, with no
product, provider, credential, deployment or native-terminal changes.

The owned domain is an allowlisted research companion. An unauthenticated HEAD
request to [market.frodojo.com](https://market.frodojo.com) returned HTTP 302 to
Cloudflare Access on the audit date. No login was attempted. Public registration
is outside the current product contract; the access gate is not classified as a
defect. No personal portfolios, brokerage accounts, trades or paid data were used.

## Evidence and limits

Labels below distinguish **executed** checks from **source-confirmed** gaps and
**pending** runtime observations. A source-confirmed gap is not a reproduced
browser failure. Prior README claims are not fresh verification evidence.

| Check | Exit | Result |
| --- | ---: | --- |
| `npm ci --prefix contracts/web/client` | 0 | Lockfile install; 0 audit advisories |
| `npm ci --prefix web` | 0 | Lockfile install; 5 high-severity audit entries |
| `npm run build --prefix web` | 0 | Rust WASM, TypeScript and Vite production bundle |
| `npm run check --prefix web` after build | 0 | TypeScript passed |
| `npm test --prefix web` | 0 | 9/9: auth, real WASM fixtures, exact integers, SEC annual facts, bounds, web feature isolation |
| `npm run typecheck --prefix contracts/web/client` | 0 | Client TypeScript passed |
| `npm test --prefix contracts/web/client` | 0 | 8/8 exact codec/native fixture checks |
| `cd web && ./node_modules/.bin/playwright test --config .l00309-playwright.config.ts l00309-engine --workers=1` | 0 | 6/6 HTTP-only checks, three checks in each project; real local Worker/WASM and throwaway JWTs |
| `cd web && ./node_modules/.bin/wrangler deploy --dry-run` | 0 | Worker/assets package validated; nothing deployed |
| `npm audit --prefix web --json` | 1 | 5 high entries: miniflare, sharp, source-map-js, undici, wrangler |
| `npm audit --prefix web --omit=dev --json` | 0 | 0 production-dependency entries; not a security certification |
| Standalone Playwright Chromium launch smoke | 1 | macOS sandbox denied Mach port registration before app navigation |
| Existing `workflows.browser.ts` and visual probes | — | Not run; connected-browser local access subsequently denied |

Checked with Node 26.7.0, npm 11.19.0, rustc 1.88.0, wasm-pack 0.13.1 and
Wrangler 4.129.0. The installed Chromium revision was 1243. Node 24 CI parity was
not assessed. A premature standalone TypeScript check failed on missing generated
WASM declarations while the build was still running; the ordered check above
passed after generation. No code change was made to obtain that pass.

Compact logs are in [evidence](evidence/checks.txt); full local logs remain in
`.factory/logs/`. No screenshots exist. The screenshot destination
`.factory/screenshots/` was created but not populated. See
[BLOCKED.md](../../../BLOCKED.md) for exact limitations. The temporary browser
fixture bridge was stopped without being viewed. All local test servers were
stopped and no listeners remained on 41999–42002.

The Rust architecture suite was not rerun: this change contains no dependency
or module modifications. The existing web feature-isolation check passed.
Native terminal, live provider reliability, access-policy configuration,
VoiceOver, real devices and legal compliance were outside executed verification.

## Core desktop/mobile flow assessment

Both widths below are **source/test coverage assessments**, not observed renders.
The existing Playwright config targets 1440×1000 desktop and 390×844 Pixel 7.

| Flow | Existing implementation / offline coverage | Remaining verification |
| --- | --- | --- |
| Markets → search → research | Five shell destinations, URL view/symbol, history push/pop; existing workflow selects AAPL | Run keyboard-only search, back/forward and focus restoration; F1/F2/F6 |
| Research → chart / financials / filings / news | Labeled range scrubber, data tabs, SEC/publisher links; existing workflow checks financial row and links | Keyboard chart readout, selected-state announcements, empty filings, source timestamps; F3/F4/F6 |
| Watch → watchlist → reload → remove | Bounded 32-ticker local storage; failures become per-symbol unavailable rows; existing workflow checks persistence | Reconnection/retry, storage read failures, dated quotes and mobile row layout |
| Save → library → evidence → download | 30-snapshot limit, named dialog, Escape, exact JSON, device-local disclosure; existing workflow opens saved evidence | Nested dialog shortcut, JSON scrolling, focus return, empty library and backup behavior |
| Options / fixed income / strategy backtest | Real WASM fixture parity and authenticated HTTP pass; existing workflow runs all models and requests download | Browser form submission, invalid fields, long outputs, reduced motion and mobile focus/scroll |
| Public entry | Executed unauthenticated redirect to Access | Authenticated production UI deliberately not accessed |

The existing workflow suite has three tests per viewport: a research/watchlist/
library journey, all three models plus download, and a whole-quotes 502 response.
It checks page width only at the end of the first two journeys. It has no explicit
coverage for search-query races, multiple dialogs, tab selected semantics, session
HTML responses, empty filings, blocked storage reads or reduced-motion scrolling.
The HTTP suite's “desktop/mobile” labels do **not** imply that a browser rendered.

## Prioritized findings and bounded follow-ups

P1 means address before relying on keyboard-driven research workflows. P2 means
the next readiness increment. All findings below are source-confirmed; runtime
reproduction and screenshots are part of the acceptance checks, not claimed here.
Each task is deliberately limited to the web companion.

### F1 — P1: search can open a result from a previous query

[Search.tsx:30–82](../../../web/src/features/research/Search.tsx#L30) retains
`results` when the query changes or a request fails. Loading starts after a
200ms debounce. Enter checks only `results[selected]`, even when the visible
panel is loading or showing an error. Thus the selected instrument is not tied
to the query displayed in the textbox.

**Reproduction to run:** load AAPL results, enter a different query with its
response deferred or rejected, then press Enter before it resolves. Navigation
must not use the prior AAPL result. Also test the debounce interval itself.

**Bounded task (60–90 minutes):** invalidate selection immediately on edit and
bind activation to results for the current successful query. Add deterministic
slow/error/empty transitions. Keep pointer and keyboard activation equivalent.
Acceptance: Enter cannot select hidden or stale results; the latest valid result
still opens. No provider/API change is needed.

### F2 — P1: multiple modal focus handlers can target the wrong dialog

[Shell.tsx:42](../../../web/src/app/Shell.tsx#L42) opens search for Cmd/Ctrl-K
regardless of another modal being open. About and saved evidence retain their
own state; [useModal.ts:12–47](../../../web/src/ui/useModal.ts#L12) installs a
handler per modal but queries the first `[role="dialog"]` in the document.
Escape is handled by every mounted modal. The code does not establish a single
active dialog or a topmost-dialog focus target.

**Reproduction to run:** open About or saved evidence, press Cmd/Ctrl-K, cycle
Tab/Shift-Tab and then Escape. Observe focus and which dialogs close.

**Bounded task (60–90 minutes):** define one active-overlay policy in the shell
and scope focus handling to its dialog. Acceptance: only the active dialog owns
Tab/Escape, focus returns to a visible trigger, scrolling restores correctly,
and background content is excluded from modal navigation.

### F3 — P2: research recovery is inconsistent with model recovery

[research.ts:2–14](../../../web/src/infrastructure/research.ts#L2) parses JSON
before inspecting status/content type. A redirected Access HTML page becomes a
JSON parse error; a JSON 401 receives generic research handling. In contrast,
[analytics.ts:3–34](../../../web/src/infrastructure/analytics.ts#L3) already maps
401/403, HTML and transport failures to session-recovery guidance. Backtest
`bars()` has the same unguarded JSON parsing as research.

[Headlines](../../../web/src/features/research/Workspace.tsx#L51) has no retry
or refresh input. Markets Refresh updates quotes/history/leaders, not headlines.
History and SEC errors also lack a local Retry control; watchlist adapter errors
become unavailable rows without a retry action. Navigation/reload is the fallback.
The filings branch maps an empty array with no empty-state message, while
financials/news/chart have explicit empty states.

**Bounded task (90–120 minutes):** add consistent session response classification
to research/backtest history, retry the failed resource, include headlines in
overview refresh, and explain empty filings. Acceptance: synthetic 401, HTML,
502, then healthy responses offer understandable recovery; missing data stays
missing. Keep provider calls bounded and avoid fabricated fallback prices.

### F4 — P2: compact quote views omit observation-time provenance

[Watchlist](../../../web/src/features/watchlist/Workspace.tsx#L64) renders price,
change and sparkline but never `asOf`, although its
[contract already includes it](../../../web/src/features/watchlist/contracts.ts#L8).
The footer describes storage only. Markets tiles also omit timestamps, but do
tell users to open an asset for its source time. Research displays source,
currency, exchange and quote time; that is a useful pattern to retain.

SEC financial rows show fiscal-period end and generic “latest filed” copy but
omit the available `filed` date
([adapter](../../../web/worker/infrastructure/sec.ts#L88),
[table](../../../web/src/features/research/Workspace.tsx#L410)).
News and filings already provide named publishers/SEC and external links.

**Bounded task (60–90 minutes):** show quote source/observation time at the point
of use, distinguish missing/old timestamps, and expose the available SEC filing
date without implying all metrics came from one accession. Acceptance: differing
synthetic dates stay visible at desktop/mobile widths; no “live” implication from
the decorative edge status. Full metric/accession lineage is separate work.

### F5 — P2: mobile help and privacy information are incomplete

The only About trigger is in
[the sidebar](../../../web/src/app/Shell.tsx#L88), which
[mobile CSS hides](../../../web/src/styles.css#L1127). That dialog includes
assumption, storage and Add to Home Screen guidance with no mobile replacement.
Local-storage warnings in the watchlist/library remain useful. No privacy page
or privacy link exists in the frontend; the watchlist's “No account” wording can
be confused with the Access-authenticated application.

**Bounded task (45–75 minutes):** expose the same help on mobile and add a concise
privacy/data-use page reachable from both layouts, covering device storage,
provider requests and Access/Cloudflare processing. Describe uncertain retention
settings as pending owner confirmation; do not invent legal promises or broaden
access. Acceptance: help/privacy can be reached and closed with touch/keyboard,
and storage terminology agrees with the actual authentication model.

### F6 — P2: visual selection is not programmatically exposed

Search arrow keys change a CSS class while focus remains on the input; there is
no active-descendant/listbox relationship or selected-result announcement
([Search.tsx:66–119](../../../web/src/features/research/Search.tsx#L66)).
Research tabs, range segments and model tabs likewise use active classes without
selected/pressed state. Shell navigation correctly uses `aria-current`.
The chart has a labeled native slider, but announces an observation index rather
than date/value; route changes do not update focus or announce the new heading
([bootstrap.tsx:44](../../../web/src/bootstrap.tsx#L44)).

**Bounded task (90–120 minutes):** implement suitable selected-state semantics,
meaningful slider value text and route orientation; preserve standard button
behavior or implement the complete tab pattern. Acceptance: assistive technology
can identify the active query result, tab/range, inspected date/value and new
workspace. Verify Ctrl-K from an input and focus-visible styling. A missing ARIA
role alone is not classified as a WCAG failure; verify the actual interaction.

### F7 — P2: reduced motion is only implemented in CSS

[styles.css:1521](../../../web/src/styles.css#L1521) disables animations and
transitions, but [model completion](../../../web/src/features/models/Workspace.tsx#L142)
unconditionally requests smooth scrolling at mobile widths.

**Bounded task (30–45 minutes):** honor the motion preference in scripted result
navigation and keep keyboard orientation. Acceptance: a mobile model run under
reduced-motion uses immediate scrolling; CSS loading transitions stay disabled.
Check both successful and failed runs without forcing focus away unexpectedly.

### F8 — P2: development dependency advisories need a bounded update

The audit registry response lists five high-severity package entries in the
development graph; `--omit=dev` reports zero. These are package-chain entries,
not five independently established production exploits. No dependency was changed.
The [captured advisory metadata](evidence/dependency-advisories.json) retains
the specific advisory URLs, affected ranges and dependency chains observed on
the audit date, so later registry changes do not erase the evidence.

**Bounded task (60–90 minutes):** update the affected lockfile dependencies,
inspect changes, rebuild WASM/frontend, run unit/client/HTTP/browser tests and
Wrangler dry-run. Acceptance: the listed advisories are resolved or individually
documented with evidence; no automatic production deployment or force update.

### V1 — required verification follow-up (60–90 minutes)

Use an authorized browser runner in a fresh context, preferably Claude. Run the
unchanged existing deterministic browser suite, then exercise the F1/F2/F3 cases.
Capture and inspect screenshots under `.factory/screenshots/` at 1440×1000 and
390×844 for markets, search, research, watchlist, saved evidence, model output,
loading/error/empty states. Include 320px reflow, 200% text/zoom, long symbols,
table scrolling, real keyboard focus, dialog close/restore, reduced motion and
blocked/full/corrupt device storage. Existing storage reads silently return an
empty array on parsing/access failure; establish user impact before changing it.

Use [DESIGN.md](../../../DESIGN.md) as the reference. Score design quality,
originality, craft and functionality 1–5 only after viewing the screenshots.
No score is assigned here. Also assess contrast, mobile's 8–10px supporting
labels, small chart targets, and the absent light theme; these are review targets,
not measured failures. Add meaningful regression coverage for confirmed gaps
without weakening the existing acceptance tests.

## Reproduction and handoff

Install/build in the order shown in the checks table. Full browser execution
normally uses `npm run test:browser --prefix web`; this audit did not run it.
Factory's allowed ports required temporary ignored copies, not edits to the
original tests. The exact adaptation is recorded in [reproduce.py](reproduce.py).
It copies the existing config/host/tests, substitutes 41999/42000, sets inspector
port 42001, and leaves all assertions intact. Run from the repository root:

```sh
python3 docs/audits/l00309/reproduce.py
cd web
WRANGLER_SEND_METRICS=false XDG_CONFIG_HOME="$PWD/../.factory/config" \
  ./node_modules/.bin/playwright test --config .l00309-playwright.config.ts \
  l00309-engine --workers=1
```

An authorized supervisor can omit `l00309-engine` to include the browser
workflows. The copied suite uses no live providers unless explicitly opted in;
do not set `MARKET_LIVE`, production URL or a personal storage-state file.
Clean up only the generated `web/.l00309-*` and `web/tests/l00309-*` files.

Rollback: revert the audit documentation commits. No deployment, migration,
production mutation or dependency update occurred. The owner retains merge
authority. This audit must remain partial until V1 supplies the missing browser
evidence; F1–F8 are implementation follow-ups, not work completed in this PR.

## Independent review

A fresh-context Codex verifier reviewed the diff, brief, command evidence and
referenced source, without browser access or product edits. It confirmed F1–F7
and the recorded check results, and requested durable advisory identifiers for
F8; those are now retained above. A proposed single-symbol HTTP-200 unavailable
case was withdrawn after checking the Worker: all requested quotes failing
produces HTTP 502, which Research already handles. Browser acceptance remains
unmet, not waived. Cross-harness browser verification is assigned to V1.
