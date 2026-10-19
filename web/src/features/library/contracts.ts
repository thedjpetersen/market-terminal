export interface SavedItem {
  id: string;
  title: string;
  createdAt: string;
  content: string;
}
export interface LibraryStorage {
  read(): SavedItem[];
  mutate(change: (current: SavedItem[]) => SavedItem[]): Promise<SavedItem[]>;
  subscribe(listener: () => void): () => void;
}
