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
