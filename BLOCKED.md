# L00374 browser verification

Implementation and non-browser checks are complete; desktop/mobile browser acceptance is pending.

Required environment: a permitted Playwright Chromium runner able to launch a browser and access the loopback Worker. The current sandbox terminates Chromium before page creation with:

```text
FATAL:base/apple/mach_port_rendezvous_mac.cc:159
bootstrap_check_in org.chromium.Chromium.MachPortRendezvousServer: Permission denied (1100)
```

The attempted browser run is recorded in `.factory/browser-red.log`. Three selected tests failed at browser launch, not at their assertions. No screenshots were produced or reviewed. Do not count the six request-only Playwright HTTP checks as browser acceptance.

Question for the supervisor: can a permitted browser verifier run the complete existing workflow suite plus `web/tests/recovery.browser.ts` on desktop and mobile, review the screenshots, and confirm recovery before publication?

The prepared local-only harness is `web/.factory/playwright.config.ts`; it uses ports 41774/41775 and inspector 41776. Wrangler logs and registry remain inside the worktree:

```sh
cd web
WRANGLER_LOG_PATH="$PWD/.factory/wrangler-logs" WRANGLER_REGISTRY_PATH="$PWD/.factory/wrangler-registry" MARKET_TEST_ISSUER=http://127.0.0.1:41775 npx playwright test --config=.factory/playwright.config.ts
```

The prepared harness and port-only acceptance copies are uncommitted verification artifacts; the normal checked-in suite remains runnable with `npm run test:browser` in an environment where its default ports are allowed. The new tests save session and empty-filings screenshots under `.factory/screenshots/` at the repository root.
