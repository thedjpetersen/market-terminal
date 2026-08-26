# Market Terminal

_I pasted this tweet into ChatGPT and just told it to keep going in between boogieboarding_

<img width="592" height="877" alt="image" src="https://github.com/user-attachments/assets/19912571-d2f0-4ef8-8230-bb868e14389a" />

A native Rust, open-source market workstation inspired by the information
density and keyboard ergonomics of professional financial terminals.

The application runs directly in your terminal. Ratatui draws every panel,
table, chart, border, and color; Crossterm handles input and terminal state.
There is no HTML, CSS, JavaScript, WebAssembly, or browser runtime.

## Workspaces

- Overview — returns, risk, holdings, watchlist, and movers
- Markets — global indices, cross-asset monitor, sectors, breadth, and rates
- Security — quote/chart, financials, estimates, ownership, filings, peers,
  and linked news
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
  data-quality states, last-known-good fallback, and replay
- Chart — comparative performance, zero baselines, inspection cursor, market
  profile statistics, volume histograms, and moving averages
- Chat — TLS-capable IRC market rooms with bounded queues, background reconnect,
  participant presence, notices, actions, and an inline composer
- Alerts — idempotent, debounced local rules with acknowledgement and audit state

Use the labeled navigation keys, click a visible workspace tab, or type a
function such as `MON`, `CHART`, `SHEET`, `CHAT`, `FIND`, or `ASK` into the
command bar. Mouse input is enabled in the interactive terminal: click the
command box and `GO`, select table rows and spreadsheet cells, activate chart
controls and research tabs, focus AI/chat composers, and scroll navigable
lists. News uses live source feeds and Portfolio uses your imported snapshot;
quote, analytics, and research panels that still use deterministic demo data
remain labeled as such.

Run `HELP` from the command bar—or press `F1`—to open the command and controls
guide without leaving the current workspace. Close it with `Esc`, `Q`, `F1`,
the on-screen close button, or by selecting a workspace tab.

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

The interactive Monitor, Chart, and Spreadsheet resolve quote/history fields
through Alpha Vantage on bounded background workers; provider latency never
runs on the input or render thread. Configure a personal key, comma-separated
watchlist, and initial chart symbol:

```dotenv
ALPHA_VANTAGE_API_KEY="your-key"
MARKET_TERMINAL_WATCHLIST="IBM,AAPL,MSFT"
MARKET_TERMINAL_CHART_SYMBOL="AAPL"
```

With no key, the app uses Alpha Vantage's real `demo` quote and full-history
IBM data, and limits the default monitor, chart, and sheet to IBM. Quote cells
and history are explicitly labeled delayed end-of-day data with provider and
quality. Unsupported symbols, one-day intraday history, or unentitled periods
show unavailable/permission-denied state; the app never fills them with
generated prices or replayed bars. Press `F9` or click the Chart header to
refresh. See [the data-source register](docs/data-sources.md) for freshness,
attribution, caching, and retention details.

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
