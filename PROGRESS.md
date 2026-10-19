# Factory progress

## L00374 — Research session and provider recovery

- Baseline: clean task branch at `afb01ca`; installing locked web dependencies and building the WASM/web host before implementation.
- Scope: classify research/backtest-history session responses before JSON parsing, retry provider failures, include headlines in market refresh, explain empty SEC filings, and add focused regression coverage.
- Plan: reproduce through the public adapters and browser controls, implement within web infrastructure/research ownership, run build/typecheck/unit/browser/architecture checks, and commit the verified change.
- Decisions: preserve existing UI primitives and styling; do not change search or modal behavior owned by other factory tasks. Use local-only deterministic providers and ports 41000–49999 for browser verification. No deployment or remote publication.
- Baseline passed: `npm run build --prefix web` (WASM + TypeScript + Vite), `npm test --prefix web` (9/9). A standalone typecheck attempted during WASM compilation initially lacked generated files; it passed after generation.
- Reproduced: new adapter regression suite failed 47/71 cases before the fix, including HTML sign-in responses leaking JSON parser errors and JSON auth errors lacking recovery guidance (`.factory/recovery-red.log`).
- Implemented: infrastructure-owned response classification shared by research, history, and engine clients; useful provider/network fallbacks; unchanged exact engine JSON; overview headline refresh; panel retries; explicit empty SEC filings.
- Checks passed: final build, standalone TypeScript check, 82/82 unit tests, and 6/6 authenticated Worker HTTP checks (all native fixtures, authentication, body/method/origin/rate limits). Logs are in `.factory/`. Existing acceptance assertions were unchanged; temporary HTTP-test copies remapped only localhost ports to 41774/41775, with inspector port 41776.
- Independent read-only review: no correctness or requirement findings; browser fixtures/selectors match the implementation. Browser runtime and visual acceptance remain unverified.
- Remote main was checked with `git ls-remote origin refs/heads/main`: `afb01cacb17a5d3a1e652e534191298a3e106564`, identical to the branch base and local main. `git fetch origin main` could not write the worktree's `FETCH_HEAD`; no integration was needed because the read-only remote check matched.
- Blocked: Chromium cannot launch under this sandbox (`MachPortRendezvousServer`, permission denied). The Worker runs successfully, but no page assertions or screenshots ran. See `BLOCKED.md`; do not publish this partial checkpoint before independent browser acceptance.
- Architecture passed: `CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --test architecture_boundaries` exited 0, 10/10 tests passed (`.factory/architecture.log`). No Rust source or dependency manifests changed.
- Checkpoint: implementation and regression coverage committed as `8401b1e` after the plan commit `e748a6b`. No PR, push, or deployment. All local verification servers stopped; ports 41774, 41775, and 41776 no longer respond.
- Handoff: the change is partial only because browser execution and screenshot review are blocked. Run `cd web && node .factory/run-browser.mjs` in this worktree with a permitted browser runner; it generates port-only copies of the unchanged existing acceptance tests and cleans them afterward. New recovery tests are committed and included in the standard browser suite. `features.json` remains false until full acceptance passes.
- Rollback: revert implementation commit `8401b1e`; no deployed resources changed.
- Final handoff documentation remains uncommitted: the sandbox rejected `git add` with `Unable to create .../worktrees/L00374/index.lock: Operation not permitted`. The implementation and tests are already committed; the supervisor can commit the final `PROGRESS.md` and `BLOCKED.md` updates. `FACTORY_RESULT.json` must stay uncommitted.
