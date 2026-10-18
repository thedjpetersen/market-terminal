# Market Terminal web

Production: **https://market.frodojo.com**

An authenticated research workspace, deployed as Cloudflare Worker
`market-terminal` with Workers Static Assets. It serves markets, ticker search,
price charts, headlines, SEC financials and filings, a local watchlist, research
snapshots, and options, fixed-income and moving-average backtest models.

## Development

Requirements: Rust stable, `wasm32-unknown-unknown`, wasm-pack 0.13.1+, Node 24+.
From the repository root:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-pack --locked --version 0.13.1
npm ci --prefix contracts/web/client
npm ci --prefix web
npm run build --prefix web
```

The browser suite automatically runs `tests/local-host.mjs`. It starts the Worker on 8899 and a loopback-only test identity provider on 8900.
The browser tests obtain short-lived, freshly signed test JWTs from that provider;
the production Worker has no authentication bypass. The direct `npm run preview`
command uses production authentication settings and will reject unsigned requests.

```sh
npm run check --prefix web
npm test --prefix web
cd web
npx playwright install chromium
npm run test:browser
```

Unit and HTTP integration checks replay **every native engine fixture through
the actual compiled WASM**, including values above JavaScript's safe-integer
limit. Authentication tests cover absent, forged, expired, future, wrong-audience,
wrong-issuer and unapproved-user tokens. HTTP tests exercise private assets,
sessions, body caps, methods, cross-origin rejection and per-user rate limiting.
Browser tests exercise both desktop and 390-pixel touch layouts, all three models, saving,
downloads, search, watchlist persistence and provider failure states. The normal
browser suite uses explicitly deterministic provider fixtures; the engine is real.
To exercise real providers after deployment:

```sh
MARKET_WEB_URL=https://market.frodojo.com MARKET_LIVE=1 MARKET_STORAGE_STATE=/private/path/session.json npm run test:browser
```

## Deployment

`wrangler.jsonc` declares the account, custom domain, assets binding, SPA fallback,
API routing and observability. No market-data API secret is required.
Cloudflare Access application `feafb6c5-40f9-4eb0-a389-6b7d269dd594` protects the
entire hostname. Google and email one-time-code login allow only
`thedjpetersen@gmail.com`, with a 24-hour session. The Worker independently
verifies the Access JWT signature, issuer, audience, expiry and email allowlist
before serving any API or asset. `workers.dev` and preview URLs are disabled.

To add a user, update both the Access application policy and `ACCESS_EMAILS` in
`wrangler.jsonc`; neither change alone grants access. Access owns interactive
sessions and revocation. No passwords or long-lived bearer secrets are embedded
in the app. With Cloudflare Wrangler authentication available:

```sh
npm run deploy --prefix web
```

This builds the Rust WASM, checks TypeScript, builds the frontend, uploads the
Worker/assets and attaches `market.frodojo.com`. The initial deployment used the
authenticated Cloudflare connector's direct asset-upload and Worker APIs; no
account token is stored in the project. The CI web job checks builds, fixtures,
browser workflows and Wrangler dry-run without publishing automatically.

The deployment on 2026-09-05 is Worker version
`03c4dacc-42fb-438f-a3d9-f2f19bd03331`. Google sign-in and production option,
bond and backtest execution were checked in a real browser.

Use `/api/health` and `/api/session` after signing in for deployment smoke checks.
`POST /api/engine` accepts the shared engine request JSON and returns its exact
response JSON. All four operations enter `AnalyticalApplicationService` with a
verified principal and server-owned analytical capabilities. Results and sessions
are never cached. Requests have a 4 MiB body cap, a 10-second body-read deadline,
a 30-second Worker CPU budget, and 30 model requests per principal per minute
per Cloudflare location. The platform limiter is approximate, not a global quota.
Shared Rust application budgets still bound bars and comparison points.
Zone browser-integrity rules may reject generic command-line user agents; a normal browser or a browser user agent
can verify the route. Existing zone-wide security settings are unchanged.

## Ownership and product limits

- `src/features/research` owns research contracts and screens; `models` owns its
  analytical port and inputs; `watchlist` owns its consumer contract; `library`
  owns saved evidence. Shared `ui` contains domain-free primitives.
- `src/bootstrap.tsx` wires features and adapters. `src/infrastructure` handles
  the authenticated engine HTTP client. `worker/infrastructure` owns Yahoo and SEC wire
  formats, bounded response reads, provider URLs and caching.
- `crates/market-terminal-wasm` enters `AnalyticalApplicationService` through
  an authenticated edge export. The Worker obtains identity only from a verified
  Access JWT and capabilities only from deployment configuration. The existing
  local browser export remains available for fixture parity and grants no remote
  artifact, tenant or credential access.
- The engine runs as compiled WASM inside the Cloudflare Worker. The lossless
  client sends exact JSON integer tokens; neither HTTP boundary parses and
  rounds engine evidence through JavaScript numbers. Downloads retain original
  request/result text. Chart coordinates alone use display-rounded numbers.
- Watchlists (32 tickers) and snapshots (30) remain in localStorage on this
  browser. Storage failures are visible. Download research to back it up; there
  is no cloud synchronization, portfolio import or trade execution.
- Yahoo is a public, unofficial, potentially delayed source. SEC coverage is
  for matched US reporting companies. Provider outages are shown explicitly;
  the app never substitutes fabricated market prices. Search may fall back to
  a small reference catalog or direct ticker lookup when the provider is down.
- Option and bond defaults are hypothetical assumptions. Backtests use a year
  of daily OHLC, next-bar execution and configured costs; they exclude dividends
  and do not claim an adjusted total-return history. Engine methodology and
  disclosures accompany each result.
- The web surface is a mobile research companion. Native portfolio, risk,
  spreadsheet, alert and other advanced terminal workspaces remain native.

The app has a web manifest for Add to Home Screen. It requires connectivity for
market data and first load; it does not promise offline data freshness.

## Readiness audit

The [2026-10-18 readiness and accessibility audit](../docs/audits/l00309/README.md)
records current local checks, source-confirmed gaps and bounded follow-ups.
Desktop/mobile browser verification and screenshots remain pending because the
audit runner could not obtain permitted browser access; passing HTTP tests do
not establish UI readiness.
