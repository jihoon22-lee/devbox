import assert from "node:assert/strict";
import test from "node:test";
import { reconnectAgent } from "./windows-agent-reconnect.mjs";

test("records the exact product and phase without replaying a failed reconnect", async () => {
  const calls = [],
    reports = [];
  const failure = new Error("original timeout");
  const item = {
    product: "api-studio",
    cdp: {
      evaluate: async (expression, options) => {
        calls.push({ expression, options });
        if (expression.includes("agent_reconnect")) throw failure;
        if (expression === "1") return 1;
        return "connected";
      },
    },
  };
  await assert.rejects(
    reconnectAgent(item, "before-stop", (state) => reports.push(state)),
    (error) => error === failure,
  );
  assert.equal(calls.filter((call) => call.expression.includes("agent_reconnect")).length, 1);
  assert.equal(calls[0].options.timeoutMs, 35000);
  assert.deepEqual(reports.at(-1), {
    product: "api-studio",
    phase: "before-stop",
    stage: "failed",
    rendererResponsive: true,
    nativeStatus: "connected",
  });
});

test("keeps unresponsive renderer and native probes bounded and preserves the original error", async () => {
  const reports = [],
    calls = [];
  const failure = new Error("timeout");
  const item = {
    product: "workspace",
    cdp: {
      evaluate: async (_, options) => {
        calls.push(options.timeoutMs);
        throw failure;
      },
    },
  };
  await assert.rejects(
    reconnectAgent(item, "after-stop", (state) => reports.push(state)),
    (error) => error === failure,
  );
  assert.deepEqual(calls, [35000, 1000, 1000]);
  assert.deepEqual(reports.at(-1), {
    product: "workspace",
    phase: "after-stop",
    stage: "failed",
    rendererResponsive: false,
    nativeStatus: "probe_failed",
  });
});

test("success is connected and arbitrary native details never enter evidence", async () => {
  const reports = [];
  await reconnectAgent({ product: "knowledge", cdp: { evaluate: async () => "connected" } }, "before-stop", (state) =>
    reports.push(state),
  );
  assert.deepEqual(reports, [
    { product: "knowledge", phase: "before-stop", stage: "submitted" },
    { product: "knowledge", phase: "before-stop", stage: "passed" },
  ]);
  const bad = {
    product: "control-center",
    cdp: {
      evaluate: async (expression) =>
        expression.includes("agent_reconnect") ? "private-data" : expression === "1" ? 1 : "private-data",
    },
  };
  await assert.rejects(
    reconnectAgent(bad, "before-stop", (state) => reports.push(state)),
    /agent reconnect did not connect/,
  );
  assert.equal(reports.at(-1).nativeStatus, "unexpected");
  assert.ok(!JSON.stringify(reports).includes("private-data"));
});
