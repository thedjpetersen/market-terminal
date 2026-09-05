export interface Quote {
  symbol: string;
  name: string;
  currency: string;
  exchange: string;
  price: number;
  change: number;
  changePercent: number;
  asOf: string;
  previousClose: number;
  high: number | null;
  low: number | null;
  volume: number | null;
  marketState: string;
  source: string;
  points: number[];
}
export interface Bar {
  timestamp: number;
  open: number;
  high: number;
  low: number;
  close: number;
  volume: number;
}
export interface History {
  symbol: string;
  currency: string;
  range: string;
  bars: Bar[];
  source: string;
  asOf: string;
}
export interface Instrument {
  symbol: string;
  name: string;
  exchange: string;
  kind: string;
}
export interface Story {
  id: string;
  title: string;
  summary: string;
  source: string;
  url: string;
  publishedAt: string;
}
export interface CompanyResearch {
  name: string;
  cik: string;
  industry: string;
  fiscalYearEnd: string;
  periods: {
    end: string;
    filed: string;
    revenue: number;
    operatingIncome: number | null;
    netIncome: number | null;
    eps: number | null;
  }[];
  filings: { form: string; date: string; url: string; title: string }[];
}
export interface ResearchPort {
  quotes(
    symbols: string[],
    signal?: AbortSignal,
  ): Promise<{ quotes: Quote[]; unavailable: string[] }>;
  history(
    symbol: string,
    range: string,
    signal?: AbortSignal,
  ): Promise<History>;
  search(query: string, signal?: AbortSignal): Promise<Instrument[]>;
  news(symbol: string, signal?: AbortSignal): Promise<Story[]>;
  company(symbol: string, signal?: AbortSignal): Promise<CompanyResearch>;
}
