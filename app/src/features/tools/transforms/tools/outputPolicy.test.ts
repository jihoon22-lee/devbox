import { expect, it, vi } from "vitest";
import { CATALOG } from "../catalog";
import { recentTools, rememberTool } from "../recent";

it("records at most eight known IDs and never HMAC or JWT input/output", () => {
  localStorage.clear();
  const write = vi.spyOn(Storage.prototype, "setItem");
  for (const tool of CATALOG) rememberTool(tool.id);
  rememberTool("hmac");
  rememberTool("jwt");
  rememberTool("fixture-sensitive-input");
  expect(recentTools()).toHaveLength(8);
  expect(recentTools().slice(0, 2)).toEqual(["jwt", "hmac"]);
  for (const [key, text] of write.mock.calls) {
    expect(key).toBe("devbox.tools.recent");
    const ids = JSON.parse(text) as string[];
    expect(ids.every((id) => CATALOG.some((tool) => tool.id === id))).toBe(true);
  }
  expect(CATALOG.find((tool) => tool.id === "hmac")?.policy).toBe("non-persistable");
  expect(CATALOG.find((tool) => tool.id === "jwt")?.policy).toBe("sensitive");
  write.mockRestore();
});
