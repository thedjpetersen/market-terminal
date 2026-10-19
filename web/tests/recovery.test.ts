import { afterEach, describe, expect, test, vi } from "vitest";
import { publicResearch } from "../src/infrastructure/research";
import { cloudAnalytics } from "../src/infrastructure/analytics";

afterEach(() => vi.unstubAllGlobals());

const sessionMessage = "Your session has expired. Reload to sign in again.";
const clients = [
  ["quotes", () => publicResearch.quotes(["AAPL"])],
  ["history", () => publicResearch.history("AAPL", "1m")],
  ["search", () => publicResearch.search("Apple")],
  ["news", () => publicResearch.news("AAPL")],
  ["company", () => publicResearch.company("AAPL")],
  ["backtest history", () => cloudAnalytics.bars("AAPL")],
  ["engine", () => cloudAnalytics.execute("{}")],
] as const;

describe.each(clients)("%s recovery", (_name, request) => {
  test.each([200, 401, 403])(
    "classifies an HTML session response (%i) before decoding JSON",
    async (status) => {
      vi.stubGlobal(
        "fetch",
        vi.fn().mockResolvedValue(
          new Response("<!doctype html><title>Sign in</title>", {
            status,
            headers: { "Content-Type": "text/html; charset=utf-8" },
          }),
        ),
      );
      await expect(request()).rejects.toThrow(sessionMessage);
    },
  );

  test.each([401, 403])(
    "uses session recovery for a JSON auth failure (%i)",
    async (status) => {
      vi.stubGlobal(
        "fetch",
        vi
          .fn()
          .mockResolvedValue(Response.json({ error: "Denied" }, { status })),
      );
      await expect(request()).rejects.toThrow(sessionMessage);
    },
  );

  test("preserves a useful provider error", async () => {
    vi.stubGlobal(
      "fetch",
      vi
        .fn()
        .mockResolvedValue(
          Response.json(
            { error: "Provider temporarily unavailable" },
            { status: 502 },
          ),
        ),
    );
    await expect(request()).rejects.toThrow("Provider temporarily unavailable");
  });

  test.each([
    ["HTML gateway failure", "<html>Bad gateway</html>", "text/html"],
    ["malformed JSON failure", "{broken", "application/json"],
    ["invalid error shape", '{"error":{"code":42}}', "application/json"],
  ])(
    "uses actionable fallback copy for %s",
    async (_label, body, contentType) => {
      vi.stubGlobal(
        "fetch",
        vi.fn().mockResolvedValue(
          new Response(body, {
            status: 502,
            headers: { "Content-Type": contentType },
          }),
        ),
      );
      await expect(request()).rejects.toThrow(
        /(Unable to load research|Historical prices are unavailable|The model could not complete)/,
      );
    },
  );
});

test("malformed successful research JSON has a useful error", async () => {
  vi.stubGlobal(
    "fetch",
    vi.fn().mockResolvedValue(
      new Response("{broken", {
        headers: { "Content-Type": "application/json" },
      }),
    ),
  );
  await expect(publicResearch.news("AAPL")).rejects.toThrow(
    "Unable to load research",
  );
});

test.each([null, {}, { bars: null }, { bars: "invalid" }])(
  "backtest history rejects missing or invalid bars: %j",
  async (data) => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(Response.json(data)));
    await expect(cloudAnalytics.bars("AAPL")).rejects.toThrow(
      "Historical prices are unavailable",
    );
  },
);

test("successful research and history payloads retain their data", async () => {
  const bars = [
    {
      timestamp: 1700000000,
      open: 100,
      high: 110,
      low: 90,
      close: 105,
      volume: 50,
    },
  ];
  const history = {
    symbol: "AAPL",
    bars,
    range: "1y",
    currency: "USD",
    source: "Test",
    asOf: "2026-10-18",
  };
  vi.stubGlobal(
    "fetch",
    vi.fn().mockImplementation(() => Promise.resolve(Response.json(history))),
  );
  await expect(publicResearch.history("AAPL", "1y")).resolves.toEqual(history);
  await expect(cloudAnalytics.bars("AAPL")).resolves.toEqual(bars);
});

test("engine evidence retains exact integer tokens", async () => {
  const evidence = '{"value":9998990001009999}';
  vi.stubGlobal(
    "fetch",
    vi.fn().mockResolvedValue(
      new Response(evidence, {
        headers: { "Content-Type": "application/json; charset=utf-8" },
      }),
    ),
  );
  await expect(cloudAnalytics.execute("{}")).resolves.toBe(evidence);
});

test.each([
  ["research", () => publicResearch.news("AAPL")],
  ["backtest history", () => cloudAnalytics.bars("AAPL")],
] as const)(
  "%s network failures explain how to recover",
  async (_name, request) => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockRejectedValue(new TypeError("Failed to fetch")),
    );
    await expect(request()).rejects.toThrow(
      "Check your connection or reload to refresh your session.",
    );
  },
);

test("research cancellation and symbol encoding survive the transport boundary", async () => {
  const controller = new AbortController();
  const aborted = new DOMException("Cancelled", "AbortError");
  const fetcher = vi.fn().mockImplementation(() => {
    controller.abort();
    return Promise.reject(aborted);
  });
  vi.stubGlobal("fetch", fetcher);
  await expect(
    publicResearch.history("^GSPC", "1m", controller.signal),
  ).rejects.toBe(aborted);
  expect(fetcher).toHaveBeenCalledWith(
    "/api/history?symbol=%5EGSPC&range=1m",
    expect.objectContaining({ signal: controller.signal }),
  );
});
