interface WatchQuote {
  symbol: string;
  name: string;
  price: number;
  currency: string;
  changePercent: number;
  points: number[];
  asOf: string;
}
export interface WatchlistPort {
  quotes(
    symbols: string[],
    signal: AbortSignal,
  ): Promise<{ quotes: WatchQuote[]; unavailable: string[] }>;
}
export interface WatchlistStorage {
  read(): string[];
  write(symbols: string[]): void;
}
