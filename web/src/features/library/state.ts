import { useState } from "react";
import type { SavedItem, LibraryStorage } from "./contracts";
export function useLibrary(
  storage: LibraryStorage,
  notify: (message: string) => void,
) {
  const [items, setItems] = useState(() => storage.read().slice(0, 30));
  function persist(next: SavedItem[]): boolean {
    try {
      storage.write(next);
      setItems(next);
      return true;
    } catch {
      notify(
        "Browser storage is full or unavailable. Download your evidence to keep it.",
      );
      return false;
    }
  }
  function save(title: string, content: string) {
    if (items.length >= 30) {
      notify("Your library is full. Download and remove a snapshot first.");
      return;
    }
    if (
      persist([
        {
          id: crypto.randomUUID(),
          title,
          content,
          createdAt: new Date().toISOString(),
        },
        ...items,
      ])
    )
      notify("Snapshot saved on this device.");
  }
  return {
    items,
    save,
    remove: (id: string) => persist(items.filter((item) => item.id !== id)),
  };
}
