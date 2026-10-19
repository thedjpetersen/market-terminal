import { mkdir } from "node:fs/promises";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { test, expect } from "@playwright/test";

const screenshots = fileURLToPath(
  new URL("../../.factory/screenshots/", import.meta.url),
);

// Focus acceptance only: deterministic local evidence and search, no live data.
test.beforeEach(async ({ page }) => {
  if (!process.env.MARKET_WEB_URL) {
    const { token } = (await (
      await fetch("http://127.0.0.1:8900/token")
    ).json()) as { token: string };
    await page.setExtraHTTPHeaders({ "Cf-Access-Jwt-Assertion": token });
  }
  await page.addInitScript(() => {
    localStorage.setItem(
      "mt:library:v1",
      JSON.stringify(
        Array.from({ length: 30 }, (_, i) => ({
          id: `modal-${i}`,
          title: `Saved research ${i}`,
          createdAt: "2026-10-18T00:00:00Z",
          content: JSON.stringify(
            {
              symbol: "AAPL",
              evidence: Array.from({ length: 60 }, (_, row) => ({
                row,
                close: 105,
              })),
            },
            null,
            2,
          ),
        })),
      ),
    );
  });
  await page.route("**/api/search?**", (route) =>
    route.fulfill({
      json: [
        {
          symbol: "AAPL",
          name: "Apple Inc.",
          exchange: "NASDAQ",
          kind: "Equity",
        },
      ],
    }),
  );
});

test("saved evidence owns keyboard input and preserves page scrolling", async ({
  page,
}, info) => {
  await page.goto("/?view=saved");
  const trigger = page.locator(".saved-title").nth(15);
  await trigger.scrollIntoViewIfNeeded();
  const scroll = await page.evaluate(() => {
    document.body.style.overflow = "auto";
    return scrollY;
  });
  expect(scroll).toBeGreaterThan(0);
  await trigger.click();
  const dialog = page.getByRole("dialog", {
    name: "Saved research 15",
    exact: true,
  });
  const close = dialog.getByRole("button", { name: "Close evidence" });
  const download = dialog.getByRole("button", { name: "Download evidence" });
  await expect(close).toBeFocused();
  for (const modifier of ["Control", "Meta"]) {
    await page.keyboard.press(`${modifier}+k`);
    await expect(page.getByRole("dialog")).toHaveCount(1);
    await expect(close).toBeFocused();
  }
  await page.keyboard.press("Shift+Tab");
  await expect(download).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(close).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(dialog.getByLabel("Original JSON evidence")).toBeFocused();
  await page.mouse.move(2, 2);
  await page.mouse.wheel(0, 500);
  await page.evaluate(
    () =>
      new Promise<void>((done) =>
        requestAnimationFrame(() => requestAnimationFrame(() => done())),
      ),
  );
  expect(await page.evaluate(() => scrollY)).toBe(scroll);
  await mkdir(screenshots, { recursive: true });
  await page.screenshot({
    path: resolve(screenshots, `evidence-${info.project.name}.png`),
  });
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(trigger).toBeFocused();
  expect(await page.evaluate(() => document.body.style.overflow)).toBe("auto");
  expect(await page.evaluate(() => scrollY)).toBe(scroll);
  // Reopening and dismissing by backdrop also releases the scroll lock.
  await trigger.click();
  await page.mouse.click(2, 2);
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(trigger).toBeFocused();
  expect(await page.evaluate(() => scrollY)).toBe(scroll);
  await page.keyboard.press("Control+k");
  const search = page.getByRole("dialog", { name: "Search instruments" });
  const input = search.getByRole("textbox", { name: "Ticker or company" });
  await expect(input).toBeFocused();
  const result = search.getByRole("button", { name: /Apple Inc/ });
  await expect(result).toBeVisible();
  await page.keyboard.press("Shift+Tab");
  await expect(result).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(input).toBeFocused();
  await input.fill("AAPL");
  await page.keyboard.press("Control+k");
  await expect(input).toHaveValue("AAPL");
  await expect(page.getByRole("dialog")).toHaveCount(1);
  await page.screenshot({
    path: resolve(screenshots, `search-${info.project.name}.png`),
  });
  await search.getByRole("button", { name: "Close search" }).click();
  await expect(trigger).toBeFocused();
  expect(await page.evaluate(() => document.body.style.overflow)).toBe("auto");
  expect(await page.evaluate(() => scrollY)).toBe(scroll);
});

test("Tab wrapping reveals controls in an overflowing search dialog", async ({
  page,
}, info) => {
  // Twelve provider results overflow a short desktop window as well as mobile.
  if (info.project.name === "desktop")
    await page.setViewportSize({ width: 1440, height: 760 });
  await page.route("**/api/search?**", (route) =>
    route.fulfill({
      json: Array.from({ length: 12 }, (_, i) => ({
        symbol: `TEST${i}`,
        name: `Instrument ${i}`,
        exchange: "Test",
        kind: "Equity",
      })),
    }),
  );
  await page.goto("/?view=saved");
  await page.keyboard.press("Control+k");
  const dialog = page.getByRole("dialog", { name: "Search instruments" });
  const input = dialog.getByRole("textbox", { name: "Ticker or company" });
  const last = dialog.getByRole("button", { name: /Instrument 11/ });
  await expect(last).toBeVisible();
  expect(
    await dialog.evaluate(
      (element) => element.scrollHeight > element.clientHeight,
    ),
  ).toBe(true);
  await page.keyboard.press("Shift+Tab");
  await expect(last).toBeFocused();
  await expect(last).toBeInViewport({ ratio: 1 });
  await page.keyboard.press("Tab");
  await expect(input).toBeFocused();
  await expect(input).toBeInViewport({ ratio: 1 });
  await mkdir(screenshots, { recursive: true });
  await page.screenshot({
    path: resolve(screenshots, `long-search-${info.project.name}.png`),
  });
});

test("About keeps shortcut, Tab and Escape ownership", async ({
  page,
}, info) => {
  test.skip(
    info.project.name === "mobile",
    "About is currently in the desktop sidebar only",
  );
  await page.goto("/?view=saved");
  const trigger = page.getByRole("button", { name: "About this workspace" });
  await trigger.click();
  const button = page.getByRole("button", { name: "Back to workspace" });
  for (const modifier of ["Meta", "Control"]) {
    await page.keyboard.press(`${modifier}+k`);
    await expect(page.getByRole("dialog")).toHaveCount(1);
    await expect(button).toBeFocused();
  }
  await page.keyboard.press("Tab");
  await expect(button).toBeFocused();
  await page.keyboard.press("Shift+Tab");
  await expect(button).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(trigger).toBeFocused();
  await page.keyboard.press("Meta+k");
  await expect(
    page.getByRole("textbox", { name: "Ticker or company" }),
  ).toBeFocused();
});
