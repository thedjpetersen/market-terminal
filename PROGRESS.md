# L00309 — web companion readiness audit

## Scope and decisions

- Audit baseline: `afb01ca` on `factory/l00309-market-frodojo-com-web-companion-readine`.
- Inspect the existing Cloudflare companion, navigation, states, accessibility,
  attribution and offline checks. No product or native-terminal changes.
- Use synthetic browser fixtures and ephemeral local authentication only. The
  unauthenticated production root redirects to Cloudflare Access; no sign-in,
  private data, provider purchases, deployment or infrastructure changes.
- Reference: Bloomberg information density with Linear keyboard clarity.
- Existing browser harness ports 8899/8900 are outside Factory policy. Run
  temporary copies on 41999/42000 without changing the original acceptance tests.
- This worktree initially had no PROGRESS.md or features.json. The new feature
  checklist describes audit completion, not a claim of production readiness.

## Done

- Read repository/web documentation, architecture constraints and existing checks.
- Confirmed a clean task branch and inventoried research, watchlist, models,
  saved evidence, provider adapters and modal/navigation behavior.

## Next

- Complete baseline build, TypeScript, unit/contract and browser checks.
- Capture desktop/mobile synthetic workflows and focused accessibility/state probes.
- Document reproducible gaps and bounded follow-ups, obtain independent review,
  then commit/push and open an audit PR if git permissions allow.

## Evidence

- Local command logs: `.factory/logs/`.
- Browser captures: `.factory/screenshots/` (synthetic data only).

## Surprises / limitations

- Production is an allowlisted research companion, not a public onboarding flow.
- Web browser tooling is being installed from the checked-in lockfile. No
  production settings, credentials or existing tests have been changed.
