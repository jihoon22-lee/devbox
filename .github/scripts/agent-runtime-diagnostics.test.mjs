import assert from "node:assert/strict";
import test from "node:test";
import { projectInitializationDiagnostics, projectConnectionDiagnostics } from "./agent-runtime-diagnostics.mjs";
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

test("connection diagnostics expose only closed stage codes and product names", () => {
  const row = {
    tsMs: 2,
    product: "workspace",
    component: "agent-connection",
    method: "connect",
    outcome: "failed",
    code: "pipe_busy",
    message: "private secret",
  };
  assert.deepEqual(
    projectConnectionDiagnostics(
      [row, { ...row, code: "private-secret" }, { ...row, product: "private" }]
        .map((row) => JSON.stringify(row))
        .join("\n"),
    ),
    [{ tsMs: 2, product: "workspace", code: "pipe_busy", outcome: "failed" }],
  );
  assert.equal(projectConnectionDiagnostics(Array(150).fill(JSON.stringify(row)).join("\n")).length, 96);
});

test("handoff diagnostics retain only fixed stage and issue codes", async () => {
  const { projectHandoffDiagnostics } = await import("./agent-runtime-diagnostics.mjs");
  const row = {
    tsMs: 3,
    product: "workspace",
    component: "suite-handoff",
    method: "receive_webhook_log",
    outcome: "failed",
    code: "suite_activation_pending",
    body: "private",
  };
  assert.deepEqual(
    projectHandoffDiagnostics(
      [row, { ...row, code: "private" }, { ...row, method: "private" }].map(JSON.stringify).join("\n"),
    ),
    [{ tsMs: 3, product: "workspace", method: "receive_webhook_log", code: "suite_activation_pending" }],
  );
  assert.equal(projectHandoffDiagnostics(Array(50).fill(JSON.stringify(row)).join("\n")).length, 32);
});
