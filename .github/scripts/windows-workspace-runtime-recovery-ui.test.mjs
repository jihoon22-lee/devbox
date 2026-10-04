import assert from "node:assert/strict";
import test from "node:test";
import { run, scenarioIds } from "./windows-workspace-runtime-recovery-ui.mjs";
test("missing actual adapters leave every mandatory runtime scenario NOT_RUN", async () => {
  const results = await run({ sourceSha: "a".repeat(40), fixtureSha: "a".repeat(40), artifactDigests: {} });
  assert.deepEqual(
    results.map((result) => result.id),
    scenarioIds,
  );
  assert.ok(results.every((result) => result.status === "NOT_RUN" && result.failureCode));
});
test("an incomplete adapter proof cannot claim packaged UI PASS", async () => {
  const results = await run({ workspaceFixture: { runtimeLostReply: async () => ({ assertions: ["incomplete"] }) } });
  assert.equal(results[0].status, "FAIL");
});
test("nested runtime adapter failures retain bounded error and first screenshot", async () => {
  const results = await run({
    ui: { screenshot: async () => "/owned/runtime-first.png" },
    workspaceFixture: {
      runtimeLostReply: async () => {
        throw new Error("runtime original failure");
      },
    },
  });
  assert.equal(results[0].status, "FAIL");
  assert.equal(results[0].error.message, "runtime original failure");
  assert.deepEqual(results[0].screenshotPaths, ["/owned/runtime-first.png"]);
});
