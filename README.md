# Market Terminal

_I pasted this tweet into ChatGPT and just told it to keep going in between boogieboarding_

<img width="592" height="877" alt="image" src="https://github.com/user-attachments/assets/19912571-d2f0-4ef8-8230-bb868e14389a" />

A native Rust, open-source market workstation inspired by the information
density and keyboard ergonomics of professional financial terminals.

The application runs directly in your terminal. Ratatui draws every panel,
table, chart, border, and color; Crossterm handles input and terminal state.
There is no HTML, CSS, JavaScript, WebAssembly, or browser runtime.

## Workspaces

- Overview — imported positions and live cached headlines, with unavailable
  performance fields left explicit rather than synthesized
- Desk — responsive Monitor and Chart panes above News, with Tab/1/2/3 pane
  focus and click routing
- Markets — external listed-instrument snapshots, with unsupported cross-asset
  and analytics datasets called out instead of mocked
- Security — quote/chart, financials, raw SEC Form 4 insider transactions,
  filings, explicitly unavailable estimates/peers, and linked news
- Portfolio — imported positions, reconciled value/weights, and source status
- News — asynchronously refreshed RSS/Atom stories, filters, unread/bookmarks,
  clickable publisher links, and linked securities
- Spreadsheet — workbook-scoped recalculation, cross-sheet and mixed absolute
  references, translated copy/fill, lookups, undo/redo, CSV, and asynchronously
  resolved `PX_LAST`/`PX_CHANGE` cells with explicit data-quality state
- AI — ChatGPT-authenticated Codex analysis and natural-language workspace control,
  with an optional OpenRouter fallback
- Find — canonical instrument identity and ranked symbol/company discovery
- Monitor — configurable watchlists, bounded quote streams, sorting,
  provider day ranges, bounded session sparklines, responsive columns,
  data-quality states, last-known-good fallback, and replay
- Chart — comparative performance, zero baselines, inspection cursor, market
  profile statistics, half-block OHLC candlesticks, volume histograms, SMA/EMA
  overlays, and Wilder RSI
- Chat — TLS-capable IRC market rooms with bounded queues, background reconnect,
  participant presence, notices, actions, and an inline composer
- Alerts — idempotent, debounced local rules with acknowledgement and audit state

Use the labeled navigation keys, click a visible workspace tab, or type a
function such as `MON`, `CHART`, `SHEET`, `CHAT`, `FIND`, or `ASK` into the
command bar. Mouse input is enabled in the interactive terminal: click the
command box and `GO`, select table rows and spreadsheet cells, activate chart
controls and research tabs, focus AI/chat composers, and scroll navigable
lists. News uses live source feeds, Portfolio uses your imported snapshot, and
Overview composes those same real snapshots without performing I/O while
rendering. Persistent workspaces do not substitute deterministic gallery
analytics when an external source is missing; the separate gallery host remains
available for screenshots and tests.

`DESK` (aliases `SPLIT` and `DASHBOARD`) opens the combined workspace adapted
from `alphai-tui`. Press `Tab`/`Shift+Tab` or `1`/`2`/`3` to focus Monitor,
Chart, or News. Clicking inside a pane focuses it and sends subsequent keys to
that pane. On short terminals the News pane yields instead of crushing the
market panels; use `NEWS` for the full feed.

Run `HELP` from the command bar—or press `F1`—to open the command and controls
guide without leaving the current workspace. Close it with `Esc`, `Q`, `F1`,
the on-screen close button, or by selecting a workspace tab.

Run `SETTINGS` (aliases `CONFIG` and `SETUP`) or press `F2` to inspect the
secret-free effective startup configuration. On the first persistent launch,
this setup screen opens automatically once. It shows credential presence but
never credential values, and identifies which `.env` changes require a restart.

Tmux-style panel switching is also available. Press `Ctrl+B`, release it, then
use `Left`/`Right` or `N`/`P` for the next or previous workspace. Use `1`–`9`
and `0` to select the corresponding numbered workspace, or `?` for help.

The command bar starts in `INSERT` mode, so ordinary typing, arrows,
Home/End, Backspace/Delete, Enter, `Ctrl+W`, `Ctrl+U`, and Up/Down command
history work directly. Press `Esc` with a non-empty command to enter optional
Vi `NORMAL` mode: use `h`/`l`, `0`/`$`, `w`/`b`, `x`, `D`, `dd`, and
`i`/`a`/`I`/`A`. Press `Esc` again to cancel the command.

