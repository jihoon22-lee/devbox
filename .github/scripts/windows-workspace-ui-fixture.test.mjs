import assert from "node:assert/strict";
import test from "node:test";
import { createWorkspaceUiFixture } from "./windows-workspace-ui-fixture.mjs";

function performanceFixture({ readinessFailure = null } = {}) {
  const actions = [];
  let tasksReady = false;
  const job = { id: "owned-job", name: "owned performance task", enabled: false };
  const ui = {
    async click(target) {
      actions.push(["click", target]);
      if (target.name === "+ 새 작업" && !tasksReady)
        throw new Error("Expected one accessible button: + 새 작업; found 0");
    },
    async waitForTarget(target) {
      actions.push(["ready", target]);
      if (readinessFailure) throw readinessFailure;
      tasksReady = true;
    },
    async fill(target, value) {
      actions.push(["fill", target, value]);
    },
  };
  const cdp = {
    async evaluate(expression) {
      const value = expression.includes('"list_jobs"') ? [job] : [{ jobId: job.id, status: "succeeded", exitCode: 0 }];
      return { operation: { outcome: { state: "succeeded" } }, value };
    },
  };
  return { fixture: createWorkspaceUiFixture({ ui, cdp, fixtureRoot: "owned-fixture" }), actions };
}

test("performance task observes lazy tasks readiness before its single create click", async () => {
  const { fixture, actions } = performanceFixture();
  const result = await fixture.performanceTask();
  assert.equal(result.taskCompleted, true);
  assert.deepEqual(actions.slice(0, 3), [
    ["click", { role: "button", name: "작업 및 서비스" }],
    ["ready", { role: "button", name: "+ 새 작업" }],
    ["click", { role: "button", name: "+ 새 작업" }],
  ]);
  assert.equal(actions.filter(([action, target]) => action === "click" && target.name === "+ 새 작업").length, 1);
});

test("unavailable tasks readiness preserves failure without issuing create or save input", async () => {
  const failure = new Error("Timed out waiting for accessible button: + 새 작업");
  const { fixture, actions } = performanceFixture({ readinessFailure: failure });
  await assert.rejects(fixture.performanceTask(), (error) => error === failure);
  assert.deepEqual(
    actions.map(([action]) => action),
    ["click", "ready"],
  );
});
