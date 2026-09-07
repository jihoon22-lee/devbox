import { describe, expect, it } from "vitest";
import { TOOLS } from "./index";
import { mayExport, TOOL_COMMANDS } from "./outputPolicy";
describe("global transform commands and output policy", () => {
  it("matches the actual tool routes without giving HMAC an export capability", () => {
    expect(TOOL_COMMANDS.map(({ id, group, name }) => ({ id, group, name })))
      .toEqual(TOOLS.map(({ id, group, name }) => ({ id, group, name })));
    expect(new Set(TOOL_COMMANDS.map(tool => tool.commandId)).size).toBe(TOOLS.length);
    expect(mayExport({ kind: "tool", toolId: "hmac" })).toBe(false);
    expect(mayExport({ kind: "tool", toolId: "unknown" })).toBe(false);
    expect(mayExport(undefined)).toBe(false);
    expect(mayExport({ kind: "tool", toolId: "jwt" })).toBe(true);
  });
});