The interactive binary restores the active workspace, workspace order, and
recent commands from crash-safe, versioned local state. Set
`MARKET_TERMINAL_STATE_DIR` to override the platform default. Corrupt current
state falls back to the previous valid generation and never blocks startup.

Press `F3`/`Shift+F3`, click the theme controls in Settings, or run `THEME`
and `THEME PREV` to cycle the color presets live. `THEME NORD` selects a
specific preset; the choices are `default`, Catppuccin Mocha/Macchiato/Frappé/
Latte, Dracula, Gruvbox Dark/Light, and Nord. The selection is persisted with
the shell session. `MARKET_TERMINAL_THEME=catppuccin-mocha` supplies the initial
theme before a saved interactive selection exists.

Shell and shared navigation keys can be remapped without changing feature
code. Set `MARKET_TERMINAL_KEYBINDINGS` to semicolon-separated actions whose
values are comma-separated keys; a listed action replaces its defaults while
unlisted actions keep theirs:

```dotenv
MARKET_TERMINAL_KEYBINDINGS="help=ctrl-h;next_panel=alt-l;previous_panel=alt-h;up=w,k;down=s,j"
```

Key names use `[ctrl-][alt-][shift-]key`, where `key` is a character, `F1`–
`F12`, `enter`, `tab`, `backtab`, an arrow, `home`, `end`, `pgup`, `pgdn`,
`backspace`, `delete`, or `insert`. Available actions are `quit`, `command`,
`next_panel`, `previous_panel`, `settings`, `help`, `next_theme`,
`previous_theme`, `refresh`, `up`, `down`, `left`, `right`, `page_up`,
`page_down`, and `open`. Invalid, duplicate, reserved, and conflicting entries
fall back safely and are counted in Settings. `Esc`, `Ctrl+C`, `Ctrl+B`, direct
workspace hotkeys, the tmux post-prefix keys, and command-mode Vim/Emacs editing
remain fixed escape routes.

## Live news

The interactive app fetches real RSS/Atom feeds on a background thread; network
latency never blocks terminal input or rendering. The defaults are CNBC Markets,
SEC press releases, and Federal Reserve press releases. Press `F9` in News to
refresh immediately. Failed sources are shown as unavailable or degraded—the
interactive app does not replace them with fabricated headlines or calendar
events. Select a story and press `O` or Enter—or click `OPEN ARTICLE`—to open
its original `http(s)` publisher page in your system browser. The terminal does
not scrape or store the article body.

Override the defaults with comma-separated feeds:

```dotenv
MARKET_TERMINAL_NEWS_FEEDS="https://example.com/markets.xml,https://example.com/company-news.xml"
MARKET_TERMINAL_NEWS_REFRESH_SECS=300
```

## Live market data

The interactive Markets, Monitor, Chart, Alerts, Security, and Spreadsheet resolve
quote/history fields on bounded background workers; provider latency never
runs on the input or render thread. The default is Yahoo Finance's delayed,
no-key chart endpoint, adapted from `alphai-tui`. It is an unofficial interface,
is labeled that way in the UI, and may change without notice. Configure any
listed symbols directly:

```dotenv
MARKET_TERMINAL_MARKET_DATA_PROVIDER="yahoo"
MARKET_TERMINAL_WATCHLIST="AAPL,MSFT,NVDA"
MARKET_TERMINAL_MARKETS_SYMBOLS="SPY,QQQ,IWM"
MARKET_TERMINAL_CHART_SYMBOL="AAPL"
```

Yahoo responses are bounded to 8 MiB and cached in process for 60 seconds;
null bars are dropped and missing OHLC fields fall back only to that same bar's
reported close. Quotes, day ranges, volume, and history are explicitly labeled
delayed with provider, source time, and cache status. The app never fills
missing results with generated prices or replayed bars. Press `F9` or click the
Chart header to refresh. See [the data-source register](docs/data-sources.md)
for source terms, freshness, attribution, caching, and retention details.

Alpha Vantage remains available as an official documented adapter. With no
personal key it uses Alpha Vantage's real `demo` access and is limited to IBM:

```dotenv
MARKET_TERMINAL_MARKET_DATA_PROVIDER="alpha-vantage"
ALPHA_VANTAGE_API_KEY="your-key-or-demo"
```

Finnhub is also available for real-time US quote snapshots with an API key.
Its stock-candle endpoint is premium, so this adapter does not fake provider
history: charts show only bounded, flat OHLC marks accumulated from quotes
during the current process and label them `DERIVED · SESSION ONLY`.

