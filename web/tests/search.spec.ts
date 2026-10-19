import { test, expect } from "@playwright/test";

test("only current successful search results navigate, by keyboard and pointer", async ({
  page,
}, info) => {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  let failPending = () => {};
  const queries: string[] = [];
  await page.route("**/api/**", async (route) => {
    const url = new URL(route.request().url());
    if (url.pathname !== "/api/search") {
      await route.fulfill({
        status: 502,
        json: { error: "Provider unavailable in isolated search test" },
      });
      return;
    }
    const query = url.searchParams.get("q")!;
    queries.push(query);
    if (query === "MSFT") {
      await new Promise<void>((resolve) => {
        failPending = resolve;
      });
      await route.fulfill({
        status: 502,
        json: { error: "Search provider unavailable" },
      });
      return;
    }
    await route.fulfill({
      json: [
        {
          symbol: query,
          name: query === "AAPL" ? "Apple Inc." : "Meta Platforms, Inc.",
          exchange: "NASDAQ",
          kind: "Equity",
        },
      ],
    });
  });
  await page.clock.install({ time: new Date("2026-10-18T12:00:00Z") });
  await page.goto("/?view=watchlist");
  await page.clock.pauseAt(new Date("2026-10-18T13:00:00Z"));
  const openSearch = page.getByRole("button", {
    name: "Search a ticker or company",
    exact: true,
  });
  const dialog = page.getByRole("dialog", { name: "Search instruments" });
  const input = page.getByRole("textbox", { name: "Ticker or company" });
  const rows = page.locator(".search-results button");
  await openSearch.click();
  await expect(input).toBeFocused();
  await page.clock.runFor(200);
  await expect(rows).toHaveCount(1);
  await page.screenshot({ path: info.outputPath("search-ready.png") });
  await input.fill("MSFT");
  await input.press("Enter");
  await expect(dialog).toBeVisible();
  await expect(page).toHaveURL(/view=watchlist/);
  await expect(rows).toHaveCount(0);
  await page.clock.runFor(200);
  await expect.poll(() => queries).toEqual(["AAPL", "MSFT"]);
  await input.press("Enter");
  await expect(dialog.getByRole("status")).toContainText(
    "Searching instruments",
  );
  await page.screenshot({ path: info.outputPath("search-pending.png") });
  failPending();
  await expect(dialog.getByRole("status")).toHaveText(
    "Search provider unavailable",
  );
  await input.press("Enter");
  await expect(page).toHaveURL(/view=watchlist/);
  await expect(dialog).toBeVisible();
  await page.screenshot({ path: info.outputPath("search-error.png") });
  await input.fill("META");
  await page.clock.runFor(200);
  await expect(rows).toHaveText([/META/]);
  await input.press("Enter");
  await expect(dialog).toHaveCount(0);
  await expect(page).toHaveURL(/view=research&symbol=META/);
  await openSearch.click();
  await page.clock.runFor(200);
  await expect(rows).toHaveText([/AAPL/]);
  await rows.click();
  await expect(dialog).toHaveCount(0);
  await expect(page).toHaveURL(/view=research&symbol=AAPL/);
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
  expect(errors).toEqual([]);
});
