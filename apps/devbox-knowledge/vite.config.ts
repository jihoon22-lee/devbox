/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
export default defineConfig({
  plugins: [react()], clearScreen: false,
  build: {
    manifest: true,
  },
  server: { port: 1432, strictPort: true, watch: { ignored: ["**/src-tauri/**"] } },
  test: { environment: "jsdom" },
});
