import assert from "node:assert/strict";
import test from "node:test";
import { mkdtemp, rm } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { createWorkspaceUiFixture } from "./windows-workspace-ui-fixture.mjs";

function performanceFixture({
  readinessFailure = null,
  runReadinessFailure = null,
  fixtureRoot = "owned-fixture",
} = {}) {
  const actions = [];
  let tasksReady = false,
    jobCardReady = false;
  const job = { id: "owned-job", name: "owned performance task", enabled: false };
  const ui = {
    async click(target) {
      actions.push(["click", target]);
      if (target.name === "+ 새 작업" && !tasksReady)
        throw new Error("Expected one accessible button: + 새 작업; found 0");
      if (target.name === "지금 실행" && !jobCardReady) throw new Error("Accessible scope must be unique");
    },
    async waitForTarget(target) {
      actions.push(["ready", target]);
      if (target.name === "+ 새 작업") {
        if (readinessFailure) throw readinessFailure;
        tasksReady = true;
      } else if (target.name === "지금 실행") {
        if (runReadinessFailure) throw runReadinessFailure;
        jobCardReady = true;
      }
    },
    async fill(target, value) {
      actions.push(["fill", target, value]);
    },
  };
  const commands = [];
  const cdp = {
    async command(method) {
      commands.push(method);
      throw new Error("Unexpected CDP action before saved card readiness");
    },
    async evaluate(expression) {
      if (expression.startsWith("window.__TAURI_INTERNALS__.invoke"))
        return { context: { worktreeId: "owned-tree", target: { kind: "windows" } } };
      if (expression.includes('"snapshot"'))
        return {
          operation: { outcome: { state: "succeeded" } },
          value: { worktrees: [{ id: "owned-tree", binding: { root: fixtureRoot } }] },
        };
      const value = expression.includes('"list_jobs"')
        ? [job, { ...job, name: "owned lost reply job" }]
        : [{ jobId: job.id, status: "succeeded", exitCode: 0 }];
      return { operation: { outcome: { state: "succeeded" } }, value };
    },
  };
  return { fixture: createWorkspaceUiFixture({ ui, cdp, fixtureRoot }), actions, commands };
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

test("native persistence is followed by readiness of the exact saved job card before one run click", async () => {
  const { fixture, actions } = performanceFixture();
  await fixture.performanceTask();
  const runTarget = { role: "button", name: "지금 실행", scope: { role: "article", name: "owned performance task" } };
  assert.deepEqual(
    actions.filter(([, target]) => target.name === "지금 실행"),
    [
      ["ready", runTarget],
      ["click", runTarget],
    ],
  );
});

test("saved job card readiness failure prevents run input and retains the original failure", async () => {
  const failure = new Error("Timed out waiting for accessible button: 지금 실행");
  const { fixture, actions } = performanceFixture({ runReadinessFailure: failure });
  await assert.rejects(fixture.performanceTask(), (error) => error === failure);
  assert.equal(actions.filter(([action, target]) => action === "click" && target.name === "지금 실행").length, 0);
});

test("lost-reply job uses the same saved-card readiness before arming debugger or run input", async () => {
  const fixtureRoot = await mkdtemp(path.join(os.tmpdir(), "devbox-runtime-readiness-"));
  try {
    const failure = new Error("Saved runtime job card still unavailable");
    const { fixture, actions, commands } = performanceFixture({ fixtureRoot, runReadinessFailure: failure });
    fixture.windowsRoot = fixtureRoot;
    await assert.rejects(fixture.runtimeLostReply(), (error) => error === failure);
    assert.deepEqual(commands, []);
    assert.equal(actions.filter(([action, target]) => action === "click" && target.name === "지금 실행").length, 0);
    assert.deepEqual(actions.at(-1), [
      "ready",
      { role: "button", name: "지금 실행", scope: { role: "article", name: "owned lost reply job" } },
    ]);
  } finally {
    await rm(fixtureRoot, { recursive: true, force: true });
  }
});
