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
  fixture.agentFailureOperations = async () => {
    throw new Error("diagnostic failure");
  };
  fixture.cleanup = async () => {
    throw new Error("agent cleanup failure");
  };
  const results = await run({ workspaceFixture: fixture, ui: { screenshot: async () => "/owned/agent-first.png" } });
  assert.ok(results.every((result) => result.status === "FAIL"));
  assert.equal(results[0].error.message, "agent original failure");
  assert.deepEqual(results[0].agentFailureOperations, { unavailable: true });
  assert.equal(results[0].cleanupError.message, "agent cleanup failure");
  assert.deepEqual(results[0].screenshotPaths, ["/owned/agent-first.png"]);
});
test("first Agent navigation waits the title before issuing its single fill", async () => {
  const actions = [];
  let ready = false;
  const stop = new Error("owned preparation boundary reached");
  const fixture = {
    prepareAgent: async () => {},
    configureAgent: async () => {
      throw stop;
    },
    registry: async () => ({ worktrees: [{ projectId: "owned-project", id: "owned-base" }] }),
    context: async () => ({ projectId: "owned-project", worktreeId: "owned-base" }),
    wait: async () => {},
    trustSource: async () => {},
    attemptAgentReview: async () => {},
    crashAndReopen: async () => {},
    cleanup: async () => {},
  };
  const results = await run({
    workspaceFixture: fixture,
    ui: {
      click: async (target) => actions.push(["click", target.name]),
      waitForTarget: async (target) => {
        assert.equal(target.name, "제목");
        ready = true;
        actions.push(["ready", target.name]);
      },
      fill: async (target) => {
        assert.ok(ready, "Agent title still loading");
        actions.push(["fill", target.name]);
      },
      screenshot: async () => "/owned/failure.png",
    },
  });
  assert.equal(results[0].error.message, stop.message);
  assert.deepEqual(actions, [
    ["click", "에이전트"],
    ["ready", "제목"],
    ["fill", "제목"],
  ]);
});
test("task discard confirms once only after asynchronous review is ready", async () => {
  const { discardAgentTask } = await import("./windows-workspace-agent-registry-ui.mjs");
  const events = [];
  let ready = false;
  await discardAgentTask({
    click: async (target) => {
      if (target.name === "버리기 확인") assert.ok(ready);
      events.push(target.name);
    },
    waitForTarget: async (target) => {
      if (target.name === "버리기") {
        assert.deepEqual(events, []);
        return;
      }
      assert.equal(target.name, "버리기 확인");
      assert.deepEqual(events, ["버리기"]);
      ready = true;
    },
  });
  assert.deepEqual(events, ["버리기", "버리기 확인"]);
});

test("Agent notification dismissal uses actual input only when a toast is visible", async () => {
  const { dismissWorkspaceUndo } = await import("./windows-workspace-agent-registry-ui.mjs");
  for (const visible of [false, true]) {
    const events = [];
    let shown = visible;
    await dismissWorkspaceUndo(
      {
        click: async (target) => {
          events.push(target);
          shown = false;
        },
      },
      async () => shown,
      async (probe) => assert.equal(await probe(), true),
    );
    assert.deepEqual(events, visible ? [{ role: "button", name: "알림 닫기" }] : []);
  }
});

test("review observes idle Source then explicitly approves the newly selected worktree", async () => {
  const { reviewAgentWorktree } = await import("./windows-workspace-agent-registry-ui.mjs");
  const events = [];
  await reviewAgentWorktree(
    {
      waitForTarget: async (target) => events.push(`ready:${target.name}`),
      click: async (target) => events.push(`click:${target.name}`),
    },
    {
      waitForAgentSourceIdle: async () => events.push("idle"),
      trustSource: async () => events.push("approve-current-worktree"),
    },
  );
  assert.deepEqual(events, [
    "idle",
    "ready:변경 검토",
    "click:변경 검토",
    "ready:Git 실행 승인",
    "approve-current-worktree",
  ]);
});

test("discard waits for the remounted Agent list before actual input", async () => {
  const { discardAgentTask } = await import("./windows-workspace-agent-registry-ui.mjs");
  const ready = new Set();
  const clicks = [];
  await discardAgentTask({
    waitForTarget: async ({ name }) => ready.add(name),
    click: async ({ name }) => {
      assert.ok(ready.has(name), `${name} has not appeared after navigation`);
      clicks.push(name);
    },
  });
  assert.deepEqual(clicks, ["버리기", "버리기 확인"]);
});