```dotenv
MARKET_TERMINAL_MARKET_DATA_PROVIDER="finnhub"
FINNHUB_API_KEY="your-key"
```

Charts start with OHLC candlesticks, SMA 20/100, Wilder RSI 14, and volume.
Candles aggregate complete OHLC buckets when history is wider than the terminal;
the right margin and price tag identify the latest history bar, not a fabricated
live tick. Press `K` to switch between candles and line display. Comparisons and
normalized performance automatically use lines because one candle scale cannot
truthfully represent multiple normalized instruments. Press `M` to show or hide
both moving averages, `E` to switch them between SMA and EMA, `I` to toggle RSI,
and `B` or `V` to toggle volume. `T`/`Shift+T` and `]`/`[` cycle periods; `Home`
returns the inspection cursor to the latest observation. The same options can
be requested from the command bar, for example `CHART AAPL 1Y STYLE CANDLES
EMA20 RSI14`.

To use Alpaca's official Market Data API instead, create Alpaca data keys and
select the provider explicitly:

```dotenv
MARKET_TERMINAL_MARKET_DATA_PROVIDER="alpaca"
APCA_API_KEY_ID="your-key-id"
APCA_API_SECRET_KEY="your-secret-key"
ALPACA_FEED="iex"
MARKET_TERMINAL_WATCHLIST="AAPL,MSFT,NVDA"
MARKET_TERMINAL_MARKETS_SYMBOLS="SPY,QQQ,IWM"
MARKET_TERMINAL_CHART_SYMBOL="AAPL"
```

`iex` is the safe default for Alpaca Basic accounts; use `sip` only with the
corresponding entitlement. Query-only providers refresh Markets and Monitor
every 60 seconds by default; set `MARKET_TERMINAL_QUOTE_REFRESH_SECS` between 5
and 3,600 seconds to change it. `R` refreshes immediately. Markets uses
`MARKET_TERMINAL_MARKETS_SYMBOLS` when set and otherwise follows the watchlist.
It does not fill rates, currencies, commodities, breadth, sector aggregation,
or calendars with equity proxies or gallery values.

Monitor's `DAY RANGE` is shown only when the selected provider reports the
current session's low and high. `SESSION` sparklines contain at most 64 distinct
observations received during this process; cached repeats are ignored, the
trace is not persisted, and no historical points are synthesized. At narrow
widths—including the Desk pane—the table removes whole secondary columns while
retaining legible symbol, price, movement, trace, and quality fields.

## Live security research

The interactive Security workspace combines the selected market provider's
quote and recent history with the SEC's official company-ticker master,
submissions, and company-facts APIs. Reported annual revenue, operating income,
net income, and diluted EPS are derived only from comparable US-GAAP 10-K
facts; recent 10-K, 10-Q, and 8-K metadata retain their accession numbers and
official document URLs. The `OWN` / `FORM4` view fetches up to six recent Form
4 or 4/A ownership documents, maps their non-derivative transactions without
invented scores, and identifies acquisition/disposition, shares, value, role,
ownership nature, and 10b5-1 status. The upper panel becomes a log-value Form 4
scatter with acquisition/disposition marks, selected-event emphasis, and
two-sided weekly value bars. Its rollup says `LOADED SAMPLE` because it covers
only those bounded filings, not an invented 12-month universe. Click a chart
mark/row or select with Up/Down or `j`/`k`, then press `O` or Enter to open the
official SEC filing index. All provider calls run on a coalescing background
worker.

SEC EDGAR does not supply analyst estimates or a canonical peer set, and this
adapter does not yet normalize institutional 13F ownership. Those panels say so
explicitly instead of showing generated values. Form 4 failures are best-effort
and visibly report parsed/requested filing counts. Press `F9` or click the
Security header to invalidate the page cache and fetch again.

## Live alert observations

Alert rules remain local and clearly marked as simulated delivery, but the
interactive app evaluates them against real snapshots from the selected market
provider.
Create a rule with `ALERT IBM > 250` or `ALERT IBM MOVE < -2`, then press `R`
or click the Alerts header to fetch a new observation. Evaluation IDs derive
from provider, canonical instrument, observation time, price, and move, so
refreshing the same delayed record is idempotent and cannot satisfy debounce by
accident. Provider entitlement and availability failures are shown directly.

## Live instrument master

