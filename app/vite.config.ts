/// <reference types="vitest/config" />
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 1450, strictPort: true, fs: { allow: [".."] }, watch: { ignored: ["**/src-tauri/**"] } },
  test: { environment: "jsdom", globals: true, setupFiles: ["src/test-setup.ts"], include: ["src/**/*.test.{ts,tsx}"] },
});
