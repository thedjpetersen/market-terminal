import type { WatchlistStorage } from "../features/watchlist/contracts";
import type { LibraryStorage, SavedItem } from "../features/library/contracts";
function store<T>(key: string, valid: (item: unknown) => item is T) {
  const read = (): T[] => {
    const value: unknown = JSON.parse(localStorage.getItem(key) ?? "[]");
    if (!Array.isArray(value) || !value.every(valid))
      throw new Error("Stored data is invalid; it has been left unchanged.");
    return value;
  };
  return {
    read,
    async mutate(change: (current: T[]) => T[]): Promise<T[]> {
      if (!navigator.locks)
        throw new Error(
          "Safe storage coordination is unavailable in this browser.",
        );
      return navigator.locks.request(`market-terminal:${key}`, () => {
        const next = change(read());
        if (!next.every(valid)) throw new Error("Invalid storage update");
        localStorage.setItem(key, JSON.stringify(next));
        window.dispatchEvent(new CustomEvent("mt:storage", { detail: key }));
        return next;
      });
    },
    subscribe(listener: () => void) {
      const changed = (event: StorageEvent) => {
        if (event.key === key || event.key === null) listener();
      };
      const local = (event: Event) => {
        if ((event as CustomEvent).detail === key) listener();
      };
      window.addEventListener("storage", changed);
      window.addEventListener("mt:storage", local);
      return () => {
        window.removeEventListener("storage", changed);
        window.removeEventListener("mt:storage", local);
      };
    },
  };
}
export const deviceWatchlist: WatchlistStorage = store(
  "mt:watchlist:v1",
  (item): item is string =>
    typeof item === "string" &&
    /^[A-Z0-9^][A-Z0-9.^=-]{0,19}$/.test(item) &&
    !item.includes(".."),
);
function isSavedItem(item: unknown): item is SavedItem {
  if (!item || typeof item !== "object") return false;
  const fields = item as Record<string, unknown>;
  return ["id", "title", "createdAt", "content"].every(
    (key) => typeof fields[key] === "string",
  );
}
export const deviceLibrary: LibraryStorage = store(
  "mt:library:v1",
  isSavedItem,
);
