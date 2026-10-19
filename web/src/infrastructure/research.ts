import type { ResearchPort } from "../features/research/contracts";
import { getApiJson } from "./apiResponse";
function get<T>(path: string, signal?: AbortSignal): Promise<T> {
  return getApiJson(
    `/api/${path}`,
    "Unable to load research. Try again.",
    signal,
  );
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
