import { expect, test } from "vitest";
import { rankFiles } from "./quickOpen";

const key = (root: string, path: string) => ({ root, path });
test("quick open deduplicates by complete key and ranks basename and recent files", () => {
  const a = key("project:a", "src/read me.ts");
  const b = key("project:b", "src/read me.ts");
  const deep = key("project:a", "read me.ts/other.ts");
  const ranked = rankFiles("read me", [deep, b, a, a], [a], "project:a");
  expect(ranked).toEqual([a, deep]);
  expect(rankFiles("", [], [b, a, a], "project:a")).toEqual([a]);
  expect(rankFiles("가😀", [key("project:a", "가😀.ts")], [], "project:a")).toHaveLength(1);
});
