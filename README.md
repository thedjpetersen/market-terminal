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

Use `G`, `M`, `S`, `P`, and `N` to navigate, or type a function into the command
bar. All displayed values are deterministic demo data.

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

## Design constraints

- crisp type and chart strokes; no simulated CRT glow
- keyboard-first navigation
- no proprietary Bloomberg code, assets, trademarks, or data
- native Rust architecture with no browser assets

## License

MIT © 2026 DJ Petersen.
