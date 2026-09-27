import assert from "node:assert/strict";
import test from "node:test";
import { projectInitializationDiagnostics } from "./agent-runtime-diagnostics.mjs";
test("projects only bounded fixed initialization metadata", () => {
  const row = {
    tsMs: 1,
    product: "agent",
    component: "runtime",
    method: "initialize_engine",
    code: "component_storage_unavailable",
    outcome: "failed",
    private: "secret",
  };
  const input = [
    JSON.stringify(row),
    "{broken",
    JSON.stringify({ ...row, code: "private-token" }),
    JSON.stringify({ ...row, product: "workspace" }),
  ].join("\n");
  assert.deepEqual(projectInitializationDiagnostics(input), [
    { tsMs: 1, method: "initialize_engine", code: "component_storage_unavailable" },
  ]);
  assert.equal(projectInitializationDiagnostics(Array(50).fill(JSON.stringify(row)).join("\n")).length, 32);
});
