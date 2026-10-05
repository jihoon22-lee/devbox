import assert from "node:assert/strict";
import test from "node:test";
import { run, scenarioIds } from "./windows-workspace-runtime-recovery-ui.mjs";

test("dependency-only diagnostics cannot execute unrelated runtime or LSP scenarios", async () => {
  let calls = 0;
  const result = await run({
    diagnosticOnly: true,
    dependenciesOnly: true,
    workspaceFixture: {
      dependencyRefresh: async () => {
        calls++;
        return { assertions: ["approved", "failed", "preserved"], screenshots: ["owned.png"] };
      },
    },
  });
  assert.equal(calls, 1);
  assert.deepEqual(
    result.map(({ id, status }) => ({ id, status })),
    [{ id: "DEPS-01", status: "PASS" }],
  );
  await assert.rejects(run({ dependenciesOnly: true }), /retained diagnostic/);
});
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

test("LSP failure is captured before closing its panel and preserves cleanup failure separately", async () => {
  const events = [];
  const results = await run({
    ui: {
      screenshot: async () => {
        events.push("capture");
        return "/owned/lsp.png";
      },
    },
    workspaceFixture: {
      managedLspLifecycle: async () => {
        throw new Error("original LSP failure");
      },
      closeFailedLsp: async () => {
        events.push("close");
        throw new Error("close failure");
      },
      dependencyRefresh: async () => {
        events.push("deps");
        return { notRun: "unavailable" };
      },
    },
  });
  assert.deepEqual(events, ["capture", "close", "deps"]);
  assert.equal(results[2].error.message, "original LSP failure");
  assert.equal(results[2].cleanupError.message, "close failure");
});
