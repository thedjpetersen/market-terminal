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
