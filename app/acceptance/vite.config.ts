import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";

export default defineConfig({
  build: {
    outDir: "dist/acceptance",
    emptyOutDir: true,
    lib: {
      entry: fileURLToPath(new URL("./terminal-probe.ts", import.meta.url)),
      name: "devboxSC10",
      formats: ["iife"],
      fileName: () => "terminal-probe.js",
    },
  },
});
