import assert from "node:assert/strict";
import test from "node:test";
import { run, scenarioIds } from "./windows-workspace-draft-ui.mjs";
test("missing owned native fixture cannot fabricate packaged UI PASS", async () => {
  const result = await run({
    sourceSha: "a".repeat(40),
    fixtureSha: "b".repeat(40),
    artifactDigests: { workspace: "c".repeat(64) },
  });
  assert.deepEqual(
    result.map((item) => item.id),
    scenarioIds,
  );
  assert.ok(result.every((item) => item.status === "NOT_RUN" && item.failureCode));
});
