import { expect, it } from "vitest";
import snapshot from "./fixtures/tools-v090.json";
import { TOOLS } from "./registry";

it("registry_matches_v090_tool_list", () => {
  expect(TOOLS.map(({ id, group, name, policy }) => ({ id, group, name, policy }))).toEqual(
    snapshot.tools.map(({ id, group, name, policy }) => ({ id, group, name, policy })),
  );
  expect(TOOLS).toHaveLength(21);
  expect(new Set(TOOLS.map((t) => t.id)).size).toBe(21);
  expect(TOOLS.every((t) => typeof t.component === "function")).toBe(true);
});
