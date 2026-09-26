import assert from "node:assert/strict";
import { test } from "node:test";
import { runInNewContext } from "node:vm";
import { typedComponentBridge } from "./typed-component-fixture.mjs";
test("renderer bridge uses typed commands and retains negative-test fields", async () => {
  const seen = [];
  const invoke = async (command, payload) => {
    seen.push(JSON.parse(JSON.stringify({ command, payload })));
    if (payload.request.method === "send_request") throw new Error("native method rejected");
    return true;
  };
  const run = (component, method, extra = {}) =>
    runInNewContext(`${typedComponentBridge} invokeComponent("api-studio", payload)`, {
      invoke,
      payload: {
        request: { header: { requestId: "same-id", installationId: "foreign" }, component, method, args: {}, ...extra },
      },
    });
  await run("knowledge.search-settings", "index_now", { unexpected: true });
  assert.equal(seen[0].command, "plugin:knowledge|search_settings");
  assert.equal(seen[0].payload.request.unexpected, true);
  assert.equal(seen[0].payload.request.header.installationId, "foreign");
  assert.equal(seen[0].payload.request.header.requestId, "same-id");
  assert.equal("component" in seen[0].payload.request, false);
  await assert.rejects(run("api-studio.webhooks", "send_request"), /native method rejected/);
  assert.equal(seen[1].command, "plugin:api-studio|webhooks");
});

test("Workspace bridge maps all main components without sending an owner field", async () => {
  for (const component of [
    "runtime",
    "processes",
    "process-actions",
    "logs",
    "terminal",
    "problems",
    "commands",
    "files",
    "lsp",
    "source",
    "registry",
    "setup",
    "definitions",
    "dependencies",
  ]) {
    let seen;
    await runInNewContext(`${typedComponentBridge} invokeComponent("workspace", payload)`, {
      invoke: async (command, payload) => {
        seen = { command, payload };
      },
      payload: {
        request: { header: { requestId: "fixture" }, component: `workspace.${component}`, method: "fixture", args: {} },
      },
    });
    assert.equal(seen.command, `plugin:workspace|${component.replaceAll("-", "_")}`);
    assert.equal("component" in seen.payload.request, false);
  }
});
