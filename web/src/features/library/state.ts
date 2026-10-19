import { useEffect, useState } from "react";
import type { SavedItem, LibraryStorage } from "./contracts";
export function useLibrary(
  storage: LibraryStorage,
  notify: (message: string) => void,
) {
  const [items, setItems] = useState<SavedItem[]>(() => {
    try {
      return storage.read();
    } catch {
      return [];
    }
  });
  useEffect(() => {
    const refresh = () => {
      try {
        setItems(storage.read());
      } catch {
        notify("Library storage cannot be read; existing data is unchanged.");
      }
    };
    const unsubscribe = storage.subscribe(refresh);
    refresh();
    return unsubscribe;
  }, [storage, notify]);
  async function save(title: string, content: string) {
    const item = {
      id: crypto.randomUUID(),
      title,
      content,
      createdAt: new Date().toISOString(),
    };
    try {
      await storage.mutate((current) => {
        if (current.length >= 30)
          throw new Error(
            "Your library is full. Download and remove a snapshot first.",
          );
        return [item, ...current];
      });
      notify("Snapshot saved on this device.");
    } catch (error) {
      notify(`Snapshot could not be saved: ${String(error)}`);
    }
  }
  async function remove(id: string) {
    try {
      await storage.mutate((current) =>
        current.filter((item) => item.id !== id),
      );
    } catch (error) {
      notify(`Snapshot could not be removed: ${String(error)}`);
    }
  }
  return { items, save, remove };
}
