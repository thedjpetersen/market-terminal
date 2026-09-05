import {
  getQuote,
  getHistory,
  searchInstruments,
  getNews,
  ranges,
  validSymbol,
} from "./infrastructure/yahoo";
import { getCompany } from "./infrastructure/sec";
import { ProviderError } from "./infrastructure/upstream";

function json(data: unknown, status = 200): Response {
  return Response.json(data, {
    status,
    headers: {
      "X-Content-Type-Options": "nosniff",
      "Cache-Control": status === 200 ? "public, max-age=60" : "no-store",
      "Referrer-Policy": "strict-origin-when-cross-origin",
    },
  });
}

export default {
  async fetch(
    request: Request,
    env: Env,
    ctx: ExecutionContext,
  ): Promise<Response> {
    const url = new URL(request.url);
    if (!url.pathname.startsWith("/api/")) return env.ASSETS.fetch(request);
    if (request.method !== "GET")
      return json({ error: "Method not allowed" }, 405);
    if (url.pathname === "/api/health")
      return json({
        status: "ok",
        service: "market-terminal",
        schema: 1,
        analytics: "browser-wasm",
      });
    if (url.search.length > 512)
      return json({ error: "Query is too long" }, 400);
    const symbol = (url.searchParams.get("symbol") || "AAPL").toUpperCase();
    const range = url.searchParams.get("range") || "1y";
    if (!validSymbol(symbol))
      return json({ error: "Invalid ticker symbol" }, 400);
    const parameters: Record<string, string[]> = {
      "/api/quotes": ["symbols"],
      "/api/history": ["symbol", "range"],
      "/api/search": ["q"],
      "/api/news": ["symbol"],
      "/api/company": ["symbol"],
    };
    const allowed = parameters[url.pathname];
    if (!allowed) return json({ error: "Unknown API route" }, 404);
    if (
      [...url.searchParams.keys()].some(
        (parameter) => !allowed.includes(parameter),
      )
    )
      return json({ error: "Unsupported query parameter" }, 400);
    const cacheUrl = new URL(url);
    cacheUrl.searchParams.sort();
    cacheUrl.searchParams.set("_data_schema", "2");
    const key = new Request(cacheUrl.toString());
    const cache = (caches as CacheStorage & { default: Cache }).default;
    const cached = await cache.match(key);
    if (cached) return cached;
    try {
      let data: unknown;
      if (url.pathname === "/api/quotes") {
        const symbols = [
          ...new Set(
            (url.searchParams.get("symbols") || "SPY,QQQ,DIA,IWM")
              .split(",")
              .map((s) => s.toUpperCase()),
          ),
        ];
        if (symbols.length > 8 || !symbols.every(validSymbol))
          return json({ error: "Provide one to eight valid symbols" }, 400);
        const results = await Promise.allSettled(symbols.map(getQuote));
        data = {
          quotes: results.flatMap((result) =>
            result.status === "fulfilled" ? [result.value] : [],
          ),
          unavailable: symbols.filter(
            (_, i) => results[i].status === "rejected",
          ),
        };
        if (results.every((result) => result.status === "rejected"))
          return json(
            {
              ...(data as object),
              error:
                "Market data is temporarily unavailable. Retry in a moment.",
            },
            502,
          );
      } else if (url.pathname === "/api/history") {
        if (!ranges.some((item) => item === range))
          return json({ error: "Unsupported history range" }, 400);
        data = await getHistory(symbol, range);
      } else if (url.pathname === "/api/search") {
        const query = (url.searchParams.get("q") || "").trim();
        if (!query || query.length > 60)
          return json({ error: "Enter a ticker or company name" }, 400);
        data = await searchInstruments(query);
      } else if (url.pathname === "/api/news") data = await getNews(symbol);
      else if (url.pathname === "/api/company") data = await getCompany(symbol);
      else return json({ error: "Unknown API route" }, 404);
      const response = json(data);
      ctx.waitUntil(cache.put(key, response.clone()));
      return response;
    } catch (error) {
      console.error(
        JSON.stringify({
          event: "provider_unavailable",
          route: url.pathname,
          symbol,
          message:
            error instanceof Error ? error.message : "Unknown provider failure",
        }),
      );
      return json(
        {
          error:
            error instanceof ProviderError
              ? error.message
              : "This source is currently unavailable. Please try again.",
        },
        502,
      );
    }
  },
} satisfies ExportedHandler<Env>;
