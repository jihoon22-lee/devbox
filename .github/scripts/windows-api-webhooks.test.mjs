import * as runner from "./windows-api-webhooks.mjs";
import { scenarioModuleContract } from "./windows-api-user-flow-contract.test-support.mjs";
scenarioModuleContract(runner, ["WEB-01", "WEB-02"], "windows-api-webhooks.mjs");

import assert from "node:assert/strict";
import test from "node:test";
import path from "node:path";
import { tmpdir } from "node:os";
test("lazy route readiness precedes exactly one first target input", async () => {
  const root = path.join(tmpdir(), "devbox-suite-delivery-readiness", "Suite UI Fixture");
  const events = [];
  let ready = false;
  const result = await runner.run({
    root,
    fixtureRoot: root,
    installationKey: "a".repeat(64),
    namespace: path.join(path.dirname(root), `com.devbox.v08.apistudio.i${"a".repeat(64)}`),
    sourceSha: "b".repeat(40),
    fixtureSha: "b".repeat(40),
    artifactDigests: Object.fromEntries(Array.from({ length: 7 }, (_, i) => [`fixture${i}`, "c".repeat(64)])),
    cdp: { evaluate: async () => true },
    nativeCall: async () => {},
    ui: {
      press: async () => {},
      confirmDialog: async () => {},
      closeOwnedWindow: async () => {},
      screenshot: async () => "/owned/failure.png",
      waitForTarget: async (target) => {
        assert.deepEqual(target, { role: "spinbutton", name: "포트" });
        events.push("ready");
        ready = true;
      },
      click: async (target) => {
        if (target.name === "웹훅 및 모의 서버") {
          events.push("navigate");
          return;
        }
        throw new Error("unexpected click");
      },
      fill: async (target) => {
        assert.equal(target.name, "포트");
        assert.equal(ready, true);
        events.push("input");
        throw new Error("fixture stop after first input");
      },
    },
  });
  assert.equal(result[0].status, "FAIL");
  assert.deepEqual(events, ["navigate", "ready", "input"]);
});

test("one Stop click waits for asynchronous native stopped observation", async () => {
  const root = path.join(tmpdir(), "devbox-suite-delivery-stop", "Suite UI Fixture");
  let port,
    stopping = false,
    stopClicks = 0,
    reads = 0;
  const result = await runner.run({
    root,
    fixtureRoot: root,
    installationKey: "a".repeat(64),
    namespace: path.join(path.dirname(root), `com.devbox.v08.apistudio.i${"a".repeat(64)}`),
    sourceSha: "b".repeat(40),
    fixtureSha: "b".repeat(40),
    artifactDigests: Object.fromEntries(Array.from({ length: 7 }, (_, i) => [`fixture${i}`, "c".repeat(64)])),
    cdp: { evaluate: async () => `127.0.0.1:${port}` },
    nativeCall: async (_command, method) => {
      assert.equal(method, "server_status");
      assert.equal(stopping, true);
      return { running: ++reads === 1 };
    },
    ui: {
      press: async () => {},
      confirmDialog: async () => {},
      closeOwnedWindow: async () => {},
      screenshot: async () => "/owned/failure.png",
      waitForTarget: async () => {},
      click: async (target) => {
        if (target.name === "중지") {
          stopping = true;
          stopClicks++;
        }
      },
      fill: async (target, value) => {
        if (target.name === "포트") port = value;
        else throw new Error("synthetic scenario failure before cleanup");
      },
    },
  });
  assert.equal(result[0].status, "FAIL");
  assert.equal(result[0].error.message, "synthetic scenario failure before cleanup");
  assert.equal(stopClicks, 1);
  assert.equal(reads, 2);
});
