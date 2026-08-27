# Third-party notices

## alphai-tui

Selected chart indicator algorithms and interaction ideas were adapted from
[`makeev/alphai-tui`](https://github.com/makeev/alphai-tui), commit
`9143d2e1176d0a67a9f26960427cf370187fc2e6`. The directly adapted indicator
implementation is identified in `src/features/charting/indicators.rs`. The
official Alpaca adapter in `src/infrastructure/alpaca.rs` also adapts the
upstream response-shape, price-fallback, feed-selection, and bar-ordering work
to this project's typed ports and bounded synchronous workers. The secret-free
first-run/effective-settings overlay is based on the upstream settings-flow
idea, with new state and rendering written for this application's shell. The
responsive Monitor/Chart/News split-desk composition also adapts the upstream
split-view behavior to this application's generic `Workspace` contract; its
implementation is identified in `src/app/desk.rs`. Responsive watchlist column
selection and Unicode sparkline downsampling adapt the upstream table behavior;
the implementation and its provider-observation boundary are identified in
`src/features/watchlist/workspace.rs`.

The half-block candlestick renderer, width-aware OHLC aggregation, reserved
right margin, last-bar price marker, and Braille moving-average overlay adapt
the upstream chart implementation; the integrated code is identified in
`src/features/charting/workspace.rs`.

The selectable Form 4 insider-activity workflow and publisher-filing open
interaction were inspired by the upstream Insider view. This project's typed
ownership model and bounded XML adapter were independently implemented against
official SEC submissions metadata and ownership XML; they do not copy
AlphaAI's proprietary scoring or enrichment contract. The relevant code is in
`src/features/security/` and `src/infrastructure/live_security.rs`.

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
