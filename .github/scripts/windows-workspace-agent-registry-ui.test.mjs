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
test("nested Agent failure survives cleanup failure with first screenshot", async () => {
  const methods = [
    "prepareAgent",
    "configureAgent",
    "registry",
    "context",
    "wait",
    "trustSource",
    "attemptAgentReview",
    "crashAndReopen",
  ];
  const fixture = Object.fromEntries(methods.map((name) => [name, async () => {}]));
  fixture.prepareAgent = async () => {
    throw new Error("agent original failure");
  };
  fixture.cleanup = async () => {
    throw new Error("agent cleanup failure");
  };
  const results = await run({ workspaceFixture: fixture, ui: { screenshot: async () => "/owned/agent-first.png" } });
  assert.ok(results.every((result) => result.status === "FAIL"));
  assert.equal(results[0].error.message, "agent original failure");
  assert.equal(results[0].cleanupError.message, "agent cleanup failure");
  assert.deepEqual(results[0].screenshotPaths, ["/owned/agent-first.png"]);
});
