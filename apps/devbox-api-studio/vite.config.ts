/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
export default defineConfig({
  plugins: [react()], clearScreen: false,
  build: {
    manifest: true,
    rollupOptions: { input: ["index.html", "migration-export.html"] },
  },
  server: { port: 1431, strictPort: true, watch: { ignored: ["**/src-tauri/**"] } },
  test: { environment: "jsdom" },
});
