# Third-party notices

## alphai-tui

Selected chart indicator algorithms and interaction ideas were adapted from
[`makeev/alphai-tui`](https://github.com/makeev/alphai-tui), commit
`9143d2e1176d0a67a9f26960427cf370187fc2e6`. The directly adapted indicator
implementation is identified in
`crates/market-terminal-tui/src/features/charting/indicators.rs`. The official
Alpaca adapter in `crates/market-terminal-tui/src/infrastructure/alpaca.rs`
also adapts the upstream response-shape, price-fallback, feed-selection, and
bar-ordering work to this project's typed ports and bounded synchronous workers.
The secret-free
first-run/effective-settings overlay is based on the upstream settings-flow
idea, with new state and rendering written for this application's shell. The
responsive Monitor/Chart/News split-desk composition also adapts the upstream
split-view behavior to this application's generic `Workspace` contract; its
implementation is identified in `crates/market-terminal-tui/src/app/desk.rs`.
Responsive watchlist column
selection and Unicode sparkline downsampling adapt the upstream table behavior;
the implementation and its provider-observation boundary are identified in
`crates/market-terminal-tui/src/features/watchlist/workspace.rs`.

The Yahoo Finance chart adapter in
`crates/market-terminal-tui/src/infrastructure/yahoo.rs` adapts the
upstream chart response mapping, null-bar handling, and daily previous-close
fallback to this project's typed market-data, chart, and spreadsheet ports.
Yahoo Finance is a data source, not the licensor of this project or upstream
code; its data terms and redistribution boundary are documented in
`docs/data-sources.md`.

The Finnhub quote adapter in
`crates/market-terminal-tui/src/infrastructure/finnhub.rs` adapts the
upstream bounded, duplicate-aware session-history behavior. This project sends
the credential in Finnhub's documented header, exposes the provider's real
quote fields through typed ports, and labels the accumulated flat chart marks
as derived rather than provider candles.

The half-block candlestick renderer, width-aware OHLC aggregation, reserved
right margin, last-bar price marker, and Braille moving-average overlay adapt
the upstream chart implementation; the integrated code is identified in
`crates/market-terminal-tui/src/features/charting/workspace.rs`.

The selectable Form 4 insider-activity workflow and publisher-filing open
interaction were inspired by the upstream Insider view. This project's typed
ownership model and bounded XML adapter were independently implemented against
official SEC submissions metadata and ownership XML; they do not copy
AlphaAI's proprietary scoring or enrichment contract. The relevant code is in
`crates/market-terminal-tui/src/features/security/` and
`crates/market-terminal-tui/src/infrastructure/live_security.rs`.
The log-value scatter, collision nudge, selected-mark emphasis, and two-sided
weekly value bars in
`crates/market-terminal-tui/src/features/security/insider_chart.rs` adapt the
upstream Insider chart renderer to the bounded raw SEC transaction sample.

The named Catppuccin, Dracula, Gruvbox, and Nord palette mappings and their
dark-before-light cycle order in `crates/market-terminal-tui/src/ui/theme.rs`
adapt the upstream preset
table. Market Terminal maps those colors into its own semantic shell slots and
adds session persistence, command handling, and clickable settings controls.

The semantic action table, key grammar, normalization, reserved-key handling,
collision resolution, and effective-label rendering in
`crates/market-terminal-tui/src/app/keymap.rs`
adapt the upstream keymap design. Market Terminal uses an environment format
and a smaller shell/navigation action vocabulary while keeping its command-mode
Vim/Emacs editor and tmux prefix contract fixed.

The expanded story-card layout, wrapped-height scroll clamp, and full/detail
toggle in `crates/market-terminal-tui/src/features/news/workspace.rs` adapt the
upstream article-card
interaction. Market Terminal does not copy AlphaAI enrichment fields. Its
background publisher-page retrieval and readability extraction are independent
implementations built on the MIT-licensed `dom_smoothie` dependency. The
narrow-width list/detail collapse also adapts the upstream responsive News
behavior to this workspace's own feed and mouse-routing model.

MIT License

Copyright (c) 2026 Mikhail Makeev

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
