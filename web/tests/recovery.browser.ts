import { expect, test } from "@playwright/test";
import { mkdir } from "node:fs/promises";

const bars = Array.from({ length: 160 }, (_, i) => ({
  timestamp: 1700000000 + i * 86400,
  open: 100 + Math.sin(i / 10) * 10,
  high: 115,
  low: 85,
  close: 100 + Math.sin(i / 10) * 10,
  volume: 100000,
}));
const history = {
  symbol: "AAPL",
  currency: "USD",
  range: "1m",
  bars,
  source: "Recovery test",
  asOf: "2026-10-18T12:00:00Z",
};
const story = {
  id: "recovered",
  title: "Recovered market headline",
  summary: "",
  source: "Test publisher",
  url: "https://example.com/story",
  publishedAt: "2026-10-18T12:00:00Z",
};
const company = {
  name: "Apple Inc.",
  cik: "0000320193",
  industry: "Electronic Computers",
  fiscalYearEnd: "0926",
  periods: [],
  filings: [],
};
const sessionMessage = "Your session has expired. Reload to sign in again.";

test.beforeEach(async ({ page }) => {
  test.skip(
    process.env.MARKET_LIVE === "1",
    "Recovery checks require deterministic failure responses",
  );
  if (!process.env.MARKET_WEB_URL) {
    const issuer = process.env.MARKET_TEST_ISSUER || "http://127.0.0.1:8900";
    const { token } = (await (await fetch(`${issuer}/token`)).json()) as {
      token: string;
    };
    await page.setExtraHTTPHeaders({ "Cf-Access-Jwt-Assertion": token });
  }
  await page.route("**/api/**", async (route) => {
    const url = new URL(route.request().url());
    if (url.pathname === "/api/quotes") {
      return route.fulfill({
        json: {
          quotes: (url.searchParams.get("symbols") || "AAPL")
            .split(",")
            .map((symbol) => ({
              symbol,
              name: `${symbol} test instrument`,
              currency: "USD",
              exchange: "Test exchange",
              price: 105,
              change: 5,
              changePercent: 5,
              previousClose: 100,
              high: 110,
              low: 99,
              volume: 1000,
              marketState: "Delayed",
              source: "Recovery test",
              asOf: history.asOf,
              points: bars.map((b) => b.close),
            })),
          unavailable: [],
        },
      });
    }
    if (url.pathname === "/api/history")
      return route.fulfill({ json: history });
    if (url.pathname === "/api/news") return route.fulfill({ json: [story] });
    if (url.pathname === "/api/company")
      return route.fulfill({ json: company });
    return route.continue();
  });
});

test("overview Refresh recovers headlines along with market data", async ({
  page,
}) => {
  let requests = 0;
  await page.route("**/api/news?**", (route) => {
    requests++;
    return requests === 1
      ? route.fulfill({
          status: 502,
          json: { error: "Headlines are temporarily unavailable." },
        })
      : route.fulfill({ json: [story] });
  });
  await page.goto("/");
  await expect(
    page.getByText("Headlines are temporarily unavailable."),
  ).toBeVisible();
  await page.getByRole("button", { name: "Refresh", exact: true }).click();
  await expect(page.getByRole("heading", { name: story.title })).toBeVisible();
  await expect(
    page.getByText("Headlines are temporarily unavailable."),
  ).toHaveCount(0);
  expect(requests).toBe(2);
});

for (const view of ["markets", "research"]) {
  test(`${view} headlines retry recovers a failed provider without navigation`, async ({
    page,
  }) => {
    let failing = true;
    await page.route("**/api/news?**", (route) =>
      failing
        ? route.fulfill({
            status: 502,
            contentType: "text/html",
            body: "<html>Gateway unavailable</html>",
          })
        : route.fulfill({ json: [story] }),
    );
    await page.goto(`/?view=${view}&symbol=AAPL`);
    if (view === "research")
      await page.getByRole("button", { name: "news", exact: true }).click();
    const notice = page
      .getByRole("status")
      .filter({ hasText: "Unable to load research" });
    await expect(notice).toBeVisible();
    await expect(page.locator(".headline")).toHaveCount(0);
    failing = false;
    await notice.getByRole("button", { name: "Retry" }).click();
    await expect(
      page.getByRole("heading", { name: story.title }),
    ).toBeVisible();
    await expect(notice).toHaveCount(0);
  });
}

