# L00373 browser acceptance

Question for the supervisor: can the desktop/mobile modal suite be run by an authorized browser worker with permission to visit this worktree's local server?

- Terminal Playwright Chromium failed before opening a page: `bootstrap_check_in org.chromium.Chromium.MachPortRendezvousServer: Permission denied (1100)` (SIGTRAP).
- Connected Chrome request to `http://127.0.0.1:41773/?view=saved` was rejected by browser security policy because permission was declined. Do not retry using another browser surface or indirect access.
- Existing browser smoke command: `MARKET_WEB_URL=http://127.0.0.1:41773 npm run test:browser -- workflows.browser.ts --grep 'research, watchlist|provider failures' --workers=1` (from `web/`). All four cases failed at browser launch, before application assertions.
- Continue code and DOM-level checks here. Real scrolling, layout, screenshots and desktop/mobile browser acceptance remain unverified until an authorized worker runs them. No production deployment is needed.

The completed fix now has focused acceptance in `web/tests/modals.browser.ts`.
With authorization, start `npm run dev --prefix web -- --port 41773 --strictPort`,
then from `web/` run
`MARKET_WEB_URL=http://127.0.0.1:41773 npm run test:browser -- modals.browser.ts --workers=1`.
Review `.factory/screenshots/{evidence,search,long-search}-{desktop,mobile}.png`
and stop the Vite process. DOM tests cannot validate these screenshots or real
scroll geometry.

## Git handoff

`git add` failed because the sandbox cannot create
`/Users/thedjpetersen/code/market-terminal/.git/worktrees/L00373/index.lock`.
No commit, push or PR was attempted after this denial. Supervisor must commit
the worktree changes (excluding ignored `FACTORY_RESULT.json` and `.factory/`),
push `factory/l00373-f2-establish-one-active-modal-and-scope-` and open the PR.
