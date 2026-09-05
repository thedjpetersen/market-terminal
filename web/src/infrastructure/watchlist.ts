import type { WatchlistPort } from "../features/watchlist/contracts";
import { publicResearch } from "./research";
export const watchlistQuotes: WatchlistPort = {
  async quotes(symbols, signal) {
    const batches = [];
    for (let i = 0; i < symbols.length; i += 8) {
      const batch = symbols.slice(i, i + 8);
      batches.push(
        publicResearch
          .quotes(batch, signal)
          .catch(() => ({ quotes: [], unavailable: batch })),
      );
    }
    const data = await Promise.all(batches);
    return {
      quotes: data.flatMap((batch) => batch.quotes),
      unavailable: data.flatMap((batch) => batch.unavailable),
    };
  },
};
