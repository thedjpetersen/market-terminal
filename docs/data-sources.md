# Data-source register

This register is part of the release boundary. A technically reachable URL is
not treated as permission to erase attribution, freshness, entitlement, or
retention constraints.

Across snapshot providers, Monitor retains at most 64 distinct observations per
instrument for an in-process session sparkline. Cached repeats do not add
points, and the trace is neither persisted nor synthesized.

## Yahoo Finance chart

- **Surfaces:** default interactive Markets/Monitor snapshots,
  Chart/Security history, local Alert observations, and Spreadsheet `PX_LAST`
  / `PX_CHANGE(..., "1D")` cells.
- **Interface status:** the no-key `query1.finance.yahoo.com/v8/finance/chart`
  interface is not a documented public Yahoo API. It may change or cease to be
  available without notice; the terminal labels it `UNOFFICIAL` and never
  silently falls back to gallery data.
- **Freshness/provenance:** values are conservatively labeled delayed 15
  minutes. Provider timestamp, receive time, cache status, currency, current-day
  low/high, volume, price, and change travel with each usable quote.
- **History semantics:** 1D uses five-minute bars; 1M/6M/YTD/1Y use daily bars;
  5Y uses weekly OHLCV. Null-close bars are dropped. A thin bar may fill missing
  open/high/low from its own close; no cross-bar or synthetic price is created.
- **Bounds/caching:** provider symbols are path-safe, responses are limited to
  8 MiB, and identical successful requests are cached only in process for 60
  seconds. Data is not written to the repo, user workbook, screenshots, or
  telemetry.
