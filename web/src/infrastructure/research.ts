import type { ResearchPort } from "../features/research/contracts";
async function get<T>(path: string, signal?: AbortSignal): Promise<T> {
  const response = await fetch(`/api/${path}`, { signal });
  const data = await response.json();
  if (!response.ok)
    throw new Error(
      data &&
        typeof data === "object" &&
        "error" in data &&
        typeof data.error === "string"
        ? data.error
        : "Unable to load research",
    );
  return data as T;
}
export const publicResearch: ResearchPort = {
  quotes: (symbols, signal) =>
    get(`quotes?symbols=${encodeURIComponent(symbols.join(","))}`, signal),
  history: (symbol, range, signal) =>
    get(`history?symbol=${encodeURIComponent(symbol)}&range=${range}`, signal),
  search: (query, signal) =>
    get(`search?q=${encodeURIComponent(query)}`, signal),
  news: (symbol, signal) =>
    get(`news?symbol=${encodeURIComponent(symbol)}`, signal),
  company: (symbol, signal) =>
    get(`company?symbol=${encodeURIComponent(symbol)}`, signal),
};
