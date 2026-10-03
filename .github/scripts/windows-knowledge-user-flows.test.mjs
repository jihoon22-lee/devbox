import assert from "node:assert/strict";
import test from "node:test";
import * as document from "./windows-knowledge-document-recovery.mjs";
import * as search from "./windows-knowledge-search-lifecycle.mjs";
import * as activity from "./windows-knowledge-activity.mjs";
for (const module of [document, search, activity])
  test(`${module.scenarioIds.join(",")} cannot fabricate Windows PASS without an owned fixture`, async () => {
    const results = await module.run({
      sourceSha: "a".repeat(40),
      fixtureSha: "b".repeat(40),
      artifactDigests: { knowledge: "c".repeat(64) },
    });
    assert.deepEqual(
      results.map((r) => r.id),
      module.scenarioIds,
    );
    assert.ok(results.every((r) => r.status === "NOT_RUN" && r.failureCode));
  });
