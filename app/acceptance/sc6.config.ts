import { defineConfig } from "@playwright/test";

// Final-acceptance SC6 harness only; e2e's stack-starting global setup is deliberately absent.
export default defineConfig({
  testDir: ".",
  testMatch: "sc6.spec.ts",
  workers: 1,
  retries: 0,
  use: { locale: "ko-KR", viewport: { width: 1280, height: 800 } },
});
