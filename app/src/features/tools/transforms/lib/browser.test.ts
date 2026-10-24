import { expect, it } from "vitest";
import { diff } from "./browser";

it("local line comparison preserves equal rows and insertion/deletion positions", async () => {
  expect(await diff("same\nremoved", "one\ntwo\nsame")).toEqual([
    { kind: 1, old_start: 0, old_end: 0, new_start: 0, new_end: 2 },
    { kind: 0, old_start: 0, old_end: 1, new_start: 2, new_end: 3 },
    { kind: 2, old_start: 1, old_end: 2, new_start: 3, new_end: 3 },
  ]);
});
it("local line comparison never silently drops the other half of a truncated diff", async () => {
  const a = Array.from({ length: 1500 }, (_, i) => `old-${i}`).join("\n");
  const b = Array.from({ length: 1500 }, (_, i) => `new-${i}`).join("\n");
  await expect(diff(a, b)).rejects.toThrow();
});
