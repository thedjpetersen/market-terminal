# L00372 — browser verification

The search implementation and deterministic React tests are complete. Desktop
and mobile Chromium execution is blocked by this worker's macOS sandbox:

```text
FATAL:base/apple/mach_port_rendezvous_mac.cc:159
bootstrap_check_in org.chromium.Chromium.MachPortRendezvousServer:
Permission denied (1100)
```

The failure occurs before a page loads, so it provides no application verdict.
The supported computer-use browser inventory also contains no browser surfaces.
No sandbox permissions were changed and no workaround bypass was attempted.

Required handoff to a permitted harness (coordinate with L00371): can
`npm run test:search --prefix web` pass for desktop and mobile, and can the
generated ready/pending/failure screenshots be reviewed? Copy the screenshots
from `web/test-results/` into `.factory/screenshots/` as factory evidence.
This suite starts and stops its own local Vite server on port 41772; it needs no
production access, credentials, or deployment.
