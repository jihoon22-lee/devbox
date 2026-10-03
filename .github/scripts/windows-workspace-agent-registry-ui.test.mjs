import assert from "node:assert/strict";
import test from "node:test";
import { run } from "./windows-workspace-agent-registry-ui.mjs";
test("a missing isolated Agent fixture remains NOT_RUN", async () => {
  const records = await run({
    sourceSha: "a".repeat(40),
    fixtureSha: "b".repeat(40),
    artifactDigests: { workspace: "c".repeat(64) },
  });
  assert.equal(records[0].id, "WORK-03");
  assert.equal(records[0].status, "NOT_RUN");
});
