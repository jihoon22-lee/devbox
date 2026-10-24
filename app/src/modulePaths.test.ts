// @vitest-environment node
import { readdirSync } from "node:fs";
import { join, relative } from "node:path";
import { fileURLToPath, URL } from "node:url";
import { expect, it } from "vitest";

it("source TypeScript module stems remain unique on case-insensitive filesystems", () => {
  const root = fileURLToPath(new URL(".", import.meta.url));
  const modules = new Map<string, string[]>();
  const visit = (directory: string) => {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const path = join(directory, entry.name);
      if (entry.isDirectory()) visit(path);
      // Ambient declarations do not create runtime modules or emitted JS files.
      else if (entry.isFile() && /\.tsx?$/.test(entry.name) && !entry.name.endsWith(".d.ts")) {
        const name = relative(root, path).replaceAll("\\", "/");
        const stem = name.replace(/\.tsx?$/, "").toLowerCase();
        modules.set(stem, [...(modules.get(stem) ?? []), name]);
      }
    }
  };
  visit(root);
  expect([...modules.values()].filter((paths) => paths.length > 1)).toEqual([]);
});
