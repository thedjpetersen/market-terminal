# Data-source register

This register is part of the release boundary. A technically reachable URL is
not treated as permission to erase attribution, freshness, entitlement, or
retention constraints.

## Alpha Vantage

- **Surfaces:** interactive Monitor snapshots and Spreadsheet `PX_LAST` /
  `PX_CHANGE(..., "1D")` cells.
- **Official documentation:** <https://www.alphavantage.co/documentation/>
- **Authentication:** `ALPHA_VANTAGE_API_KEY`; when absent, the documented
  `demo` key is used and the adapter restricts requests to IBM.
- **Freshness:** `GLOBAL_QUOTE` without a realtime/delayed premium entitlement
  is end-of-day data. The UI labels it delayed rather than executable/live.
- **Attribution/provenance:** provider ID, source trading day, receive time,
  field, and quality travel with every usable value.
- **Caching:** successful quotes are retained in process for 60 seconds to
  coalesce identical screen requests. No licensed quote is written to the repo,
  workbook, test fixture, screenshot input, or telemetry.
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

## SEC and Federal Reserve feeds

SEC/Federal Reserve RSS entries use the publisher URLs above. Structured EDGAR
company tickers, submissions, company facts, and official economic series are
planned adapters, not current market-price substitutes. Their addition must
record API-specific fair-access, user-agent, caching, and revision behavior in
this register before interactive wiring.
