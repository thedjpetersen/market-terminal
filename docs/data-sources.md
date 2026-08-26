# Data-source register

This register is part of the release boundary. A technically reachable URL is
not treated as permission to erase attribution, freshness, entitlement, or
retention constraints.

## Alpha Vantage

- **Surfaces:** interactive Monitor snapshots, Chart daily/weekly history, and
  Spreadsheet `PX_LAST` / `PX_CHANGE(..., "1D")` cells.
- **Official documentation:** <https://www.alphavantage.co/documentation/>
- **Authentication:** `ALPHA_VANTAGE_API_KEY`; when absent, the documented
  `demo` key is used and the adapter restricts quote/history requests to IBM.
- **Freshness:** `GLOBAL_QUOTE` without a realtime/delayed premium entitlement
  is end-of-day data. The UI labels it delayed rather than executable/live.
- **Attribution/provenance:** provider ID, source trading day, receive time,
  field, and quality travel with every usable value.
- **History semantics:** 1M uses recent daily bars; 6M/YTD/1Y require sufficient
  full daily history; 5Y is aggregated into ordered weekly OHLCV bars. The 1D
  view reports permission denied until an entitled intraday adapter is wired.
- **Caching:** successful quotes are retained in process for 60 seconds and
  history for 15 minutes to coalesce identical screen requests. No licensed
  quote/history is written to the repo, workbook, test fixture, screenshot
  input, or telemetry.
- **Failure/entitlement:** rate limits, invalid responses, missing fields, and
  demo-key restrictions become typed unavailable or permission-denied states.
- **Redistribution:** no redistribution right is assumed. Users are responsible
  for selecting an Alpha Vantage plan appropriate to their display and use.

## RSS/Atom publishers

- **Surfaces:** interactive News list and story metadata.
- **Default sources:** CNBC Markets, U.S. SEC press releases, and Federal
  Reserve press releases; configurable through `MARKET_TERMINAL_NEWS_FEEDS`.
- **Content boundary:** the feed-provided headline, summary/byline metadata,
  timestamp, attribution, and publisher URL are displayed. The application
  does not fetch or persist publisher article bodies. Opening an article passes
  a validated `http(s)` URL to the system browser.
- **Caching:** bounded in-memory snapshot only; no feed content is committed to
  the repository.

## User portfolio CSV

- **Surface:** interactive Portfolio positions.
- **Source/ownership:** a local export selected by the user. No broker login or
  API credential is collected.
- **Retention:** parsed positions remain in process; the configured path may be
  retained in the user's ignored environment file. Account identifiers and
  unused columns are not retained.
- **Quality:** snapshot market values are shown as imported values with source
  status, never presented as a streaming quote feed.

## SEC EDGAR company tickers

- **Surface:** interactive Find instrument master.
- **Official source:** <https://www.sec.gov/files/company_tickers.json> and the
  SEC's [developer resources](https://www.sec.gov/about/developer-resources).
- **Identity:** each equity receives a zero-padded canonical CIK identity such
  as `sec:cik:0000320193`; ticker and legal company name remain searchable
  attributes rather than identity keys.
- **Fair access:** requests use an application/contact user agent. Forks should
  set `MARKET_TERMINAL_SEC_USER_AGENT` to their own contact as described by the
  SEC's [EDGAR access guidance](https://www.sec.gov/search-filings/edgar-search-assistance/accessing-edgar-data).
- **Caching and revision:** the master is fetched once on a bounded background
  worker and held in memory. `F9` requests a coalesced full refresh; each
  success or failure increments a revision observed by the workspace.
- **Failure behavior:** loading and transport/HTTP/shape failures are visible
  in the Find header. No demo identity is inserted into the interactive app.
- **Content boundary:** this catalog is issuer reference data, not a market
  price source. Responses are limited to 2 MiB and are not persisted.

SEC/Federal Reserve RSS entries use the publisher URLs above. Structured EDGAR
submissions and company facts, plus official economic series, remain planned
adapters and are not current market-price substitutes.
