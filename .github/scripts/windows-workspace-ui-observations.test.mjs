import assert from "node:assert/strict";
import test from "node:test";
import {
  navigateWorkspaceFiles,
  observeWorkspaceFailure,
  stopWorkspaceBeforeDisconnect,
} from "./windows-workspace-ui-observations.mjs";
test("Files navigation observes loaded path before caller input", async () => {
  const events = [];
  await navigateWorkspaceFiles({
    click: async (target) => {
      assert.deepEqual(target, { role: "button", name: "파일" });
      events.push("navigate");
    },
    waitForTarget: async (target) => {
      assert.deepEqual(target, { role: "textbox", name: "열 파일 경로" });
      events.push("ready");
    },
  });
  events.push("input");
  assert.deepEqual(events, ["navigate", "ready", "input"]);
});
test("screenshot failure cannot replace the bounded original nested error", async () => {
  const failure = await observeWorkspaceFailure(
    {
      screenshot: async () => {
        throw new Error("later screenshot failure");
      },
    },
    "WORK-01",
    new Error("original token=synthetic-private"),
  );
  assert.equal(failure.error.message, "original credential=[redacted]");
  assert.deepEqual(failure.screenshotPaths, []);
});

test("crash cleanup preserves the pending reply until the owned renderer exits", async () => {
  let alive = true;
  let pending = true;
  await stopWorkspaceBeforeDisconnect(
    async () => {
      await Promise.resolve();
      alive = false;
    },
    () => {
      if (alive) pending = false;
    },
  );
  assert.equal(alive, false);
  assert.equal(pending, true);
});
test("crash cleanup disconnects after stop failure and preserves the original error", async () => {
  const failure = new Error("owned process stop failed");
  let disconnected = false;
  await assert.rejects(
    stopWorkspaceBeforeDisconnect(
      async () => {
        throw failure;
      },
      () => {
        disconnected = true;
        throw new Error("later disconnect failure");
      },
    ),
    (error) => error === failure,
  );
  assert.equal(disconnected, true);
});
test("crash cleanup reports disconnect failure when stopping succeeded", async () => {
  const failure = new Error("disconnect failed");
  await assert.rejects(
    stopWorkspaceBeforeDisconnect(
      async () => {},
      () => {
        throw failure;
      },
    ),
    (error) => error === failure,
  );
});