for (const status of [200, 401, 403]) {
  test(`research HTML session response (${status}) explains reload and permits recovery`, async ({
    page,
  }, info) => {
    let expired = true;
    await page.route("**/api/history?**", (route) =>
      expired
        ? route.fulfill({
            status,
            contentType: "text/html",
            body: "<!doctype html><title>Sign in</title>",
          })
        : route.fulfill({ json: history }),
    );
    await page.goto("/?view=research&symbol=AAPL");
    const notice = page.getByRole("status").filter({ hasText: sessionMessage });
    await expect(notice).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Save snapshot" }),
    ).toBeDisabled();
    await expect(page.getByRole("img", { name: "Price history" })).toHaveCount(
      0,
    );
    if (status === 200) {
      await mkdir("../.factory/screenshots", { recursive: true });
      await page.screenshot({
        path: `../.factory/screenshots/session-${info.project.name}.png`,
        fullPage: true,
      });
    }
    expired = false;
    await notice.getByRole("button", { name: "Retry" }).click();
    await expect(
      page.getByRole("img", { name: "Price history" }),
    ).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Save snapshot" }),
    ).toBeEnabled();
    await expect(notice).toHaveCount(0);
  });
}

test("backtest history session failure recovers after reload and rerun", async ({
  page,
}) => {
  let expired = true;
  await page.route("**/api/history?**", (route) =>
    expired
      ? route.fulfill({
          contentType: "text/html",
          body: "<html>Sign in</html>",
        })
      : route.fulfill({ json: history }),
  );
  await page.goto("/?view=models&symbol=AAPL");
  await page
    .getByRole("button", { name: "Strategy backtest", exact: true })
    .click();
  await page.getByRole("button", { name: "Run model", exact: true }).click();
  await expect(page.getByText(sessionMessage)).toBeVisible();
  await expect(page.locator(".result-stats")).toHaveCount(0);
  expired = false;
  await page.reload();
  await page
    .getByRole("button", { name: "Strategy backtest", exact: true })
    .click();
  await page.getByRole("button", { name: "Run model", exact: true }).click();
  await expect(
    page.locator(".result-stats").getByText("Final equity", { exact: true }),
  ).toBeVisible();
  await expect(page.getByText(sessionMessage)).toHaveCount(0);
});

test("SEC retry distinguishes failure, empty filings and populated filings", async ({
  page,
}, info) => {
  let failing = true;
  let populated = false;
  await page.route("**/api/company?**", (route) =>
    failing
      ? route.fulfill({
          status: 502,
          json: { error: "SEC is temporarily unavailable." },
        })
      : route.fulfill({
          json: {
            ...company,
            filings: populated
              ? [
                  {
                    form: "10-K",
                    date: "2025-10-31",
                    url: "https://www.sec.gov/Archives/test",
                    title: "Annual report",
                  },
                ]
              : [],
          },
        }),
  );
  await page.goto("/?view=research&symbol=AAPL");
  await page.getByRole("button", { name: "filings", exact: true }).click();
  const empty = page.getByText(
    "No recent SEC filings were returned for this company.",
  );
  const failure = page
    .getByRole("status")
    .filter({ hasText: "SEC is temporarily unavailable." });
  await expect(failure).toBeVisible();
  await expect(empty).toHaveCount(0);
  failing = false;
  await failure.getByRole("button", { name: "Retry" }).click();
  await expect(empty).toBeVisible();
  await expect(failure).toHaveCount(0);
  await expect(
    page.locator('a[href^="https://www.sec.gov/Archives/"]'),
  ).toHaveCount(0);
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
  await mkdir("../.factory/screenshots", { recursive: true });
  await page.screenshot({
    path: `../.factory/screenshots/empty-filings-${info.project.name}.png`,
    fullPage: true,
  });
  populated = true;
  await page.getByRole("button", { name: "financials", exact: true }).click();
  await expect(
    page.getByText("No comparable annual financial facts were found."),
  ).toBeVisible();
  await page.getByRole("button", { name: "filings", exact: true }).click();
  await expect(page.getByRole("link", { name: /Annual report/ })).toBeVisible();
  await expect(empty).toHaveCount(0);
});

test("empty headlines remain an explicit successful state", async ({
  page,
}) => {
  await page.route("**/api/news?**", (route) => route.fulfill({ json: [] }));
  await page.goto("/");
  await expect(
    page.getByText("No headlines are available for this ticker."),
  ).toBeVisible();
  await expect(page.getByRole("button", { name: "Retry" })).toHaveCount(0);
});
