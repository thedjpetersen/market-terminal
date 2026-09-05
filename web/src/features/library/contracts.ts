export interface SavedItem {
  id: string;
  title: string;
  createdAt: string;
  content: string;
}
export interface LibraryStorage {
  read(): SavedItem[];
  write(items: SavedItem[]): void;
}
