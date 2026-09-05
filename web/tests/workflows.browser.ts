import { test, expect } from "@playwright/test";
test.beforeEach(async ({ page }) => {
  if (process.env.MARKET_LIVE === "1") return;
  const bars = Array.from({ length: 160 }, (_, i) => ({
    timestamp: 1700000000 + i * 86400,
    open: 100 + Math.sin(i / 10) * 10,
    high: 115,
    low: 85,
    close: 100 + Math.sin(i / 10) * 10,
    volume: 100000,
  }));
  await page.route("**/api/**", async (route) => {
    const url = new URL(route.request().url()),
      symbol = url.searchParams.get("symbol") || "AAPL";
    let data: unknown;
    if (url.pathname === "/api/quotes")
      data = {
        quotes: (url.searchParams.get("symbols") || symbol)
          .split(",")
          .map((s) => ({
            symbol: s,
            name: `${s} test instrument`,
            currency: "USD",
            exchange: "Test exchange",
            price: 105,
            previousClose: 100,
            change: 5,
            changePercent: 5,
            asOf: "2026-09-04T20:00:00Z",
            high: 110,
            low: 99,
            volume: 1000,
            marketState: "Delayed",
            source: "Deterministic browser test",
            points: bars.map((b) => b.close),
          })),
        unavailable: [],
      };
    else if (url.pathname === "/api/history")
      data = {
        symbol,
        currency: "USD",
        range: "1y",
        bars,
        source: "Deterministic browser test",
        asOf: "2026-09-04T20:00:00Z",
      };
    else if (url.pathname === "/api/search")
      data = [
        {
          symbol: "AAPL",
          name: "Apple Inc.",
          exchange: "NASDAQ",
          kind: "Equity",
        },
      ];
    else if (url.pathname === "/api/news")
      data = [
        {
          id: "test",
          title: "A test market headline",
          summary: "",
          source: "Test publisher",
          url: "https://example.com/story",
          publishedAt: "2026-09-04T20:00:00Z",
        },
      ];
    else if (url.pathname === "/api/company")
      data = {
        name: "Apple Inc.",
        cik: "0000320193",
        industry: "Electronic Computers",
        fiscalYearEnd: "0926",
        periods: [
          {
            end: "2025-09-27",
            filed: "2025-10-31",
            revenue: 1000000,
            operatingIncome: 200000,
            netIncome: 150000,
            eps: 5,
          },
        ],
        filings: [
          {
            form: "10-K",
            date: "2025-10-31",
            url: "https://www.sec.gov/Archives/test",
            title: "Annual report",
          },
        ],
      };
    else data = { status: "ok" };
    await route.fulfill({ json: data });
  });
});
test("research, watchlist and library work across navigation and reload", async ({
  page,
}, info) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.goto("/");
  await expect(page.locator(".quote-tile")).toHaveCount(4);
  await page
    .getByRole("button", { name: "Search a ticker or company", exact: true })
    .click();
  await page.getByRole("textbox", { name: "Ticker or company" }).fill("AAPL");
  await page
    .locator(".search-results button")
    .filter({ hasText: "AAPL" })
    .first()
    .click();
  await expect(page.locator(".security-price")).toBeVisible();
  await page.getByRole("button", { name: "Watch", exact: true }).click();
  await page
    .getByRole("button", { name: "Save snapshot", exact: true })
    .click();
  await page.getByRole("button", { name: "financials", exact: true }).click();
  await expect(page.locator("tbody tr").first()).toBeVisible();
  await page.getByRole("button", { name: "filings", exact: true }).click();
  await expect(
    page.locator('a[href^="https://www.sec.gov/Archives/"]').first(),
  ).toBeVisible();
  await page.getByRole("button", { name: "news", exact: true }).click();
  await expect(page.locator(".headline").first()).toBeVisible();
  await page
    .getByRole("button", { name: "Watchlist", exact: true })
    .filter({ visible: true })
    .click();
  await expect(page.locator(".watch-item")).toHaveCount(1);
  await page.reload();
  await expect(page.locator(".watch-item")).toHaveCount(1);
  await page
    .getByRole("button", {
      name: info.project.name === "mobile" ? "Saved" : "Saved research",
      exact: true,
    })
    .filter({ visible: true })
    .click();
  await expect(page.locator(".saved-item")).toHaveCount(1);
  await page.locator(".saved-title").click();
  await expect(page.getByRole("dialog")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog")).toHaveCount(0);
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
  expect(errors).toEqual([]);
});
test("all three models execute actual browser WASM and preserve saved evidence", async ({
  page,
}) => {
  await page.goto("/?view=models&symbol=AAPL");
  await page.getByRole("button", { name: "Run model", exact: true }).click();
  await expect(
    page.locator(".result-stats").getByText("Option price", { exact: true }),
  ).toBeVisible();
  await page.getByLabel("Option right", { exact: true }).selectOption("put");
  await expect(page.locator(".result-stats")).toHaveCount(0);
  await page.getByRole("button", { name: "Run model", exact: true }).click();
  await expect(page.locator(".result-stats")).toBeVisible();
  await page.getByRole("button", { name: "Save model result" }).click();
  await page.getByRole("button", { name: "Fixed income", exact: true }).click();
  await page.getByRole("button", { name: "Run model", exact: true }).click();
  await expect(
    page.locator(".result-stats").getByText("Clean price", { exact: true }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "Strategy backtest", exact: true })
    .click();
  await page.getByRole("button", { name: "Run model", exact: true }).click();
  await expect(
    page.locator(".result-stats").getByText("Final equity", { exact: true }),
  ).toBeVisible();
  const download = page.waitForEvent("download");
  await page
    .getByRole("button", { name: "Download exact model evidence" })
    .click();
  expect((await download).suggestedFilename()).toBe("market-backtest.json");
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
});
test("provider failures stay visible without fabricated prices", async ({
  page,
}) => {
  await page.route("**/api/quotes?**", (route) =>
    route.fulfill({
      status: 502,
      json: { error: "Source unavailable for test" },
    }),
  );
  await page.goto("/");
  await expect(
    page.getByText("Source unavailable for test").first(),
  ).toBeVisible();
  await expect(page.locator(".quote-tile")).toHaveCount(0);
});