The interactive Find workspace loads the SEC EDGAR company-ticker master on a
background worker and assigns canonical identities from zero-padded CIKs. It
never substitutes the deterministic demo catalog when the SEC source is
unavailable: the header reports loading, live-count, or explicit failure state.
Search by ticker or company name, press `F9` (or click the Find header) to
refresh, and press Enter on a result to open that security.

SEC fair-access guidance requires an identifiable user agent. Set your own
application/contact value when distributing or forking the terminal:

```dotenv
MARKET_TERMINAL_SEC_USER_AGENT="market-terminal/0.1.0 your-email@example.com"
```

## Importing a real portfolio

Export current positions as CSV from your brokerage, open the command bar with
`/`, and run:

```text
PORT IMPORT "~/Downloads/positions.csv"
```

Use `PORT RELOAD` after replacing the export. To load it automatically on every
launch, set an absolute path (or a `~/` path) in the ignored `.env` file:

```dotenv
MARKET_TERMINAL_PORTFOLIO_CSV="~/Downloads/positions.csv"
```

The importer recognizes common Fidelity-, Schwab-, and Vanguard-style header
aliases, including `Symbol`/`Ticker`, `Quantity`/`Qty`/`Shares`,
`Current Value`/`Market Value`/`Mkt Val`, price, total or per-share cost basis,
gain/loss percentage, description, and currency. It finds headers after broker
preambles, combines the same symbol across accounts, identifies cash and money
market rows, and rejects non-USD totals instead of silently adding unlike
currencies. Account identifiers and other unused columns are not retained.

Market value and gain/loss come from the export. YTD return and Sharpe remain
`N/A` because a positions snapshot does not contain enough transaction history
to calculate them honestly. No broker password or API credential is required,
and the CSV stays local.

Overview immediately reflects the same in-memory imported portfolio and live
news cache. A point-in-time positions export does not contain a return series,
so Overview leaves YTD return, drawdown, volatility, Sharpe, attribution, and
movers unavailable instead of manufacturing them. Click a position to open
Security or a headline to open its News topic; press `F9` or `R` to request a
news refresh.

## Spreadsheet CSV files

The persistent Spreadsheet starts empty; the IBM model is reserved for the
deterministic gallery. Enter values directly or replace the active sheet from a
UTF-8 CSV while preserving formulas as raw cell contents:

```text
SHEET IMPORT "~/Documents/model.csv"
SHEET EXPORT "~/Documents/model-export.csv"
```

Import is bounded to 26 columns, 100 rows, and 10 MB and is one undoable edit.
Export writes only the active sheet and refuses to replace an existing file.
Use `SHEET EXPORT! <FILE.CSV>` when replacement is intentional; replacement is
written through a same-directory temporary file. On Unix, newly created files
use owner-only permissions.

## Experience gallery

These captures are generated from the native Ratatui render buffer at a
consistent 160 × 48 terminal size. They are application output, not design
mockups.

| Research overview | Live-style market monitor |
| --- | --- |
| ![Research overview with performance, holdings, news, and market context](docs/screenshots/overview.png) | ![Cross-asset market monitor with configurable quote columns and data-quality states](docs/screenshots/monitor.png) |
| **Comparative charting** | **Spreadsheet workspace** |
| ![Normalized multi-instrument chart with moving average and volume](docs/screenshots/charting.png) | ![Keyboard-first spreadsheet with formulas and market-linked cells](docs/screenshots/spreadsheet.png) |
| **IRC market chat** | **Codex AI command plane** |
| ![Native IRC market chat with channel conversation and participant presence](docs/screenshots/chat.png) | ![OpenRouter assistant for analysis and validated workspace control](docs/screenshots/assistant.png) |
| **Alerts register** | **Security research** |
| ![Debounced local alert rules with lifecycle and audit state](docs/screenshots/alerts.png) | ![Single-security quote, chart, fundamentals, estimates, and news](docs/screenshots/security.png) |
| **Instrument discovery** | |
| ![Ranked canonical instrument search results](docs/screenshots/find.png) | |

## Run locally

Install Rust, then:

```bash
cargo run --release
```

Run unit tests:

```bash
cargo test
```

Run the opt-in contracts against the configured live providers serially. The
serial setting respects the public Alpha Vantage demo-key request limit:

```bash
cargo test --lib -- --ignored --nocapture --test-threads=1
```

Create a release binary:

```bash
cargo build --release
```

## AI command plane

The `AI`/`ASK` drawer runs inference on a background thread so slow provider
requests never block terminal input or rendering. Press `A` from any workspace,
type immediately, press Enter to send, and press Esc or click outside the drawer
to return to the panel underneath. The interactive binary loads an ignored
`.env` file automatically; exported variables take precedence:

