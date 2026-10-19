import { useEffect, useState } from "react";
import type { WatchlistStorage } from "./contracts";
export function useWatchlist(
  storage: WatchlistStorage,
  notify: (message: string) => void,
) {
  const [symbols, setSymbols] = useState<string[]>(() => {
    try {
      return storage.read();
    } catch {
      return [];
    }
  });
  useEffect(() => {
    const refresh = () => {
      try {
        setSymbols(storage.read());
      } catch {
        notify("Watchlist storage cannot be read; existing data is unchanged.");
      }
    };
    const unsubscribe = storage.subscribe(refresh);
    refresh();
    return unsubscribe;
  }, [storage, notify]);
  async function toggle(symbol: string) {
    if (
      !/^[A-Z0-9^][A-Z0-9.^=-]{0,19}$/.test(symbol) ||
      symbol.includes("..")
    ) {
      notify("This ticker is not valid.");
      return;
    }
    const add = !symbols.includes(symbol);
    try {
      await storage.mutate((current) => {
        if (!add) return current.filter((s) => s !== symbol);
        if (current.includes(symbol)) return current;
        if (current.length >= 32)
          throw new Error(
            "Your watchlist is full. Remove an asset before adding another.",
          );
        return [...current, symbol];
      });
    } catch (error) {
      notify(`Watchlist could not be saved: ${String(error)}`);
    }
  }
  return { symbols, toggle };
}
