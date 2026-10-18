# L00309 — web companion readiness audit

## Scope and decisions

- Audit baseline: `afb01ca` on `factory/l00309-market-frodojo-com-web-companion-readine`.
- Inspect the existing Cloudflare companion, navigation, states, accessibility,
  attribution and offline checks. No product or native-terminal changes.
- Use synthetic browser fixtures and ephemeral local authentication only. The
  unauthenticated production root redirects to Cloudflare Access; no sign-in,
  private data, provider purchases, deployment or infrastructure changes.
- Reference: Bloomberg information density with Linear keyboard clarity.
- Existing browser harness ports 8899/8900 are outside Factory policy. Run
  temporary copies on 41999/42000 without changing the original acceptance tests.
- This worktree initially had no PROGRESS.md or features.json. The new feature
  checklist describes audit completion, not a claim of production readiness.

## Done

- Read repository/web documentation, architecture constraints and existing checks.
- Confirmed a clean task branch and inventoried research, watchlist, models,
  saved evidence, provider adapters and modal/navigation behavior.
- Committed the initial audit checklist as `3829303`.
- Build and ordered TypeScript checks passed; 9 web unit tests, 8 client tests
  and 6 HTTP-only Playwright checks passed. Wrangler dry-run passed.
- Recorded five high-severity npm development-graph entries and zero entries
  with development dependencies omitted. No dependency updates were made.
- Documented eight source-confirmed gaps and bounded follow-ups in
  [the audit](docs/audits/l00309/README.md), with compact command
  evidence and a reproducible port-only test adaptation.
- Recorded the requested design reference in [DESIGN.md](DESIGN.md).
- Independent fresh-context review confirmed the source findings and check
  evidence. Preserved advisory IDs/ranges after its evidence request. A suggested
  empty-200 quote bug was withdrawn after verifying the single-symbol Worker
  actually returns 502, already handled by Research.
- Stopped the local issuer, Worker and unused synthetic fixture bridge. Verified
  no remaining listeners on 41999–42002; reset/closed the unused browser tab.

## Next

- Supervisor: commit the remaining audit files, push the task branch and open
  the documentation PR using `.factory/PR_BODY.md`. Final git staging failed
  because the sandbox denied the worktree index lock. No PR was created.
- Supervisor: run V1 with authorized browser access, preferably Claude. Capture
  and inspect desktop/mobile screenshots and confirm keyboard behavior.
- Then address F1 stale search activation, F2 modal focus ownership and F3
  failure/session recovery before broader readiness work.

## Evidence

- Local command logs: `.factory/logs/`.
- Check excerpts: [checks.txt](docs/audits/l00309/evidence/checks.txt).
- Browser captures: **none**. `.factory/screenshots/` is empty.
- Source baseline: `afb01cacb17a5d3a1e652e534191298a3e106564`.

## Surprises / limitations

- Production is an allowlisted research companion, not a public onboarding flow.
- Standalone Chromium could not start under the macOS sandbox. Connected-browser
  local navigation was then denied by automatic approval. No workaround was
  attempted after that denial. Exact blockers are in [BLOCKED.md](BLOCKED.md).
- Core desktop/mobile flow assessment is limited to source and existing test
  coverage. No rendered layout, real touch, screen-reader or visual score is
  claimed. The audit remains partial until V1 is completed.
- An initial TypeScript invocation raced WASM generation; it passed after the
  required build. No product baseline failure required a fix.
- No production settings, credentials, product modules or original acceptance
  tests changed. Rollback is a revert of documentation commits; nothing deployed.
- Final Git head is the initial checkpoint (full SHA recorded in
  `FACTORY_RESULT.json`); the substantive report is intentionally uncommitted
  under the Factory sandbox fallback. This is a second completion blocker.
