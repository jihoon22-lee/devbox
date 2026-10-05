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

test("delivered transform waits for automatic Requests preview without clicking covered navigation", async () => {
  const events = [];
  await runner.deliverTransformPreview({
    ui: {
      click: async (target) => {
        assert.equal(target.name, "API Playground로 전달");
        events.push("deliver");
      },
      waitForTarget: async (target) => {
        assert.deepEqual(target, { role: "dialog", name: "Toolbox 텍스트 요청 미리보기" });
        events.push("preview-ready");
      },
    },
  });
  assert.deepEqual(events, ["deliver", "preview-ready"]);
});

test("new pipeline waits for renderer readiness after native deletion commits", async () => {
  const events = [];
  let release;
  const ready = new Promise((resolve) => {
    release = resolve;
  });
  const operation = runner.beginNewPipeline({
    ui: {
      waitForTarget: async (target) => {
        assert.deepEqual(target, { role: "button", name: "새 파이프라인" });
        events.push("observe");
        await ready;
      },
      click: async (target) => {
        assert.equal(target.name, "새 파이프라인");
        events.push("click");
      },
    },
  });
  await new Promise((resolve) => setImmediate(resolve));
  assert.deepEqual(events, ["observe"]);
  release();
  await operation;
  assert.deepEqual(events, ["observe", "click"]);
});

test("recipient apply waits for acknowledged preview removal before opening body", async () => {
  const events = [];
  let observations = 0;
  await runner.applyTransformBody({
    ui: {
      click: async (target) => {
        events.push(target.name);
      },
    },
    cdp: {
      evaluate: async () => {
        events.push("observe");
        return ++observations > 1;
      },
    },
  });
  assert.deepEqual(events, ["적용", "observe", "observe", "BODY"]);
});
