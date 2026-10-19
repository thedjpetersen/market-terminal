# Factory progress

## L00373 — one active modal and scoped keyboard focus

- Baseline: `afb01ca` on `factory/l00373-f2-establish-one-active-modal-and-scope-`; clean worktree.
- Read repository/web guidance and existing modal, shell, search and library code.
- Installed locked dependencies. Baseline `npm run build --prefix web` passed; `npm test --prefix web` passed 9/9. An early typecheck ran before WASM generation and reported missing generated modules; the completed build's typecheck passed.
- Browser acceptance is blocked by local Chromium sandbox permissions and a denied connected-browser request. See `BLOCKED.md`. No screenshots or visual acceptance claimed.
- Plan: reproduce with DOM integration tests; introduce one domain-free modal owner; bind focus, Escape and scroll cleanup to the owning element; verify and request an independent correctness review.
- Decision: while any modal is active, Cmd/Ctrl-K preserves that modal. Closing it releases ownership, restores its trigger and unlocks scrolling. Search result freshness and session/provider recovery remain separate factory tasks.
- Reproduced the original problem with 12 failing DOM integration cases, including overlapping dialogs, document-wide focus selection, Escape leakage and focus restoration without scroll preservation. Evidence: `.factory/modal-baseline.log`.
- Implemented one domain-free modal owner shared by About, search and saved evidence. Each owner binds its exact element; only that element traps Tab/Escape and manages the body overflow lifecycle. Removed search's duplicate Escape/focus effect, added keyboard access to JSON evidence and a focusable main fallback for navigation.
- Independent verifier found an internal-scroll issue in round 1: wrapped focus must reveal offscreen results. Fixed it and added a 12-result regression; round 2 found no remaining correctness issues. Browser acceptance remains blocked.
- Git staging failed with `Unable to create .../.git/worktrees/L00373/index.lock: Operation not permitted`. Per finish protocol, leave changes uncommitted; supervisor must commit, push the assigned branch and open the PR.
- Final web evidence: `npm test --prefix web` exited 0, 26/26 tests (17 modal regressions, 9 existing tests); `npm run build --prefix web` exited 0 (WASM, TypeScript, Vite); `npm run check --prefix web` exited 0. Logs: `.factory/tests.log`, `.factory/build.log`, `.factory/typecheck.log`.
- Merge coordination: L00372 also touches `Search.tsx`. Its result freshness changes should retain the new `dialogRef` prop and shared modal owner; this task intentionally leaves the search request/result logic unchanged.
- Architecture verification: `CARGO_PROFILE_DEV_DEBUG=0 cargo test --test architecture_boundaries -j 2` exited 0, 10/10 checks. Evidence: `.factory/architecture.log`.
- `git diff --check` and Prettier checks passed. Playwright test discovery listed six desktop/mobile cases; execution remains blocked (the mobile About case skips because the existing trigger is desktop-only). The local Vite server was stopped and port 41773 has no listener.
- Handoff HEAD: `afb01cacb17a5d3a1e652e534191298a3e106564` (baseline unchanged because Git metadata writes are denied). All task changes are uncommitted on the assigned branch. No production changes or preview deployment.
- Rollback: revert the eventual task commit(s); no deployment rollback is needed.
- Next: supervisor commits/pushes/opens the PR, then an authorized browser worker runs the documented desktop/mobile acceptance and reviews screenshots before merge. `FACTORY_RESULT.json` records partial completion because browser acceptance and PR creation remain outstanding.
