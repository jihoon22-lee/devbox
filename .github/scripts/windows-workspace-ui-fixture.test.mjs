import assert from "node:assert/strict";
import test from "node:test";
import { mkdtemp, rm } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import {
  createWorkspaceUiFixture,
  prepareTerminalStart,
  dismissFailedManagedInstall,
  loseRuntimeReply,
  openCurrentProjectTerminal,
  waitForRuntimeControlIdle,
} from "./windows-workspace-ui-fixture.mjs";

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
  let listener,
    release,
    responsePaused = false;
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
        if (method === "Debugger.setBreakpointOnFunctionCall") return { breakpointId: "owned-breakpoint" };
        if (method === "Debugger.resume") {
          responsePaused = true;
          listener({ callFrames: [{ callFrameId: "owned-frame" }] });
        }
        if (method === "Debugger.evaluateOnCallFrame") {
          if (readFailure) throw readFailure;
          if (params.expression === "window.__TAURI_INTERNALS__.runCallback")
            return { result: { objectId: "owned-callback" } };
          assert.equal(params.callFrameId, "owned-frame");
          if (!responsePaused)
            return { result: { value: { requestId: "owned-request", operationId: "original-owned-operation" } } };
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

test("Agent review waits for the selected Source root and its aggregate idle control", async () => {
  let reads = 0;
  const fixture = createWorkspaceUiFixture({
    cdp: {
      evaluate: async (expression) => {
        if (expression.includes("workspace-native-source")) {
          reads++;
          return reads > 1;
        }
        if (expression.includes('"snapshot"'))
          return {
            operation: { outcome: { state: "succeeded" } },
            value: { worktrees: [{ id: "tree", projectId: "project", binding: { root: "/owned" } }] },
          };
        return { context: { projectId: "project", worktreeId: "tree" } };
      },
    },
    ui: {},
  });
  await fixture.waitForAgentSourceIdle();
  assert.equal(reads, 2);
});

test("lost reply requires its exact native unacknowledged receipt after crash", async () => {
  const { assertLostRuntimeReceipt } = await import("./windows-workspace-ui-fixture.mjs");
  const receipt = {
    operationId: "original",
    targetId: "job",
    method: "run_job_now",
    state: "completed",
    reviewed: false,
  };
  assert.doesNotThrow(() => assertLostRuntimeReceipt([receipt], "original", "job", "run_job_now"));
  for (const receipts of [
    [],
    [{ ...receipt, reviewed: true }],
    [{ ...receipt, operationId: "another" }],
    [{ ...receipt, targetId: "foreign" }],
  ])
    assert.throws(() => assertLostRuntimeReceipt(receipts, "original", "job", "run_job_now"));
});

test("lost reply binds the intended control request before accepting its response", async () => {
  const { events, options } = pausedReplyFixture({ action: "중지" });
  await loseRuntimeReply(options);
  const conditions = events
    .filter(([event]) => event === "Debugger.setBreakpointOnFunctionCall")
    .map(([, params]) => params.condition);
  assert.equal(conditions.length, 2);
  const matchRequest = new Function("cmd", "payload", `return ${conditions[0]}`);
  const request = (method) => ({ request: { method: "runtime_control", args: { method, args: { id: "owned-job" } } } });
  assert.equal(matchRequest("plugin:workspace|runtime", request("run_job_now")), false);
  assert.equal(matchRequest("plugin:workspace|runtime", request("stop_active_run")), true);
  const matchResponse = new Function("data", `return ${conditions[1]}`);
  assert.equal(
    matchResponse({ value: { jobId: "owned-job" }, operation: { provenance: { requestId: "earlier-run" } } }),
    false,
  );
  assert.equal(
    matchResponse({ value: { jobId: "owned-job" }, operation: { provenance: { requestId: "owned-request" } } }),
    true,
  );
});

test("terminal route waits for its actionable opener after lazy navigation", async () => {
  const events = [];
  let ready = false;
  await openCurrentProjectTerminal({
    click: async ({ name }) => {
      if (name !== "터미널") assert.ok(ready);
      events.push(name);
    },
    waitForTarget: async ({ name }) => {
      assert.equal(name, "현재 프로젝트의 터미널 열기");
      ready = true;
    },
  });
  assert.deepEqual(events, ["터미널", "현재 프로젝트의 터미널 열기"]);
});
test("next lost reply waits for the prior local and durable acknowledgement", async () => {
  let local = [{ operationId: "prior" }],
    receipts = [{ targetId: "job", reviewed: false }];
  await waitForRuntimeControlIdle(
    () => local,
    () => receipts,
    async (probe) => {
      assert.equal(await probe(), false);
      local = [];
      assert.equal(await probe(), false);
      receipts = [];
      assert.equal(await probe(), true);
    },
    "job",
  );
});

test("terminal companion waits for its renderer and review before actual input", async () => {
  const actions = [];
  let ready = false,
    reviewed = false;
  await prepareTerminalStart(
    {
      async waitForTarget(target) {
        actions.push(target.name);
        if (target.name === "시작 경로") {
          // TerminalToolbar's input has a datalist: Chromium exposes a combobox.
          assert.equal(target.role, "combobox");
          ready = true;
        }
        if (target.name === "실행") reviewed = true;
      },
      async fill(target) {
        assert.equal(ready, true);
        if (target.name === "시작 경로") assert.equal(target.role, "combobox");
        actions.push(target.name);
      },
      async click(target) {
        if (target.name === "실행") assert.equal(reviewed, true);
        actions.push(target.name);
      },
    },
    "owned-root",
    "owned-command",
  );
  assert.deepEqual(actions, ["시작 경로", "시작 경로", "시작 명령", "+ 터미널", "실행", "실행"]);
});

test("failed managed install is observed before cancel and waits for dialog retirement", async () => {
  let failed = false,
    ready = false,
    closing = false,
    open = true;
  const confirmation = { role: "dialog", name: "관리형 서버 작업 확인" };
  await dismissFailedManagedInstall({
    ui: {
      async text() {
        failed = true;
        return "관리형 서버를 설치하지 못했습니다.";
      },
      async waitForTarget(target) {
        assert.equal(failed, true);
        assert.deepEqual(target.scope, confirmation);
        ready = true;
      },
      async click(target) {
        assert.equal(ready, true);
        assert.equal(target.name, "취소");
        closing = true;
      },
    },
    async isOpen() {
      return open;
    },
    async wait(predicate) {
      if (closing) {
        assert.equal(await predicate(), false);
        open = false;
      }
      assert.equal(await predicate(), true);
    },
  });
  assert.equal(open, false);
});

test("owned WSL lifetime validates ownership and releases its exact child on failure", async () => {
  const { withOwnedWslRunning } = await import("./windows-workspace-ui-fixture.mjs");
  const { EventEmitter } = await import("node:events");
  const owner = { name: "DevboxKnowledgeFixture-42-aabbccddeeff", runId: "42" };
  const original = new Error("journey failure");
  let released = false,
    launches = 0;
  const launch = (exe, args) => {
    launches++;
    assert.equal(exe, "wsl.exe");
    assert.deepEqual(args.slice(0, 3), ["--distribution", owner.name, "--exec"]);
    const child = new EventEmitter();
    child.exitCode = null;
    child.signalCode = null;
    child.stdout = new EventEmitter();
    child.stdin = Object.assign(new EventEmitter(), {
      end() {
        released = true;
        child.exitCode = 0;
        child.emit("exit", 0, null);
      },
    });
    queueMicrotask(() => child.stdout.emit("data", Buffer.from("ready\n")));
    return child;
  };
  await assert.rejects(
    withOwnedWslRunning(
      owner,
      "42",
      async () => {
        throw original;
      },
      launch,
    ),
    (error) => error === original,
  );
  assert.equal(released, true);
  await assert.rejects(withOwnedWslRunning(owner, "other", async () => {}, launch));
  assert.equal(launches, 1);
});

test("managed LSP native observation follows settled renderer and never competes with its refresh", async () => {
  const { observeManagedLspTransition } = await import("./windows-workspace-ui-fixture.mjs");
  for (const ready of [true, false]) {
    let observed = false;
    const error = new Error("renderer refresh failed");
    const result = observeManagedLspTransition(
      {
        waitForTarget: async () => {
          if (!ready) throw error;
        },
      },
      {},
      "제거",
      async () => {
        observed = true;
      },
    );
    if (ready) await result;
    else await assert.rejects(result, (value) => value === error);
    assert.equal(observed, ready);
  }
});

test("owned WSL pipe errors preserve journey failure while still awaiting child exit", async () => {
  const { withOwnedWslRunning } = await import("./windows-workspace-ui-fixture.mjs");
  const { EventEmitter } = await import("node:events");
  for (const sync of [false, true]) {
    const child = new EventEmitter();
    child.exitCode = null;
    child.signalCode = null;
    child.stdout = new EventEmitter();
    child.stdin = new EventEmitter();
    const pipeError = new Error("owned pipe failed"),
      original = new Error("original journey failed");
    let exited = false;
    child.stdin.end = () => {
      setImmediate(() => {
        exited = true;
        child.exitCode = 0;
        child.emit("exit", 0, null);
      });
      if (sync) throw pipeError;
      child.stdin.emit("error", pipeError);
    };
    const launch = () => {
      queueMicrotask(() => child.stdout.emit("data", Buffer.from("ready\n")));
      return child;
    };
    await assert.rejects(
      withOwnedWslRunning(
        { runId: "42", name: "DevboxKnowledgeFixture-42-aabbccddeeff" },
        "42",
        async () => {
          throw original;
        },
        launch,
      ),
      (error) => error === original,
    );
    assert.equal(exited, true);
  }
});

test("managed LSP readiness preserves the transition budget before native observation", async () => {
  const { observeManagedLspTransition } = await import("./windows-workspace-ui-fixture.mjs");
  for (const timeoutMs of [undefined, 120_000]) {
    let observed = false;
    await observeManagedLspTransition(
      {
        waitForTarget: async (_target, options) => {
          assert.equal(options.timeoutMs, timeoutMs ?? 30_000);
          assert.equal(observed, false);
        },
      },
      {},
      "제거",
      async () => {
        observed = true;
      },
      timeoutMs,
    );
    assert.equal(observed, true);
  }
});

test("terminal profile deletion waits its queued dialog and companion acknowledgement before stale main open", async () => {
  const { deleteTerminalProfile } = await import("./windows-workspace-ui-fixture.mjs");
  const events = [];
  let removed = false;
  await deleteTerminalProfile(
    {
      ui: {
        waitForTarget: async (target) => events.push(`ready:${target.name}`),
        click: async (target) => events.push(`click:${target.name}`),
      },
      cdp: { evaluate: async () => removed },
    },
    {
      click: async (target) => {
        assert.equal(removed, true);
        events.push(`main:${target.name}`);
      },
    },
    async (probe) => {
      assert.equal(await probe(), false);
      removed = true;
      assert.equal(await probe(), true);
    },
  );
  assert.deepEqual(events, [
    "ready:owned terminal profile 프로필 삭제",
    "click:owned terminal profile 프로필 삭제",
    "ready:삭제",
    "click:삭제",
    "main:프로필로 터미널 열기",
  ]);
});
