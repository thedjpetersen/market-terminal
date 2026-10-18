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
