import assert from "node:assert/strict";
import test from "node:test";
import { mkdtemp, rm } from "node:fs/promises";
import path from "node:path";
import { tmpdir } from "node:os";
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
test("nested draft failure keeps its first error and screenshot despite cleanup failure", async (t) => {
  const temporaryRoot = await mkdtemp(path.join(tmpdir(), "devbox-draft-test-"));
  t.after(() => rm(temporaryRoot, { recursive: true, force: true }));
  const methods = [
    "prepare",
    "crashAndReopen",
    "context",
    "recovery",
    "failRecoveryWriter",
    "reopenAfterClose",
    "observeInput",
  ];
  const fixture = Object.fromEntries(methods.map((name) => [name, async () => {}]));
  fixture.prepare = async () => {
    throw new Error("first draft failure");
  };
  fixture.cleanup = async () => {
    throw new Error("later cleanup failure");
  };
  const results = await run({
    fixtureRoot: temporaryRoot,
    workspaceFixture: fixture,
    ui: { screenshot: async () => "/owned/first.png" },
  });
  assert.equal(results[0].status, "FAIL");
  assert.equal(results[0].error.message, "first draft failure");
  assert.deepEqual(results[0].screenshotPaths, ["/owned/first.png"]);
  assert.equal(results[0].cleanupError.message, "later cleanup failure");
});