- **Use boundary:** Yahoo states that Yahoo Finance information is for
  informational purposes, not trading, and must not be redistributed. Users
  are responsible for use consistent with Yahoo's
  [exchange/data-provider notice](https://uk.help.yahoo.com/kb/exchanges-data-providers-yahoo-finance-sln2310.html)
  and [terms](https://legal.yahoo.com/us/en/yahoo/terms/otos/index.html).

Yahoo is a trademark of Yahoo Inc. This project is not affiliated with,
endorsed by, or sponsored by Yahoo.

## Finnhub

- **Surfaces:** optional Markets/Monitor snapshots, local Alert observations,
  Spreadsheet `PX_LAST` / `PX_CHANGE(..., "1D")`, and session-derived Chart and
  Security price marks.
- **Official documentation:** [Finnhub quote API](https://finnhub.io/docs/api/quote).
- **Authentication:** set `MARKET_TERMINAL_MARKET_DATA_PROVIDER=finnhub` and
  `FINNHUB_API_KEY`. The key is sent only in Finnhub's documented
  `X-Finnhub-Token` header and is never rendered.
- **Freshness/provenance:** Finnhub documents `/quote` as real-time for US
  stocks. The adapter retains provider timestamp, receive time, current price,
  change, percent change, and day low/high; it also validates the returned open
  and previous close. It reports no bid/ask or volume because `/quote` does not
  return them.
- **History boundary:** Finnhub documents stock candles as premium. This
  adapter does not call that endpoint or present quote samples as provider
  candles. It keeps at most 600 distinct-timestamp price samples in process,
  updates duplicate timestamps in place, renders each as a flat OHLC mark, and
  labels chart history `DERIVED · SESSION ONLY · NO PROVIDER CANDLES`. The
  series resets at process exit.
- **Bounds/failure:** symbols are validated, quote responses are capped at 1
  MiB, successful quotes are cached for 15 seconds, HTTP 429 becomes a typed
  rate limit, and missing/rejected keys become permission-denied states.
- **Redistribution:** no redistribution right is assumed. Users are responsible
  for selecting a Finnhub plan and display use consistent with Finnhub's terms.

## Alpha Vantage

- **Surfaces:** interactive Markets/Monitor snapshots, Chart daily/weekly
  history, local Alert rule observations, and Spreadsheet `PX_LAST` /
  `PX_CHANGE(..., "1D")` / `HISTORY(...)` cells.
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
  Spreadsheet `HISTORY` returns the latest official daily observation in the
  requested inclusive ISO-date interval. Supported scalar fields are `PX_OPEN`,
  `PX_HIGH`, `PX_LOW`, `PX_LAST`, and `VOLUME`; an empty interval remains
  unavailable instead of being forward-filled.
- **Caching:** successful quotes are retained in process for 60 seconds and
  history for 15 minutes to coalesce identical screen requests. No licensed
  quote/history is written to the repo, workbook, test fixture, screenshot
  input, or telemetry.
- **Failure/entitlement:** rate limits, invalid responses, missing fields, and
  demo-key restrictions become typed unavailable or permission-denied states.
- **Alert boundary:** quote observations are live provider inputs, while alert
  delivery remains simulated/local. Stable evaluation IDs prevent a cached or
  repeated provider observation from counting twice toward debounce. Complete
  bounded rule runtime state is persisted locally so this idempotency,
  debounce, acknowledgement, and audit state survive application restart.
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
- **Default sources:** Seeking Alpha news and investment ideas, Bloomberg
  Markets, MarketWatch Top Stories, Financial Times Markets, U.S. SEC press
  releases, and Federal Reserve press releases; configurable through
  `MARKET_TERMINAL_NEWS_FEEDS`.
- **Content boundary:** the feed-provided headline, summary/byline metadata,
  timestamp, attribution, publisher URL, and any feed-provided body are
  displayed. On explicit reader activation, the background worker may fetch the
  publisher page and apply readability extraction. It does not bypass access
  controls; unavailable full text stays visibly excerpt-only. Opening the web
  source passes a validated `http(s)` URL to the system browser.
- **Caching:** bounded in-memory snapshots and article bodies only; no feed or
  extracted article content is persisted or committed to the repository.

## User portfolio CSV

- **Surfaces:** interactive Portfolio positions and the position/summary
  sections of interactive Overview.
- **Source/ownership:** a local export selected by the user. No broker login or
  API credential is collected.
- **Retention:** parsed positions remain in process and raw CSV contents are not
  copied. A successful import stores only the selected path in a private,
  crash-safe feature document (or the ignored environment file can provide an
  override). Raw broker account identifiers are replaced with import-local
  labels; unused columns are not retained.
- **Quality:** snapshot market values are shown as imported values with source
  status, never presented as a streaming quote feed. Exact-minor-unit totals
  reconcile independently by ISO currency with no invented FX conversion.
  Unpriced positions remain visible and excluded from an explicitly incomplete
  NAV. Each snapshot carries a deterministic input version, valuation time,
  methodology, and missing-data disclosures. Overview does not infer returns,
  attribution, movers, volatility, or drawdown from this point-in-time
  snapshot. Risk uses only explicit market values for per-currency
  concentration and a labeled parallel non-cash shock; it does not infer an FX
  rate, historical distribution, or executable price.

## User portfolio activity CSV

- **Surface:** Portfolio Activity, Portfolio Performance input coverage, and the
  assistant's bounded read-only `portfolio_get_activity` tool.
- **Source/ownership:** an explicit local cash-account or broker activity export
  selected by the user. Supported aliases cover dated actions, amounts,
  symbols, quantities, fees, accounts, and currencies. The Monarch cash CSV
  convention is documented by [Monarch's official import/export
  guide](https://help.monarchmoney.com/hc/en-us/articles/4409682789908-Import-data-manually-from-banks-or-other-finance-apps):
  positive is income and negative is expense.
- **Retention/privacy:** parsed activity remains in process and raw CSV contents
  are not copied. Only the separately selected path is stored in a private
  crash-safe feature document. Raw account identifiers are replaced with
  import-local labels before reaching the domain or AI tool.
- **Bounds:** files are capped at 10 MB, 100,000 rows, 256 columns, 96 display
  characters per description, 50 rows per AI tool response, and 20 optional
  symbol filters. Any invalid dated or monetary row refuses the whole import.
- **Quality:** provider signs are preserved; dedicated fees become non-negative
  cost magnitudes; totals reconcile exactly in minor units by currency; missing
  currency is visibly defaulted to USD; and no FX rate is invented. Cash-account
  activity is explicitly not verified broker trade history. Activity without
  dated valuations does not produce TWR, contribution, or attribution.

## User portfolio performance CSV

- **Surface:** Portfolio Performance.
- **Source/ownership:** a local dated valuation export selected by the user.
  Required inputs are date and portfolio value; external flow, benchmark value,
  and reporting currency are optional and explicitly disclosed when absent.
- **Retention/bounds:** parsed values remain in process. Only the independent
  selected path is stored in private crash-safe state. Files are capped at 10
  MB, 100,000 data rows, and 64 columns; any malformed row refuses the import.
- **Quality:** exact-money sub-period returns remove end-of-period external flows
  before linking into TWR. Benchmark and active return are calculated only from
  a complete benchmark column. Returns remain separate by currency, every
  snapshot has a deterministic input version and methodology, and no
  contribution or attribution is inferred.

## User portfolio contribution CSV

- **Surface:** Portfolio Contribution and Security drill-through from each
  resolved symbol row.
- **Source/ownership:** a local security-level, single-period valuation export
  selected by the user with `PORT IMPORT CONTRIBUTION <CSV>` or
  `MARKET_TERMINAL_PORTFOLIO_CONTRIBUTION_CSV`. Required inputs are period
  start/end, symbol, beginning value, and ending value. Account, end-of-period
  external flow, paired benchmark values, and currency are optional.
- **Retention/privacy:** parsed rows remain in process and raw CSV contents are
  not copied. Only the independent selected path is stored in a private,
  crash-safe feature document. Raw account identifiers become import-local
  labels before reaching the domain or UI.
- **Bounds:** files are capped at 10 MB, 25,000 data rows, and 64 columns; the
  header must occur within the first 32 records. At most eight row-level
  rejection details are displayed, and malformed or mixed-period input refuses
  the whole import.
- **Quality:** exact minor-unit gain/loss and additive contribution reconcile by
  currency. Benchmark beginning/end values must form a complete pair on every
  row before benchmark or active contribution is calculated. Missing flows are
  explicitly treated as zero, missing currency is visibly defaulted to USD,
  and centibasis-point rounding residuals remain visible. No FX conversion,
  multi-period linking, or inferred history is introduced.

## User portfolio open-tax-lot CSV

- **Surface:** Portfolio Lots and Security drill-through from each resolved
  symbol row.
- **Source/ownership:** a local broker open-lot export selected by the user. No
  broker credential is collected. Required inputs are symbol, acquired date,
  positive quantity, and total cost basis; account, provider holding-period
  term, current value, and currency are optional.
- **Retention/privacy:** lot data remains in process and CSV contents are not
  copied. Only the independent path is stored in private crash-safe state.
  Broker account identifiers become import-local labels before reaching the
  domain or UI.
- **Bounds:** files are capped at 10 MB, 100,000 data rows, 128 columns, and
  eight displayed rejection details. A malformed security, date, quantity,
  basis, value, or currency refuses the whole import.
- **Quality:** cost basis, priced basis, current value, and unrealized gain
  reconcile exactly in minor units by currency. Unpriced lots remain visible
  and excluded from value/gain, unknown provider holding periods remain
  explicit, and no FX rate is invented. The displayed as-of is the local import
  time because this schema has no provider valuation timestamp. This snapshot
  is not closed-trade history, a realized-gain ledger, tax advice, contribution,
  or attribution.

### Portfolio closed-lot CSV

- **Role:** optional local broker export for closed lots and realized gains,
  selected with `PORT IMPORT REALIZED <CSV>` or
  `MARKET_TERMINAL_PORTFOLIO_REALIZED_GAINS_CSV`.
- **Required evidence:** symbol, acquisition date, disposal date, positive
  quantity, non-negative proceeds, and non-negative cost basis on every row.
  Optional provider gain/loss must exactly equal proceeds less basis.
- **Bounds:** 10 MB, 100,000 data rows, 128 columns, and a header within the
  first 32 records; at most eight row-level rejection details are displayed.
- **Quality:** rows are rejected atomically when dates, currency, quantity,
  money, chronology, or reported reconciliation is invalid. Exact minor-unit
  proceeds, basis, realized gain/loss, and holding-period buckets reconcile
  separately by ISO currency; no FX rate is invented.
- **Privacy and persistence:** broker account identifiers are replaced with
  import-local labels. Only the selected path is stored in private crash-safe
  state; the CSV contents are not copied.
- **Limitations:** this is provider-reported closed-lot history, not inferred
  tax-lot matching, a tax return, tax advice, order/fill history, contribution,
  or attribution.

### Portfolio broker execution CSV

- **Role:** optional local broker order/fill export selected with
  `PORT IMPORT TRADES <CSV>` or `MARKET_TERMINAL_PORTFOLIO_TRADES_CSV`.
- **Required evidence:** execution timestamp or trade date, buy/sell side,
  symbol, positive quantity, and positive execution price on every row.
  Account, broker order ID, gross amount, commission, fees, signed net amount,
  and currency are optional.
- **Bounds:** 10 MB, 100,000 data rows, 128 columns, and a header within the
  first 32 records; at most eight row-level rejection details are displayed.
- **Quality:** quantity and price retain six decimal places. Gross is rounded
  once into the currency's exact minor units and, when supplied, must match
  quantity × price. Buy net cash is `-(gross + fees)` and sell net cash is
  `gross - fees`; conflicting supplied values reject the entire import.
  Currency totals and timestamp-precision limitations remain explicit, and no
  FX conversion is invented.
- **Privacy and persistence:** broker account and order identifiers are
  replaced with import-local labels. Only the selected path is stored in
  private crash-safe state; CSV contents are not copied.
- **Limitations:** this is read-only execution history. It cannot stage, route,
  submit, cancel, or modify an order and does not infer fills from cash
  activity.

## User workbook and CSV

- **Surface:** interactive Spreadsheet workbook and active-sheet CSV exchange.
- **Source/ownership:** a local UTF-8 CSV selected by the user; raw formulas are
  preserved as cell text and recalculated by the local workbook engine.
- **Bounds:** imports are capped at 10 MB, 26 columns, and 100 rows. Export uses
  the active sheet's minimal populated range.
- **Retention/writes:** no workbook CSV is uploaded. New exports refuse to
  replace an existing file; `SHEET EXPORT!` is the explicit replacement form
  and uses a same-directory temporary file before rename. Complete workbooks
  autosave as bounded, schema-versioned feature documents in the user's local
  application state with atomic replacement and previous-generation recovery.
- **Market data:** resolved financial values are overlays and are not written
  into raw formulas. Live quote adapters cover `PX_LAST` and `PX_CHANGE`.
  Selecting Alpha Vantage also resolves daily `HISTORY`; reported annual
  `FUNDAMENTAL` values route independently to SEC Company Facts. Unsupported
  fields and provider entitlement failures remain typed, visible states. The
  deterministic gallery remains presentation input, never an interactive-data
  fallback.

## SEC EDGAR structured data

- **Surfaces:** interactive Find instrument master; Security identity, reported
  financials, issuer reference fields, recent filing metadata, and recent Form
  4/4-A non-derivative insider transactions; and Spreadsheet `FUNDAMENTAL`
  cells.
- **Official source:** <https://www.sec.gov/files/company_tickers.json> and the
  SEC's [submissions and company-facts APIs](https://www.sec.gov/search-filings/edgar-application-programming-interfaces),
  documented through its [developer resources](https://www.sec.gov/about/developer-resources).
  Ownership XML follows the SEC's current [Forms 3/4/5 technical specifications](https://www.sec.gov/submit-filings/technical-specifications).
- **Identity:** each equity receives a zero-padded canonical CIK identity such
  as `sec:cik:0000320193`; ticker and legal company name remain searchable
  attributes rather than identity keys.
- **Fair access:** requests use an application/contact user agent. Forks should
  set `MARKET_TERMINAL_SEC_USER_AGENT` to their own contact as described by the
  SEC's [EDGAR access guidance](https://www.sec.gov/search-filings/edgar-search-assistance/accessing-edgar-data).
- **Caching and revision:** the company master, successful Security pages, and
  at most 32 / 64 MiB of successful Company Facts payloads are retained in
  process for 15 minutes. Find and Security use bounded background workers.
  Security `F9` invalidates its page and Company Facts entries before fetching;
  Find `F9` requests a coalesced master refresh.
- **Form 4 methodology:** Security examines at most six recent Form 4/4-A
  entries from submissions metadata, fetches each raw ownership XML document
  with a 1 MiB bound, and retains at most 40 non-derivative transactions. The UI
  displays transaction code, acquisition/disposition, reported shares and
  price-derived value, owner role, direct/indirect ownership, and filing-level
  10b5-1 status without a relevance or trading signal. A selected row opens the
  official SEC filing index through a validated `http(s)` URL.
- **Form 4 visualization:** the log-value scatter and weekly
  acquisition/disposition bars use only those loaded transactions and reported
  price-derived values. Unpriced transactions are not assigned a dollar value.
  The chart and rollup are labeled `LOADED SAMPLE`; they do not claim complete
  12-month coverage. Clicking the plot selects the nearest dated transaction.
- **Failure behavior:** loading and transport/HTTP/shape failures are visible
  in the Find header. No demo identity is inserted into the interactive app.
- **Fundamental methodology:** annual values use latest-filed comparable
  US-GAAP facts from 10-K fiscal-year durations. Spreadsheet supports
  `REVENUE`, `OPERATING_INCOME`, `NET_INCOME`, and `DILUTED_EPS` for `FY####`
  (or `FY####A`) periods and retains raw USD or USD/share values with fiscal
  period-end provenance. Values retain neither analyst
  estimates nor invented gap filling. SEC does not define peer sets; ownership
  beyond the current Form 4 transaction slice remains visibly unavailable.
- **Content boundary:** SEC data is issuer reference, reported facts, and filing
  metadata—not a market-price source. Master responses are limited to 2 MiB,
  submissions/company facts to 8 MiB, Form 4 XML to 1 MiB per filing, and none
  are persisted.

SEC/Federal Reserve RSS entries use the publisher URLs above. Structured EDGAR
institutional ownership forms and official economic series remain planned
adapters and are not current market-price substitutes.
