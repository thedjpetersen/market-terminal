# Data-source register

This register is part of the release boundary. A technically reachable URL is
not treated as permission to erase attribution, freshness, entitlement, or
retention constraints.

Across snapshot providers, Monitor retains at most 64 distinct observations per
instrument for an in-process session sparkline. Cached repeats do not add
points, and the trace is neither persisted nor synthesized.

## Alpha Vantage

- **Surfaces:** interactive Markets/Monitor snapshots, Chart daily/weekly
  history, local Alert rule observations, and Spreadsheet `PX_LAST` /
  `PX_CHANGE(..., "1D")` cells.
- **Official documentation:** <https://www.alphavantage.co/documentation/>
- **Authentication:** `ALPHA_VANTAGE_API_KEY`; when absent, the documented
  `demo` key is used and the adapter restricts quote/history requests to IBM.
- **Freshness:** `GLOBAL_QUOTE` without a realtime/delayed premium entitlement
  is end-of-day data. The UI labels it delayed rather than executable/live.
- **Attribution/provenance:** provider ID, source trading day, receive time,
  field, current-day low/high, and quality travel with every usable value.
- **History semantics:** 1M uses recent daily bars; 6M/YTD/1Y require sufficient
  full daily history; 5Y is aggregated into ordered weekly OHLCV bars. The 1D
  view reports permission denied until an entitled intraday adapter is wired.
- **Caching:** successful quotes are retained in process for 60 seconds and
  history for 15 minutes to coalesce identical screen requests. No licensed
  quote/history is written to the repo, workbook, test fixture, screenshot
  input, or telemetry.
- **Failure/entitlement:** rate limits, invalid responses, missing fields, and
  demo-key restrictions become typed unavailable or permission-denied states.
- **Alert boundary:** quote observations are live provider inputs, while alert
  delivery remains simulated/local. Stable evaluation IDs prevent a cached or
  repeated provider observation from counting twice toward debounce.
- **Redistribution:** no redistribution right is assumed. Users are responsible
  for selecting an Alpha Vantage plan appropriate to their display and use.

## Alpaca Market Data

- **Surfaces:** optional replacement provider for interactive Markets/Monitor
  snapshots, Chart/Security history, local Alert observations, and Spreadsheet
  `PX_LAST` / `PX_CHANGE(..., "1D")` cells.
