import { defineConfig } from "@playwright/test";
export default defineConfig({
  testDir: "./tests",
  testMatch: "audit.browser.ts",
  workers: 1,
  use: { baseURL: "http://127.0.0.1:5174" },
  webServer: {
    command: "npx vite --host 127.0.0.1 --port 5174",
    url: "http://127.0.0.1:5174/tests/audit-fixture.html",
    reuseExistingServer: false,
  },
});
