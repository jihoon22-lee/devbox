import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "e2e",
  fullyParallel: false,
  workers: 1,
  retries: 0,
  timeout: 45_000,
  globalSetup: "./e2e/global-setup.ts",
  use: {
    baseURL: "http://localhost:1460",
    locale: "ko-KR",
    viewport: { width: 1280, height: 800 },
    trace: "retain-on-failure",
  },
});
