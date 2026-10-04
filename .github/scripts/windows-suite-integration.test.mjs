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
