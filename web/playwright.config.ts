import { defineConfig, devices } from "@playwright/test";
export default defineConfig({
  testDir: "./tests",
  testMatch: "**/*.browser.ts",
  timeout: 45_000,
  use: {
    baseURL: process.env.MARKET_WEB_URL || "http://127.0.0.1:8799",
    trace: "retain-on-failure",
  },
  projects: [
    {
      name: "desktop",
      use: {
        ...devices["Desktop Chrome"],
        viewport: { width: 1440, height: 1000 },
      },
    },
    {
      name: "mobile",
      use: { ...devices["Pixel 7"], viewport: { width: 390, height: 844 } },
    },
  ],
  webServer: process.env.MARKET_WEB_URL
    ? undefined
    : {
        command: "npm run preview",
        url: "http://127.0.0.1:8799",
        reuseExistingServer: !process.env.CI,
      },
});
