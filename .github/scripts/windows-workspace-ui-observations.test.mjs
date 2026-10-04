import assert from "node:assert/strict";
import test from "node:test";
import { navigateWorkspaceFiles, observeWorkspaceFailure } from "./windows-workspace-ui-observations.mjs";
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