```bash
codex login # choose Sign in with ChatGPT
cp .env.example .env
cargo run --release
```

The default provider keeps one Codex app-server process warm and uses its cached
ChatGPT login. It does not copy an OAuth token or require API credits. Each
request gets a fresh ephemeral, read-only Codex thread inside that process, with
structured output restricted to the terminal's validated UI actions.
`CODEX_MODEL` is optional; when omitted, Codex uses the model selected by its
local configuration.

OpenRouter remains available as an API-key fallback:

```dotenv
MARKET_TERMINAL_AI_PROVIDER="openrouter"
OPENROUTER_API_KEY="your-openrouter-key"
OPENROUTER_MODEL="openrouter/auto"
```

Because the adapter uses Chat Completions, a separately billed OpenAI Platform
key can also be used with the OpenAI endpoint and model name.

You can also issue a direct command such as
`AI bring portfolio forward and open it`. The
model can request only four validated UI operations: focus a registered
workspace, bring a registered workspace to the front, dispatch an existing
terminal command, or restore the default workspace order. It cannot execute
shell commands, read credentials, submit trades, or mutate arbitrary
application state.

## IRC chat

The `CHAT`/`IRC` workspace connects on a dedicated Tokio worker, keeping DNS,
TLS, registration, stream reads, and reconnect delays off the render loop.
Incoming and outgoing queues are bounded so a busy room cannot grow terminal
memory without limit. Configure a server before launching:

```bash
export IRC_SERVER="irc.libera.chat"
export IRC_PORT="6697"                 # optional; inferred from IRC_TLS
export IRC_TLS="true"                  # true by default
export IRC_NICKNAME="market-terminal"
export IRC_CHANNEL="#market-terminal"
cargo run --release
```

Optional `IRC_SERVER_PASSWORD` and `IRC_NICKSERV_PASSWORD` values are passed
only to the IRC adapter and are never rendered. Press `H` or type `CHAT`, then
press `I`/Enter to compose, Enter to send, Esc to leave input, or `R` to
reconnect.

## Design constraints

- crisp type and chart strokes; no simulated CRT glow
- keyboard-first navigation
- no proprietary Bloomberg code, assets, trademarks, or data
- native Rust architecture with no browser assets

## Acknowledgements and third-party code

HawaiianNinja pointed us to
[`makeev/alphai-tui`](https://github.com/makeev/alphai-tui). Codex then
integrated the selected MIT-licensed pieces we care about—indicator math,
chart and watchlist-density ideas, the responsive split desk,
provider-selection patterns, Yahoo/Finnhub/Alpaca adapter behavior, the fast
first-run/settings flow, named theme presets, semantic keymap parsing, and the
Form 4 insider workflow—into this project's bounded, provider-aware
architecture. `alphai-tui` is Copyright (c) 2026 Mikhail Makeev and licensed
under MIT. The copied-code provenance and complete upstream license text are in
[`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md).

## Architecture

The codebase follows domain-driven, package-by-feature boundaries. Each
workspace owns its domain model, query port, local interaction state, and
terminal adapter. The application kernel knows only the `Workspace` contract
and registry; infrastructure implements feature-owned ports and is wired at
the composition root.

```text
src/
├── app/             lifecycle, input modes, workspace contract and registry
├── features/        bounded contexts packaged by feature
│   ├── overview/    domain + port + workspace
│   ├── markets/     domain + port + workspace
│   ├── security/    domain + port + workspace
│   ├── portfolio/   domain + port + workspace
│   ├── news/        domain + port + workspace
│   ├── instrument/  search port + discovery workspace
│   ├── market_data/ typed quote/history read contracts
│   ├── persistence/ versioned session + opaque feature-document ports
│   ├── watchlist/   monitor model + catalog port + workspace
│   ├── charting/    chart specification + history port + workspace
│   ├── chat/        IRC domain + gateway port + workspace
│   ├── spreadsheet/ workbook domain + application + presentation
│   └── assistant/   AI conversation domain + provider port + workspace
├── foundation/      narrowly shared value objects such as InstrumentId
├── infrastructure/ adapters implementing feature-owned ports
├── ui/              theme, terminal chrome, and reusable visual primitives
└── bootstrap.rs     dependency injection and feature registration
```

See [`docs/architecture.md`](docs/architecture.md) for dependency rules and
the recipe for adding a new terminal function.

## License

MIT © 2026 DJ Petersen.
