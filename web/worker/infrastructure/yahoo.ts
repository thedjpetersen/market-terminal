import type {
  Bar,
  History,
  Instrument,
  Quote,
  Story,
} from "../../src/features/research/contracts";
import {
  array,
  number,
  record,
  string,
  upstream,
  ProviderError,
} from "./upstream";

export const ranges = ["1d", "1m", "3m", "6m", "1y", "5y"] as const;
export function validSymbol(symbol: string): boolean {
  return /^[A-Z0-9^][A-Z0-9.^=-]{0,19}$/.test(symbol) && !symbol.includes("..");
}
const source = "Yahoo Finance · delayed / unofficial";

async function chart(symbol: string, range: string) {
  const interval = range === "1d" ? "5m" : range === "5y" ? "1wk" : "1d";
  const providerRange = range.endsWith("m") ? `${range}o` : range;
  const payload = record(
    await upstream(
      `https://query1.finance.yahoo.com/v8/finance/chart/${encodeURIComponent(symbol)}?range=${providerRange}&interval=${interval}`,
      60,
      2 * 1024 * 1024,
    ),
  );
  const result = record(array(record(payload.chart).result)[0]);
  if (!Object.keys(result).length)
    throw new ProviderError(
      "No price history was returned for this instrument",
    );
  const prices = record(array(record(result.indicators).quote)[0]);
  const bars: Bar[] = [];
  array(result.timestamp).forEach((timestamp, index) => {
    const values = ["open", "high", "low", "close", "volume"].map((key) =>
      number(array(prices[key])[index]),
    );
    if (
      typeof timestamp === "number" &&
      values.slice(0, 4).every((v) => v !== null)
    ) {
      bars.push({
        timestamp,
        open: values[0]!,
        high: values[1]!,
        low: values[2]!,
        close: values[3]!,
        volume: values[4] ?? 0,
      });
    }
  });
  return { meta: record(result.meta), bars };
}

export async function getQuote(symbol: string): Promise<Quote> {
  const { meta, bars } = await chart(symbol, "1d");
  const price = number(meta.regularMarketPrice) ?? bars.at(-1)?.close;
  const previous =
    number(meta.chartPreviousClose) ?? number(meta.previousClose);
  if (price === undefined || !previous)
    throw new ProviderError("Quote is unavailable");
  const time = number(meta.regularMarketTime);
  return {
    symbol,
    name: string(meta.longName) || string(meta.shortName) || symbol,
    currency: string(meta.currency) || "USD",
    exchange: string(meta.fullExchangeName) || string(meta.exchangeName),
    price,
    previousClose: previous,
    change: price - previous,
    changePercent: (price / previous - 1) * 100,
    high: number(meta.regularMarketDayHigh),
    low: number(meta.regularMarketDayLow),
    volume: number(meta.regularMarketVolume),
    asOf: time ? new Date(time * 1000).toISOString() : "",
    marketState: "Delayed",
    source,
    points: bars.map((bar) => bar.close),
  };
}
export async function getHistory(
  symbol: string,
  range: string,
): Promise<History> {
  const { meta, bars } = await chart(symbol, range);
  return {
    symbol,
    currency: string(meta.currency) || "USD",
    range,
    bars,
    source,
    asOf: bars.length
      ? new Date(bars.at(-1)!.timestamp * 1000).toISOString()
      : "",
  };
}

const popular: Instrument[] = [
  ["AAPL", "Apple Inc."],
  ["MSFT", "Microsoft Corporation"],
  ["NVDA", "NVIDIA Corporation"],
  ["AMZN", "Amazon.com, Inc."],
  ["GOOGL", "Alphabet Inc."],
  ["META", "Meta Platforms, Inc."],
  ["TSLA", "Tesla, Inc."],
  ["AMD", "Advanced Micro Devices"],
  ["JPM", "JPMorgan Chase & Co."],
  ["BRK-B", "Berkshire Hathaway"],
  ["SPY", "SPDR S&P 500 ETF"],
  ["QQQ", "Invesco QQQ"],
  ["IWM", "iShares Russell 2000 ETF"],
  ["GLD", "SPDR Gold Shares"],
  ["TLT", "iShares 20+ Year Treasury Bond ETF"],
  ["BTC-USD", "Bitcoin / US Dollar"],
  ["ETH-USD", "Ethereum / US Dollar"],
].map(([symbol, name]) => ({ symbol, name, exchange: "", kind: "Instrument" }));

export async function searchInstruments(query: string): Promise<Instrument[]> {
  try {
    const payload = record(
      await upstream(
        `https://query1.finance.yahoo.com/v1/finance/search?q=${encodeURIComponent(query)}&quotesCount=10&newsCount=0`,
        300,
        512000,
      ),
    );
    const results = array(payload.quotes)
      .map(record)
      .map((item) => ({
        symbol: string(item.symbol),
        name: string(item.longname) || string(item.shortname),
        exchange: string(item.exchDisp),
        kind: string(item.typeDisp),
      }))
      .filter((item) => validSymbol(item.symbol));
    if (results.length) return results;
  } catch {
    /* Public reference symbols remain searchable during a provider outage. */
  }
  const matches = popular.filter((item) =>
    `${item.symbol} ${item.name}`.toLowerCase().includes(query.toLowerCase()),
  );
  if (!matches.length && validSymbol(query.toUpperCase()))
    return [
      {
        symbol: query.toUpperCase(),
        name: "Look up ticker",
        exchange: "",
        kind: "Ticker",
      },
    ];
  return matches;
}

export async function getNews(symbol: string): Promise<Story[]> {
  const payload = record(
    await upstream(
      `https://query1.finance.yahoo.com/v1/finance/search?q=${encodeURIComponent(symbol || "stock market")}&quotesCount=0&newsCount=12`,
      300,
      1024000,
    ),
  );
  return array(payload.news)
    .map(record)
    .flatMap((item) => {
      const url = string(item.link);
      const published = number(item.providerPublishTime);
      if (!url.startsWith("https://") || !published || !string(item.title))
        return [];
      return [
        {
          id: string(item.uuid) || url,
          title: string(item.title),
          summary: "",
          source: string(item.publisher),
          url,
          publishedAt: new Date(published * 1000).toISOString(),
        },
      ];
    })
    .slice(0, 12);
}
