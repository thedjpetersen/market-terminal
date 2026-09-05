# Market Terminal web

Production: **https://market.frodojo.com**

A responsive public research workspace, deployed as Cloudflare Worker
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
npm run preview --prefix web
```

Open http://127.0.0.1:8799. For UI hot reload, also run `npm run dev --prefix web`
and open Vite's printed URL. Vite proxies `/api` to the local Worker on 8799.
The Worker provides the production-equivalent asset headers and WASM execution.

```sh
npm run check --prefix web
npm test --prefix web
cd web
npx playwright install chromium
npm run test:browser
```

Unit checks replay **every native engine fixture through the actual compiled
WASM**, including values above JavaScript's safe-integer limit. Browser tests
exercise both desktop and 390-pixel touch layouts, all three models, saving,
downloads, search, watchlist persistence and provider failure states. The normal
browser suite uses explicitly deterministic provider fixtures; the engine is real.
To exercise real providers after deployment:

```sh
MARKET_WEB_URL=https://market.frodojo.com MARKET_LIVE=1 npm run test:browser
```

## Deployment

`wrangler.jsonc` declares the account, custom domain, assets binding, SPA fallback,
API routing and observability. No market-data secret or private account is required.
With Cloudflare Wrangler authentication available:

```sh
npm run deploy --prefix web
```

This builds the Rust WASM, checks TypeScript, builds the frontend, uploads the
Worker/assets and attaches `market.frodojo.com`. The initial deployment used the
authenticated Cloudflare connector's direct asset-upload and Worker APIs; no
account token is stored in the project. The CI web job checks builds, fixtures,
browser workflows and Wrangler dry-run without publishing automatically.

Use `/api/health` for deployment smoke checks. Zone browser-integrity rules may
reject generic command-line user agents; a normal browser or a browser user agent
can verify the route. Existing zone-wide security settings are unchanged.

## Ownership and product limits

- `src/features/research` owns research contracts and screens; `models` owns its
  analytical port and inputs; `watchlist` owns its consumer contract; `library`
  owns saved evidence. Shared `ui` contains domain-free primitives.
- `src/bootstrap.tsx` wires features and adapters. `src/infrastructure` handles
  HTTP and the browser Worker. `worker/infrastructure` owns Yahoo and SEC wire
  formats, bounded response reads, provider URLs and caching.
- `crates/market-terminal-wasm` enters `AnalyticalApplicationService`. It grants
  only a fixed local-browser execution context, with application workload limits,
  a 4 MiB request cap and a 30-second browser worker lifetime. It never grants
  access to the authenticated HTTP host's artifacts, tenants or credentials.
- The engine is lazy-loaded in a Web Worker. The lossless shared client emits
  bigint as exact JSON integer tokens; downloads retain original request/result
  text. Chart coordinates alone use display-rounded numbers.
- Watchlists (32 tickers) and snapshots (30) remain in localStorage on this
  browser. Storage failures are visible. Download research to back it up; there
  is no login, cloud synchronization, portfolio import or trade execution.
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
