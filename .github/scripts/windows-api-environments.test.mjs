import * as runner from "./windows-api-environments.mjs";
import { scenarioModuleContract } from "./windows-api-user-flow-contract.test-support.mjs";
scenarioModuleContract(runner, ["ENV-01", "ENV-02"], "windows-api-environments.mjs");
import assert from "node:assert/strict";
import test from "node:test";
import path from "node:path";
import { tmpdir } from "node:os";
test("restart waits for loaded environment name before one selection", async () => {
  const root = path.join(tmpdir(), "devbox-suite-delivery-env-ready", "Suite UI Fixture");
  const events = [];
  const result = await runner.run({
    root,
    fixtureRoot: root,
    installationKey: "a".repeat(64),
    namespace: path.join(path.dirname(root), `com.devbox.v08.apistudio.i${"a".repeat(64)}`),
    sourceSha: "b".repeat(40),
    fixtureSha: "b".repeat(40),
    artifactDigests: Object.fromEntries(Array.from({ length: 7 }, (_, i) => [`fixture${i}`, "c".repeat(64)])),
    cdp: { evaluate: async () => true },
    nativeCall: async () => "synthetic-sealed",
    seedDocument: async () => {},
    restart: async () => {
      events.push("restart");
    },
    ui: {
      press: async () => {},
      confirmDialog: async () => {},
      closeOwnedWindow: async () => {},
      screenshot: async () => "/owned/failure.png",
      fill: async () => {},
      waitForTarget: async (target) => {
        assert.deepEqual(target, { role: "button", name: "Synthetic sparse" });
        events.push("ready");
      },
      click: async (target) => {
        if (target.name === "Synthetic sparse") {
          assert.deepEqual(events, ["restart", "ready"]);
          events.push("select");
          throw new Error("fixture stop after one environment selection");
        }
      },
    },
  });
  assert.equal(result[0].status, "FAIL");
  assert.deepEqual(events, ["restart", "ready", "select"]);
});
test("restarted environment editor observes its textbox before reading enabled state", async () => {
  const events = [];
  let release;
  const pending = new Promise((resolve) => {
    release = resolve;
  });
  const checking = runner.waitEnvironmentEditorReady({
    ui: {
      waitForTarget: async (target) => {
        assert.deepEqual(target, { role: "textbox", name: "환경 이름" });
        events.push("readonly target");
        await pending;
      },
    },
    cdp: {
      evaluate: async (expression) => {
        events.push("enabled probe");
        assert.match(expression, /\?\.disabled===false/);
        return true;
      },
    },
  });
  await new Promise((resolve) => setImmediate(resolve));
  assert.deepEqual(events, ["readonly target"]);
  release();
  await checking;
  assert.deepEqual(events, ["readonly target", "enabled probe"]);
});

test("revision conflict observes the actual native issue rather than generic fallback", async () => {
  const seen = [];
  await runner.expectEnvironmentRevisionConflict({
    cdp: {
      evaluate: async () => {
        seen.push("read");
        return "다른 곳에서 바뀌었습니다. 다시 불러온 뒤 저장해 주세요.";
      },
    },
  });
  assert.equal(seen.length, 1);
});
