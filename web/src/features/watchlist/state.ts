import { useState } from "react";
import type { WatchlistStorage } from "./contracts";
export function useWatchlist(
  storage: WatchlistStorage,
  notify: (message: string) => void,
) {
  const [symbols, setSymbols] = useState(() =>
    [...new Set(storage.read())]
      .filter(
        (symbol) =>
          /^[A-Z0-9^][A-Z0-9.^=-]{0,19}$/.test(symbol) &&
          !symbol.includes(".."),
      )
      .slice(0, 32),
  );
  function toggle(symbol: string) {
    if (
      !/^[A-Z0-9^][A-Z0-9.^=-]{0,19}$/.test(symbol) ||
      symbol.includes("..")
    ) {
      notify("This ticker is not valid.");
      return;
    }
    if (!symbols.includes(symbol) && symbols.length >= 32) {
      notify("Your watchlist is full. Remove an asset before adding another.");
      return;
    }
    const next = symbols.includes(symbol)
      ? symbols.filter((s) => s !== symbol)
      : [...symbols, symbol];
    try {
      storage.write(next);
      setSymbols(next);
    } catch {
      notify(
        "Browser storage is full or unavailable. Your watchlist could not be saved.",
      );
    }
  }
  return { symbols, toggle };
}
