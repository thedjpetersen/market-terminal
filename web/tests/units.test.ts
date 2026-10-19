import { describe, it, expect, vi, afterEach } from "vitest";
import { formatQuotePrice } from "../src/foundation/quoteUnits";
import { cloudAnalytics } from "../src/infrastructure/analytics";
afterEach(() => vi.unstubAllGlobals());
describe("provider units", () => {
  it("retains subunits and unknown codes without changing values", () => {
    expect(formatQuotePrice(122.05, "GBp")).toBe("122.05p");
    expect(formatQuotePrice(122.05, "GBX")).toBe("122.05p");
    expect(formatQuotePrice(122.05, "GBP")).toContain("122.05");
    expect(formatQuotePrice(122.05, "XYZ")).toBe("122.05 XYZ");
    expect(formatQuotePrice(122.05, "ZAc")).toBe("122.05 ZAc");
  });
  it.each(["GBp", "GBP", "EUR", "XYZ", undefined])(
    "rejects unsupported history %s before backtesting",
    async (currency) => {
      vi.stubGlobal(
        "fetch",
        vi
          .fn()
          .mockResolvedValue(
            new Response(
              JSON.stringify({ currency, bars: [{ close: 122.05 }] }),
            ),
          ),
      );
      await expect(cloudAnalytics.bars("SAMPLE")).rejects.toThrow(
        "no FX conversion is available",
      );
    },
  );
  it("retains USD observations", async () => {
    const bars = [
      { timestamp: 1, open: 100, high: 102, low: 99, close: 101, volume: 10 },
    ];
    vi.stubGlobal(
      "fetch",
      vi
        .fn()
        .mockResolvedValue(
          new Response(JSON.stringify({ currency: "USD", bars })),
        ),
    );
    await expect(cloudAnalytics.bars("SAMPLE")).resolves.toEqual(bars);
  });
});
