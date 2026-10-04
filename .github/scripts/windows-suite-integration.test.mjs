import * as runner from "./windows-suite-integration.mjs";
import { scenarioModuleContract } from "./windows-api-user-flow-contract.test-support.mjs";
scenarioModuleContract(runner, ["HANDOFF-01", "HANDOFF-02"], "windows-suite-integration.mjs");
import assert from "node:assert/strict";
import test from "node:test";
test("editor context menu readiness precedes one transform selection", async () => {
  const events = [];
  let ready = false;
  await runner.selectSource({
    cdp: {
      command: async (_method, params) => {
        events.push(params.type);
      },
    },
    ui: {
      press: async () => {},
      click: async (target) => {
        if (target.role === "menuitem") {
          assert.equal(ready, true);
          events.push("select");
        }
      },
      waitForTarget: async (target) => {
        assert.equal(target.name, "선택 내용을 API Studio에서 변환");
        ready = true;
        events.push("ready");
      },
    },
  });
  assert.deepEqual(events, ["keyDown", "keyUp", "ready", "select"]);
});
test("incoming review uses accessible region name rather than absent visible heading", async () => {
  const events = [];
  await runner.review({
    cdp: {
      evaluate: async () => {
        throw new Error("aria-label is not visible body text");
      },
    },
    ui: {
      waitForTarget: async (target) => {
        assert.deepEqual(target, { role: "region", name: "다른 제품의 열기 요청" });
        events.push("ready");
      },
      click: async (target) => {
        assert.equal(target.name, "화면 열기");
        assert.equal(target.scope.name, "다른 제품의 열기 요청");
        events.push("click");
      },
    },
  });
  assert.deepEqual(events, ["ready", "click"]);
});
