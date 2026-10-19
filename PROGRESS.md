# Factory progress

## L00372 — prevent stale search result activation

- Scope: web research search activation during debounce, loading, failure, and
  overlapping requests. Modal ownership and research-session recovery belong to
  L00373 and L00374 and are outside this task.
- Startup: assigned branch and clean worktree verified; read repository and web
  guidance. No prior `PROGRESS.md` or `features.json` existed in this checkout.
- Baseline: `npm run build --prefix web` passed (exit 0); `npm test --prefix web`
  passed all 9 existing tests (exit 0). Typechecking needs generated WASM
  declarations and is included in the successful build.
- Acceptance: current search results open by keyboard and pointer; previous
  results never open during debounce, loading, or failure; late completions do
  not replace newer results, including returning to an earlier query.
- Rollback: revert this task's commits. No deployment or data migration is needed.
- Reproduction: the new React/DOM suite produced 5 failures and 5 passes against
  the original component. Enter selects AMZN during debounce/loading and AAPL
  after failure, returning to an earlier query, or changing the provider.
- Browser blocker: Chromium exits before opening a page because macOS denies
  `bootstrap_check_in ... MachPortRendezvousServer` (permission denied 1100).
  Deterministic component tests use jsdom. Hosted GitHub CI subsequently ran
  the desktop/mobile browser checks successfully.
- Fix: every edit carries a distinct request identity; a response is visible
  and actionable only when both that identity and the provider match. Keyboard
  and pointer share activation, failures carry no results, and success resets
  selection. Existing abort protection remains in place.
- Final local checks (exit 0): `npm run build --prefix web`,
  `npm run check --prefix web`, `npm test --prefix web` (20/20, including 11
  search regressions), Prettier, and `git diff --check`.
- `CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0
  cargo test --test architecture_boundaries` passed 10/10 (exit 0), with reduced
  build output size. Evidence: `.factory/architecture.log`.
- Independent review found no correctness gaps across two rounds. The reviewer
  also independently passed the initial 19-test suite and typecheck.
- Hosted browser verification caught uncanceled Enter reopening the dialog
  after selection restores focus. Added a failing event-cancellation regression
  and consumed Enter before activation; the unchanged browser checks now pass.
- Hosted web CI passed for code SHA
  `5e9d14ddbd2df9e9694879d942b833f2512dd7c8`, including build, 20 tests, all 12
  existing browser tests, both new desktop/mobile workflows, and Wrangler
  dry-run. Evidence: https://github.com/thedjpetersen/market-terminal/actions/runs/39177902644/job/117824275836
- Attempted to retain CI browser screenshots, but GitHub rejected the push
  because its OAuth token lacks `workflow` scope. Removed the upload step in a
  new commit, preserving history and the original workflow. No credentials or
  permissions changed. Existing CI can run browser tests; screenshot review
  still needs a permitted harness because that workflow does not export them.
- Separate CI finding: Node 24.21.0 rejects `export type` in the unchanged
  generated-contract syntax-check step. The same command passes locally on Node
  26.7.0; neither the generated contract nor that CI command changed in this task.
  Track this as a separate Node 24 CI-compatibility follow-up.
- Handoff: https://github.com/thedjpetersen/market-terminal/pull/11 is a draft;
  branch `factory/l00372-f1-prevent-stale-search-result-activatio` is pushed.
  No deployment, merge, or data migration occurred; no task server remains.
- Final documentation commit was blocked by the sandbox denying creation of
  `.git/worktrees/L00372/index.lock`. Implementation through `5e9d14d` is already
  pushed; only the final `PROGRESS.md` and `BLOCKED.md` updates remain uncommitted
  for the supervisor, per factory finish policy. The PR description is updated.
- Next: a permitted harness/L00371 should run `npm run test:search --prefix web`,
  review all six ready/pending/error screenshots, and copy them into
  `.factory/screenshots/`. `features.json` remains false and the factory result
  is partial solely because screenshot acceptance has not been completed.
  Repair the separate generated-contract CI compatibility issue before merging.

## L00490 — complete L00372 screenshot acceptance

- Assigned branch: `factory/l00490-complete-l00372-screenshot-acceptance-wi`.
- Startup HEAD/main: `afb01cacb17a5d3a1e652e534191298a3e106564`; clean worktree.
  Read repository/web guidance and L00371/L00372 progress, designs and blockers.
- The assigned baseline does not yet contain L00372: `npm run test:search
  --prefix web` initially exits 1 because the script is absent. The required
  upstream exists at `factory/l00372-f1-prevent-stale-search-result-activatio`.
- Fast-forwarding to that upstream was denied when Git tried to create
  `.git/worktrees/L00490/ORIG_HEAD.lock`. Per Factory fallback, applied its exact
  diff to this worktree without changing Git metadata. These upstream files
  are uncommitted; supervisor must preserve the upstream history when integrating.
- No L00371 product changes are needed: that branch contains audit evidence and
  broader pending acceptance. Its visual criteria were read, but this task is
  limited to the six F1 search states; it cannot mark the wider V1 audit passed.
- Plan: locked install, unchanged search suite on port 41772, inspect every
  desktop/mobile ready/pending/error capture against DESIGN.md, retain evidence
  under `.factory/screenshots/`, and record fresh results. Product code and
  acceptance assertions will remain identical to L00372.
- Fresh result: locked installs passed. The search suite exited 1: desktop and
  mobile both failed before navigation with the same macOS Mach-port denial.
  Its first attempt also reported the missing client `lossless-json` install;
  the documented client install was then completed. No product fix was made.
- Connected Chrome was available, but automatic approval denied local navigation
  and expressly prohibited alternate browser routes/workarounds. Stopped browser
  work immediately. No screenshots exist and no visual score/pass is assigned.
- Supporting check: unchanged `vitest run tests/search.test.tsx` passed 11/11
  (exit 0). Eight source/design/checklist files match L00372 byte-for-byte.
  Full build, full unit suite and architecture checks were not rerun after the
  browser baseline blocker; prior-task results are not fresh L00490 evidence.
- Stopped the task's Vite server and confirmed no listener on 41772. Evidence
  and exact reproduction details: [L00490 handoff](docs/audits/l00490/README.md).
- Handoff status: **partial**, screenshot acceptance 0/6, `features.json` still
  false. No PR/push/deployment. HEAD remains
  `afb01cacb17a5d3a1e652e534191298a3e106564`; upstream snapshot and this report
  remain uncommitted under the required sandbox fallback. Supervisor must
  integrate upstream history and use a confirmed permitted browser runner;
  this partial result does not authorize publication to main.