- **Official documentation:** [Market Data API](https://docs.alpaca.markets/us/v1.1/docs/about-market-data-api),
  [stock snapshots](https://docs.alpaca.markets/us/reference/stocksnapshots-1),
  and [historical bars](https://docs.alpaca.markets/us/v1.4.2/reference/stockbars).
- **Authentication:** set `MARKET_TERMINAL_MARKET_DATA_PROVIDER=alpaca`,
  `APCA_API_KEY_ID`, and `APCA_API_SECRET_KEY`. Credentials are sent only in
  Alpaca's documented request headers and are never rendered.
- **Feed/entitlement:** `ALPACA_FEED=iex` is the default and represents the IEX
  venue. `sip` must only be selected with an appropriate consolidated-feed
  entitlement. The explicit feed prevents account-plan defaults from silently
  changing application behavior.
- **Freshness/provenance:** successful snapshots retain provider timestamp,
  receive timestamp, IEX/SIP provider ID, cache status, bid/ask, last, daily
  low/high, change, volume, and realtime quality. Chart series name the selected
  feed.
- **Bounds/caching:** snapshot batches are limited to 200 symbols and cached
  for five seconds. History requests keep at most the newest 10,000-bar page;
  a pagination token becomes a typed range-too-wide error instead of an
  unbounded fetch. Responses are capped at 8 MiB. Query-only providers refresh
  Monitor snapshots on a configurable 5–3,600 second coalescing interval.
- **Failure/entitlement:** HTTP 401/403/422 responses become typed permission
  failures; rate limits retain retry timing; malformed or missing observations
  remain unavailable. Existing Monitor rows remain visibly last-known-good.
- **Redistribution:** no redistribution right is assumed. Users are responsible
  for an Alpaca plan and display use consistent with Alpaca's agreements.

## RSS/Atom publishers

- **Surfaces:** interactive News list/story metadata and the cached-headline
  section of interactive Overview.
- **Default sources:** CNBC Markets, U.S. SEC press releases, and Federal
  Reserve press releases; configurable through `MARKET_TERMINAL_NEWS_FEEDS`.
- **Content boundary:** the feed-provided headline, summary/byline metadata,
  timestamp, attribution, and publisher URL are displayed. The application
  does not fetch or persist publisher article bodies. Opening an article passes
  a validated `http(s)` URL to the system browser.
- **Caching:** bounded in-memory snapshot only; no feed content is committed to
  the repository.

## User portfolio CSV

- **Surfaces:** interactive Portfolio positions and the position/summary
  sections of interactive Overview.
- **Source/ownership:** a local export selected by the user. No broker login or
  API credential is collected.
- **Retention:** parsed positions remain in process; the configured path may be
  retained in the user's ignored environment file. Account identifiers and
  unused columns are not retained.
- **Quality:** snapshot market values are shown as imported values with source
  status, never presented as a streaming quote feed. Overview does not infer
  returns, risk statistics, attribution, or movers from this point-in-time
  snapshot.

## User workbook CSV

- **Surface:** interactive Spreadsheet active sheet.
- **Source/ownership:** a local UTF-8 CSV selected by the user; raw formulas are
  preserved as cell text and recalculated by the local workbook engine.
- **Bounds:** imports are capped at 10 MB, 26 columns, and 100 rows. Export uses
  the active sheet's minimal populated range.
- **Retention/writes:** no workbook CSV is uploaded. New exports refuse to
  replace an existing file; `SHEET EXPORT!` is the explicit replacement form
  and uses a same-directory temporary file before rename.
- **Market data:** resolved `PX_LAST`/`PX_CHANGE` values are overlays and are not
  written into the raw CSV in place of formulas.

## SEC EDGAR structured data

- **Surfaces:** interactive Find instrument master and Security identity,
  reported financials, issuer reference fields, and recent filing metadata.
- **Official source:** <https://www.sec.gov/files/company_tickers.json> and the
  SEC's [submissions and company-facts APIs](https://www.sec.gov/search-filings/edgar-application-programming-interfaces),
  documented through its [developer resources](https://www.sec.gov/about/developer-resources).
- **Identity:** each equity receives a zero-padded canonical CIK identity such
  as `sec:cik:0000320193`; ticker and legal company name remain searchable
  attributes rather than identity keys.
- **Fair access:** requests use an application/contact user agent. Forks should
  set `MARKET_TERMINAL_SEC_USER_AGENT` to their own contact as described by the
  SEC's [EDGAR access guidance](https://www.sec.gov/search-filings/edgar-search-assistance/accessing-edgar-data).
- **Caching and revision:** the company master and successful Security pages are
  retained in process for 15 minutes. Find and Security use bounded background
  workers. Security `F9` invalidates its page cache before fetching; Find `F9`
  requests a coalesced master refresh.
- **Failure behavior:** loading and transport/HTTP/shape failures are visible
  in the Find header. No demo identity is inserted into the interactive app.
- **Fundamental methodology:** annual values use latest-filed comparable
  US-GAAP facts from 10-K fiscal-year durations. Values retain neither analyst
  estimates nor invented gap filling. SEC does not define peer sets; ownership
  normalization remains visibly unavailable.
- **Content boundary:** SEC data is issuer reference, reported facts, and filing
  metadata—not a market-price source. Master responses are limited to 2 MiB,
  submissions/company facts to 8 MiB, and none are persisted.

SEC/Federal Reserve RSS entries use the publisher URLs above. Structured EDGAR
ownership forms and official economic series remain planned adapters and are
not current market-price substitutes.
