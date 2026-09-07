import { afterEach, expect, it } from "vitest";
import { applyBrowserPatch, captureBrowser, migrationFailure, type BrowserPatch } from "./protocol";
afterEach(() => localStorage.clear());
function patch(): BrowserPatch {
  const before = captureBrowser();
  return { id: "fixture", rollback: false, before, after: { ...before, "apip-collections-v2": "new collections fixture", "apip-history-v2": "new history fixture" }, summary: { added: 2, matched: 0, alreadyImported: 0, conflicts: 0, capacityExcluded: 0 } };
}
it("resumes a partially written browser activation and can restore its exact before image", () => {
  const value = patch(); localStorage.setItem("apip-collections-v2", value.after["apip-collections-v2"]!);
  expect(applyBrowserPatch(value)).toEqual(value.after);
  expect(applyBrowserPatch({ ...value, rollback: true, before: value.after, after: value.before })).toEqual(value.before);
});
it("rejects a third value before writing any key", () => {
  const value = patch(); localStorage.setItem("apip-history-v2", "a later product edit");
  expect(() => applyBrowserPatch(value)).toThrow("데이터가 변경");
  expect(localStorage.getItem("apip-collections-v2")).toBeNull();
  expect(localStorage.getItem("apip-history-v2")).toBe("a later product edit");
});
it("rejects unexpected storage keys and never displays arbitrary native error text", () => {
  const value = patch(); value.after.unexpected = "secret-looking fixture";
  expect(() => applyBrowserPatch(value)).toThrow();
  expect(migrationFailure({ issue: "private fixture text" }).code).toBe("unavailable");
  expect(migrationFailure({ issue: "source-open" }).message).toContain("API Playground");
});
