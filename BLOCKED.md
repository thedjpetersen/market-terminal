# L00372 — screenshot acceptance

The search implementation, deterministic React tests, and hosted desktop/mobile
Chromium checks pass. Local Chromium execution is blocked by this worker's macOS
sandbox:

```text
FATAL:base/apple/mach_port_rendezvous_mac.cc:159
bootstrap_check_in org.chromium.Chromium.MachPortRendezvousServer:
Permission denied (1100)
```

The failure occurs before a page loads, so it provides no application verdict.
The supported computer-use browser inventory also contains no browser surfaces.
No sandbox permissions were changed and no workaround bypass was attempted.

Required handoff to a permitted harness (coordinate with L00371): run
`npm run test:search --prefix web` and review all six desktop/mobile
ready/pending/failure screenshots against `DESIGN.md`. Copy the screenshots
from `web/test-results/` into `.factory/screenshots/` as factory evidence.
This suite starts and stops its own local Vite server on port 41772; it needs no
production access, credentials, or deployment.

Hosted GitHub CI passed the final fix:
https://github.com/thedjpetersen/market-terminal/actions/runs/39177902644/job/117824275836
Its existing workflow does not export web screenshots, and GitHub rejected
an artifact-upload workflow change because the OAuth token lacks `workflow`
scope. That change was removed in a new commit without rewriting history. Local
Chromium remains unavailable; screenshot review still requires a permitted
harness. Do not broaden credentials or permissions as part of this task.

## L00490 — blocker reproduced on 2026-10-18

**Still partial. No screenshots captured or reviewed.**

The task started at `afb01cacb17a5d3a1e652e534191298a3e106564`, which does not
contain the search script. L00372's exact snapshot
`fde8fe3a5387de397697091797672f1a7f76303d` is now present as uncommitted changes
in this assigned worktree; Git refused `ORIG_HEAD.lock`, so ancestry is unchanged.

- `npm run test:search --prefix web` exited 1. Both projects failed at Chromium
  launch with `bootstrap_check_in ... MachPortRendezvousServer: Permission
  denied (1100)`, before any page, interaction or screenshot.
- Connected Chrome is now discoverable, but automatic approval rejected
  navigation to `http://127.0.0.1:41772/?view=watchlist`: permission was declined.
  The rejection explicitly prohibits indirect execution, raw CDP, alternate
  browser surfaces and other workarounds. None was attempted after the denial.
- The Vite process was stopped. No listener remains on port 41772.
- The unchanged search component checks pass 11/11. They do not satisfy the
  six-image browser acceptance. `features.json` remains unchanged and false.

Unblock question for the supervisor: **Which runner is explicitly permitted to
launch Chromium and access this local app, with screenshot files available to
the worker?** Resolve that capability before redispatching this acceptance;
another identical Codex sandbox retry will not address either restriction.
Coordinate with existing L00426/L00489 instead of claiming the wider V1 passed.

See [the L00490 handoff](docs/audits/l00490/README.md) for commands, exact source
revisions and evidence. No PR, push, deployment or permissions change occurred.
The current Factory policy supersedes historical upstream PR instructions.
