# Market Terminal

A native Rust, open-source market workstation inspired by the information
density and keyboard ergonomics of professional financial terminals.

The application runs directly in your terminal. Ratatui draws every panel,
table, chart, border, and color; Crossterm handles input and terminal state.
There is no HTML, CSS, JavaScript, WebAssembly, or browser runtime.

## Workspaces

- Overview — returns, risk, holdings, watchlist, and movers
- Markets — global indices, cross-asset monitor, sectors, breadth, and rates
- Security — quote, price chart, fundamentals, analyst consensus, and news
- Portfolio — positions, allocation, attribution, scenarios, and activity
- News — headline browser, story reader, and live movers
- Spreadsheet — formulas, ranges, dependency evaluation, and market refresh
- AI — OpenRouter-backed analysis and natural-language workspace control
- Find — canonical instrument identity and ranked symbol/company discovery
- Monitor — configurable watchlists, sorting, data-quality states, and replay
- Chart — price/volume history, comparisons, normalization, and moving averages
- Alerts — idempotent, debounced local rules with acknowledgement and audit state

Use the labeled navigation keys, or type a function such as `MON`, `CHART`,
`SHEET`, `FIND`, or `ASK` into the command bar. All displayed market values are
deterministic demo data.

## Experience gallery

These captures are generated from the native Ratatui render buffer at a
consistent 160 × 48 terminal size. They are application output, not design
mockups.

| Research overview | Live-style market monitor |
| --- | --- |
| ![Research overview with performance, holdings, news, and market context](docs/screenshots/overview.png) | ![Cross-asset market monitor with configurable quote columns and data-quality states](docs/screenshots/monitor.png) |
| **Comparative charting** | **Spreadsheet workspace** |
| ![Normalized multi-instrument chart with moving average and volume](docs/screenshots/charting.png) | ![Keyboard-first spreadsheet with formulas and market-linked cells](docs/screenshots/spreadsheet.png) |
| **Alerts register** | **OpenRouter AI command plane** |
| ![Debounced local alert rules with lifecycle and audit state](docs/screenshots/alerts.png) | ![OpenRouter assistant for analysis and validated workspace control](docs/screenshots/assistant.png) |
| **Instrument discovery** | **Security research** |
| ![Ranked canonical instrument search results](docs/screenshots/find.png) | ![Single-security quote, chart, fundamentals, estimates, and news](docs/screenshots/security.png) |

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

## OpenRouter AI

The `AI`/`ASK` workspace runs inference on a background thread so slow provider
requests never block terminal input or rendering. Configure it with environment
variables before launching:

```bash
export OPENROUTER_API_KEY="your-key"
export OPENROUTER_MODEL="openrouter/auto" # optional; this is the default
cargo run --release
```

Press `A`, type a question, and press Enter. You can also issue a direct command
such as `AI bring portfolio forward and open it`. The model can request only
four validated UI operations: focus a registered workspace, bring a registered
workspace to the front, dispatch an existing terminal command, or restore the
default workspace order. It cannot execute shell commands, read credentials,
submit trades, or mutate arbitrary application state.

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
│   ├── watchlist/   monitor model + catalog port + workspace
│   ├── charting/    chart specification + history port + workspace
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
