import assert from "node:assert/strict";
import test from "node:test";
import { mkdtemp, rm } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { createWorkspaceUiFixture, loseRuntimeReply } from "./windows-workspace-ui-fixture.mjs";

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
test("WSL combobox readiness precedes one keyboard option selection", async () => {
  const events = [];
  let ready = false;
  const fixture = createWorkspaceUiFixture({
    cdp: {},
    fixtureRoot: "owned",
    ui: {
      waitForTarget: async (target) => {
        assert.equal(target.name, "WSL 배포판");
        ready = true;
        events.push("ready");
      },
      click: async () => {
        assert.equal(ready, true);
        events.push("click");
      },
      press: async (key) => {
        events.push(key);
      },
    },
  });
  await fixture.selectOption({ role: "combobox", name: "WSL 배포판" }, 1);
  assert.deepEqual(events, ["ready", "click", "Home", "ArrowDown", "Enter"]);
});

function pausedReplyFixture({ action = "지금 실행", readFailure = null, disableFailure = null } = {}) {
  const events = [];
  let listener, release;
  const click = (target, intent) => {
    events.push(["click", target.name, intent]);
    listener({ callFrames: [{ callFrameId: "owned-frame" }] });
    return new Promise((resolve) => {
      release = resolve;
    });
  };
  const options = {
    cdp: {
      command: async (method, params) => {
        events.push([method, params]);
        if (method === "Runtime.evaluate") return { result: { objectId: "owned-function" } };
        if (method === "Debugger.evaluateOnCallFrame") {
          if (readFailure) throw readFailure;
          assert.equal(params.callFrameId, "owned-frame");
          return { result: { value: [{ operationId: "original-owned-operation" }] } };
        }
        if (method === "Debugger.disable") {
          release?.();
          if (disableFailure) throw disableFailure;
        }
        return {};
      },
      onEvent: (method, callback) => {
        listener = callback;
        return () => events.push(["unsubscribe"]);
      },
      evaluate: () => {
        throw new Error("Runtime read would hang while paused");
      },
    },
    ui: { click, clickWithConfirmation: click },
    wait: async (predicate) => assert.equal(await predicate(), true),
    jobId: "owned-job",
    scope: { role: "article", name: "owned job" },
    action,
    pendingExpression: "owned synchronous localStorage read",
    beforeRestart: async () => events.push(["authority-evidence"]),
    restart: async (crash) => {
      assert.equal(crash, true);
      events.push(["restart"]);
      release();
    },
  };
  return { events, options };
}
test("lost reply reads the paused call frame and preserves original operation before crash", async () => {
  const { events, options } = pausedReplyFixture();
  assert.equal(await loseRuntimeReply(options), "original-owned-operation");
  assert.equal(events.filter(([event]) => event === "click").length, 1);
  assert.ok(
    events.findIndex(([event]) => event === "Debugger.evaluateOnCallFrame") <
      events.findIndex(([event]) => event === "restart"),
  );
  assert.deepEqual(
    events.slice(-3).map(([event]) => event),
    ["authority-evidence", "restart", "unsubscribe"],
  );
  assert.ok(!events.some(([event]) => event === "Debugger.disable"));
});
test("stop lost reply explicitly accepts exactly one inline confirmation", async () => {
  const { events, options } = pausedReplyFixture({ action: "중지" });
  await loseRuntimeReply(options);
  assert.deepEqual(
    events.filter(([event]) => event === "click"),
    [["click", "중지", true]],
  );
});
test("paused read failure releases debugger and input while preserving first error", async () => {
  const failure = new Error("original paused read failure");
  const { events, options } = pausedReplyFixture({
    readFailure: failure,
    disableFailure: new Error("cleanup failure"),
  });
  await assert.rejects(loseRuntimeReply(options), (error) => error === failure);
  assert.ok(!events.some(([event]) => event === "restart"));
  assert.deepEqual(
    events.slice(-2).map(([event]) => event),
    ["unsubscribe", "Debugger.disable"],
  );
});
test("Source navigation waits approval controls and settled readers before returning", async () => {
  const events = [],
    ready = new Set();
  const fixture = createWorkspaceUiFixture({
    cdp: {},
    ui: {
      click: async (target) => {
        if (target.name !== "소스") assert.ok(ready.has(target.name), `missing readiness: ${target.name}`);
        events.push(["click", target.name]);
      },
      waitForTarget: async (target) => {
        ready.add(target.name);
        events.push(["ready", target.name]);
      },
      text: async (target) => {
        events.push(["text", target.name]);
        return "";
      },
    },
  });
  await fixture.trustSource();
  assert.deepEqual(events, [
    ["click", "소스"],
    ["ready", "Git 실행 검토"],
    ["click", "Git 실행 검토"],
    ["ready", "검토한 Git 실행 승인"],
    ["click", "검토한 Git 실행 승인"],
    ["text", "커밋 메시지"],
    ["ready", "Git 승인 상태 확인"],
  ]);
});
test("runtime recovery selects its owned project before mounting Tasks after restart", async () => {
  const { resumeRuntimeUi } = await import("./windows-workspace-ui-fixture.mjs");
  const actions = [];
  let selected = false;
  await resumeRuntimeUi(
    {
      selectWindows: async () => {
        actions.push("select");
        selected = true;
      },
    },
    {
      click: async (target) => {
        assert.ok(selected);
        actions.push(target.name);
      },
      waitForTarget: async (target) => {
        assert.ok(selected);
        actions.push(target.name);
      },
    },
  );
  assert.deepEqual(actions, ["select", "작업 및 서비스", "+ 새 작업"]);
  await assert.rejects(
    resumeRuntimeUi(
      {
        selectWindows: async () => {
          throw new Error("owned root missing");
        },
      },
      {
        click: async () => {
          throw new Error("must not navigate before selection");
        },
      },
    ),
    /owned root missing/,
  );
});

test("lost-reply journey explicitly stops the first live owner before requesting another run", async () => {
  const { stopReconciledRuntimeRun } = await import("./windows-workspace-ui-fixture.mjs");
  const events = [],
    scope = { role: "article", name: "owned" };
  await stopReconciledRuntimeRun(
    {
      waitForTarget: async (target) => events.push(["ready", target]),
      clickWithConfirmation: async (target, confirmed) => events.push(["click", target, confirmed]),
    },
    async (check) => {
      events.push(["observe"]);
      assert.equal(await check(), true);
    },
    async () => null,
    scope,
  );
  assert.deepEqual(events, [
    ["ready", { role: "button", name: "중지", scope }],
    ["click", { role: "button", name: "중지", scope }, true],
    ["observe"],
    ["ready", { role: "button", name: "지금 실행", scope }],
  ]);
  await assert.rejects(
    stopReconciledRuntimeRun(
      {
        waitForTarget: async () => {},
        clickWithConfirmation: async () => {},
      },
      async (check) => {
        if (!(await check())) throw new Error("original run still active");
      },
      async () => ({ jobId: "owned" }),
      scope,
    ),
    /still active/,
  );
});
