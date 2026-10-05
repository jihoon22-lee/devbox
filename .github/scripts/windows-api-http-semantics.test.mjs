import * as runner from "./windows-api-http-semantics.mjs";
import { scenarioModuleContract } from "./windows-api-user-flow-contract.test-support.mjs";
scenarioModuleContract(runner, ["HTTP-01", "HTTP-02", "HTTP-03"], "windows-api-http-semantics.mjs");

import assert from "node:assert/strict";
import test from "node:test";
import { sendHttpFixtureRequest } from "./windows-api-http-semantics.mjs";
test("HTTP completed text cannot release next request before the send button becomes ready", async () => {
  let completeReady;
  const pending = new Promise((resolve) => {
    completeReady = resolve;
  });
  const fixture = { hits: [] };
  let waits = 0,
    clicks = 0,
    finished = false;
  const context = {
    ui: {
      waitForTarget: async (target) => {
        assert.deepEqual(target, { role: "button", name: "보내기" });
        if (++waits === 2) await pending;
      },
      click: async (target) => {
        assert.deepEqual(target, { role: "button", name: "보내기" });
        clicks++;
        fixture.hits.push({ body: "owned response" });
      },
    },
    cdp: { evaluate: async () => "요청이 완료되었습니다." },
  };
  const result = sendHttpFixtureRequest(context, fixture).then((value) => {
    finished = true;
    return value;
  });
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(waits, 2);
  assert.equal(clicks, 1);
  assert.equal(finished, false);
  completeReady();
  assert.deepEqual(await result, { body: "owned response" });
  assert.equal(clicks, 1);
});
test("HTTP readiness failure prevents any send mutation", async () => {
  const original = new Error("send readiness unavailable");
  let clicks = 0;
  await assert.rejects(
    sendHttpFixtureRequest(
      {
        ui: {
          waitForTarget: async () => {
            throw original;
          },
          click: async () => {
            clicks++;
          },
        },
      },
      { hits: [] },
    ),
    (error) => error === original,
  );
  assert.equal(clicks, 0);
});
