export interface AnalyticsPort {
  execute(request: string): Promise<string>;
  bars(symbol: string): Promise<
    {
      timestamp: number;
      open: number;
      high: number;
      low: number;
      close: number;
      volume: number;
    }[]
  >;
}
