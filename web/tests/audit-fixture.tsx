import React from "react";
import { createRoot } from "react-dom/client";
import { useWatchlist } from "../src/features/watchlist/state";
import { useLibrary } from "../src/features/library/state";
import {
  deviceWatchlist,
  deviceLibrary,
} from "../src/infrastructure/deviceStorage";
import { Search } from "../src/features/research/Search";
import type { ResearchPort } from "../src/features/research/contracts";
const notify = (message: string) => {
  document.body.dataset.notice = message;
};
const requests = new Map<string, (items: unknown[]) => void>();
Object.assign(window, {
  stores: { deviceWatchlist, deviceLibrary },
  resolveSearch: (query: string) =>
    requests.get(query)?.([
      { symbol: query, name: query, exchange: "TEST", kind: "equity" },
    ]),
});
const port = {
  search: (query: string) =>
    new Promise((resolve) => requests.set(query, resolve)),
} as unknown as ResearchPort;
function Fixture() {
  const watch = useWatchlist(deviceWatchlist, notify);
  const library = useLibrary(deviceLibrary, notify);
  return (
    <>
      <output id="watch">{watch.symbols.join(",")}</output>
      <output id="library">
        {library.items.map((item) => item.title).join(",")}
      </output>
      {["AAPL", "MSFT"].map((symbol) => (
        <React.Fragment key={symbol}>
          <button onClick={() => watch.toggle(symbol)}>Watch {symbol}</button>
          <button onClick={() => library.save(symbol, "synthetic")}>
            Save {symbol}
          </button>
        </React.Fragment>
      ))}
      {library.items.map((item) => (
        <button key={item.id} onClick={() => library.remove(item.id)}>
          Delete {item.title}
        </button>
      ))}
      <Search
        port={port}
        onClose={() => {}}
        onSelect={(symbol) => {
          document.body.dataset.selected = symbol;
        }}
      />
    </>
  );
}
createRoot(document.getElementById("root")!).render(<Fixture />);
