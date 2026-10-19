import { test, expect } from "@playwright/test";
test("simultaneous tabs preserve additions, deletions and mounted subscriptions", async ({
  context,
}) => {
  const a = await context.newPage();
  const b = await context.newPage();
  await Promise.all([
    a.goto("/tests/audit-fixture.html"),
    b.goto("/tests/audit-fixture.html"),
  ]);
  await Promise.all([
    a.getByText("Watch AAPL", { exact: true }).click(),
    b.getByText("Watch MSFT", { exact: true }).click(),
  ]);
  for (const page of [a, b]) {
    await expect(page.locator("#watch")).toContainText("AAPL");
    await expect(page.locator("#watch")).toContainText("MSFT");
  }
  await Promise.all([
    a.getByText("Save AAPL", { exact: true }).click(),
    b.getByText("Save MSFT", { exact: true }).click(),
  ]);
  for (const page of [a, b]) {
    await expect(page.locator("#library")).toContainText("AAPL");
    await expect(page.locator("#library")).toContainText("MSFT");
  }
  await Promise.all([
    a.getByText("Delete AAPL", { exact: true }).click(),
    b.getByText("Watch MSFT", { exact: true }).click(),
  ]);
  await a.reload();
  await b.reload();
  for (const page of [a, b]) {
    await expect(page.locator("#watch")).toHaveText("AAPL");
    await expect(page.locator("#library")).toHaveText("MSFT");
  }
});
test("actual storage locks serialize concurrent read-modify-write updates", async ({
  context,
}) => {
  const pages = await Promise.all([context.newPage(), context.newPage()]);
  await Promise.all(pages.map((p) => p.goto("/tests/audit-fixture.html")));
  await Promise.all(
    pages.map((p, index) =>
      p.evaluate(async (index) => {
        const storage = (window as any).stores.deviceLibrary;
        await Promise.all(
          Array.from({ length: 20 }, (_, n) =>
            storage.mutate((current: any[]) => [
              ...current,
              {
                id: `${index}-${n}`,
                title: "Synthetic",
                content: "test",
                createdAt: "2020-01-01",
              },
            ]),
          ),
        );
      }, index),
    ),
  );
  expect(
    await pages[0].evaluate(
      () => JSON.parse(localStorage.getItem("mt:library:v1")!).length,
    ),
  ).toBe(40);
  await pages[0].evaluate(() =>
    localStorage.setItem("mt:watchlist:v1", "corrupt"),
  );
  await pages[1].reload();
  await pages[1].getByText("Watch AAPL", { exact: true }).click();
  expect(
    await pages[0].evaluate(() => localStorage.getItem("mt:watchlist:v1")),
  ).toBe("corrupt");
});
test("Enter never selects results for an earlier query, including late responses", async ({
  page,
}) => {
  await page.goto("/tests/audit-fixture.html");
  await page.waitForTimeout(250);
  await page.evaluate(() => (window as any).resolveSearch("AAPL"));
  await expect(page.locator(".search-results")).toContainText("AAPL");
  const input = page.getByRole("textbox");
  await input.fill("MSFT");
  await input.press("Enter");
  expect(await page.locator("body").getAttribute("data-selected")).toBeNull();
  await page.waitForTimeout(250);
  await input.fill("IBM");
  await page.evaluate(() => (window as any).resolveSearch("MSFT"));
  await input.press("Enter");
  expect(await page.locator("body").getAttribute("data-selected")).toBeNull();
  await page.waitForTimeout(250);
  await page.evaluate(() => (window as any).resolveSearch("IBM"));
  await expect(page.locator(".search-results")).toContainText("IBM");
  await input.press("Enter");
  await expect(page.locator("body")).toHaveAttribute("data-selected", "IBM");
});
