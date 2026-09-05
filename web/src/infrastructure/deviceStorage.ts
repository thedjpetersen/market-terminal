import type { WatchlistStorage } from "../features/watchlist/contracts";
import type { LibraryStorage, SavedItem } from "../features/library/contracts";
function read(key: string): unknown[] {
  try {
    const value: unknown = JSON.parse(localStorage.getItem(key) ?? "[]");
    return Array.isArray(value) ? value : [];
  } catch {
    return [];
  }
}
export const deviceWatchlist: WatchlistStorage = {
  read: () =>
    read("mt:watchlist:v1").filter(
      (item): item is string => typeof item === "string",
    ),
  write: (symbols) =>
    localStorage.setItem("mt:watchlist:v1", JSON.stringify(symbols)),
};
function isSavedItem(item: unknown): item is SavedItem {
  if (!item || typeof item !== "object") return false;
  const fields = item as Record<string, unknown>;
  return ["id", "title", "createdAt", "content"].every(
    (key) => typeof fields[key] === "string",
  );
}
export const deviceLibrary: LibraryStorage = {
  read: () => read("mt:library:v1").filter(isSavedItem),
  write: (items) =>
    localStorage.setItem("mt:library:v1", JSON.stringify(items)),
};
