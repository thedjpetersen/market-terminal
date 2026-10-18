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
- Plan: reproduce through the real Search component with a deferred provider and
  controlled browser clock; bind activation to the successful current request;
  run desktop/mobile regressions, existing checks, and a separate diff review.
- Acceptance: current search results open by keyboard and pointer; previous
  results never open during debounce, loading, or failure; late completions do
  not replace newer results, including returning to an earlier query.
- Rollback: revert this task's commits. No deployment or data migration is needed.
- Reproduction: the new React/DOM suite produces 5 failures and 5 passes against
  the original component. Enter selects AMZN during debounce/loading and AAPL
  after failure, returning to an earlier query, or changing the provider.
- Browser blocker: Chromium exits before opening a page because macOS denies
  `bootstrap_check_in ... MachPortRendezvousServer` (permission denied 1100).
  Deterministic component tests use jsdom instead. Desktop/mobile browser
  checks remain provided for execution in a permitted environment.
