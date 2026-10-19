# L00426 — resume V1 with a permitted browser runner

## Scope and decisions

- Assigned branch: `factory/l00426-resume-v1-with-a-permitted-browser-runne`.
- Product baseline and local `main`: `afb01cacb17a5d3a1e652e534191298a3e106564`.
- Read AGENTS.md, README.md, web/README.md, the existing browser suites, and
  L00371's handoff at `5f8b537` plus L00309's audit at `8b223c9` through Git.
  PROGRESS.md and features.json were absent on the assigned starting branch.
- Gate expensive setup on confirmed browser access. Do not repeat the previous
  standalone Chromium launch or full install/build while permission is unresolved.
- F1/F2/F3 remain owned by L00372/L00373/L00374. Their branches are not integrated
  into this baseline; this attempt neither implements nor verifies those fixes.

## Done

- Discovered the connected Chrome extension through the supported CUA API and
  read its local-development guidance.
- Started one minimal, loopback-only preflight server at `127.0.0.1:42600`.
  It served static capability-check text, with no app data or credentials.
- Requested local navigation through CUA once. Automatic approval review denied
  it and explicitly prohibited alternative browser routes. Browser work stopped.
- Stopped the preflight server through its owning exec session; verified that
  port 42600 no longer accepts connections. No other task servers were started.
- Verified original browser config, host, fixtures and assertions are byte-for-byte
  identical to the baseline. Product/dependency files are unchanged.
- Recorded the exact blocker and pending acceptance in
  [the L00426 handoff](docs/audits/l00426/README.md) and [BLOCKED.md](BLOCKED.md).

## Next / surprises

- **Partial:** no browser render, screenshots, keyboard probes or independent
  browser verification. There is no evidence that access approval was granted.
- No fresh install/build/unit/HTTP suite was run: the browser preflight failed
  before application setup. L00371's historical green checks are not fresh checks.
- The supervisor must arrange an explicitly permitted runner before resuming the
  existing V1 queue. Do not create another identical sandbox retry task.
- No production access, push, PR, deployment, credentials or security settings
  were changed. This partial checkpoint does not authorize publication to main.

## Verification and handoff

See the audit for executed checks and their limits. Browser checklist entries
stay false. Final commit/check information will be recorded before handoff.
