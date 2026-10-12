/// <reference types="vitest/config" />
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";
import { version } from "./package.json";

export default defineConfig({
  plugins: [react()],
  define: { __APP_VERSION__: JSON.stringify(version) },
  clearScreen: false,
  server: { port: 1450, strictPort: true, fs: { allow: [".."] }, watch: { ignored: ["**/src-tauri/**"] } },
  test: {
    maxWorkers: Number(process.env.VITEST_MAX_WORKERS ?? 2),
    environment: "jsdom",
    globals: true,
    setupFiles: ["src/test-setup.ts"],
    include: ["src/**/*.test.{ts,tsx}"],
  },
});
