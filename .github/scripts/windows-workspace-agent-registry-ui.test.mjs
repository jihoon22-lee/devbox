import assert from "node:assert/strict";
import test from "node:test";
import { run } from "./windows-workspace-agent-registry-ui.mjs";
test("a missing isolated Agent fixture remains NOT_RUN", async () => {
  const records = await run({
    sourceSha: "a".repeat(40),
    fixtureSha: "b".repeat(40),
    artifactDigests: { workspace: "c".repeat(64) },
  });
  assert.deepEqual(
    records.map((result) => result.id),
    ["WORK-02", "WORK-03"],
  );
  assert.ok(records.every((result) => result.status === "NOT_RUN"));
});
