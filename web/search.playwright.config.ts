import { defineConfig } from "@playwright/test";
import browserConfig from "./playwright.config";

// Search UI checks need only the web app and deterministic provider responses.
export default defineConfig({
  testDir: "./tests",
  testMatch: "search.spec.ts",
  timeout: 15_000,
  workers: 2,
  projects: browserConfig.projects,
  use: {
    baseURL: "http://127.0.0.1:41772",
    trace: "retain-on-failure",
  },
  webServer: {
    command: "vite --host 127.0.0.1 --port 41772 --strictPort",
    url: "http://127.0.0.1:41772",
    reuseExistingServer: false,
  },
});
