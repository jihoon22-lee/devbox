import * as runner from "./windows-api-transforms.mjs";
import { scenarioModuleContract } from "./windows-api-user-flow-contract.test-support.mjs";
scenarioModuleContract(runner, ["TRANSFORM-01"], "windows-api-transforms.mjs");
import assert from "node:assert/strict";
import test from "node:test";
test("transform setup navigates once and observes Requests URL readiness before one fill", async () => {
  const events = [];
  let release;
  const pending = new Promise((resolve) => {
    release = resolve;
  });
  const preparing = runner.prepareTransformRequest(
    {
      ui: {
        click: async (target) => {
          assert.deepEqual(target, { role: "button", name: "요청" });
          events.push("navigation");
        },
        waitForTarget: async (target) => {
          assert.deepEqual(target, { role: "textbox", name: "요청 URL" });
          events.push("readonly target");
          await pending;
        },
        fill: async (target, value) => {
          assert.equal(target.name, "요청 URL");
          assert.equal(value, "http://127.0.0.1/synthetic");
          events.push("fill");
        },
      },
    },
    "http://127.0.0.1/synthetic",
  );
  await new Promise((resolve) => setImmediate(resolve));
  assert.deepEqual(events, ["navigation", "readonly target"]);
  release();
  await preparing;
  assert.deepEqual(events, ["navigation", "readonly target", "fill"]);
});
