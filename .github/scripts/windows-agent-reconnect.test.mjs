import assert from "node:assert/strict";
import test from "node:test";
import { reconnectAgent, observeReconnectBaseline } from "./windows-agent-reconnect.mjs";

test("verified crash recovery accepts a replacement already started by background reads", async () => {
  const calls = [],
    reports = [];
  await reconnectAgent(
    {
      product: "workspace",
      cdp: {
        evaluate: async (expression) => {
          calls.push(expression);
          return "connected";
        },
      },
    },
    "after-crash",
    (state) => reports.push(state),
  );
  assert.deepEqual(calls, ["window.__TAURI_INTERNALS__.invoke('plugin:product-shell|agent_reconnect')"]);
  assert.equal(reports.at(-1).stage, "passed");
});

test("the control skips only the startup observer and records that absence explicitly", async () => {
  let probes = 0;
  const observation = {
    state: "observed",
    browserResponsive: true,
    rendererResponsive: true,
    nativeStatus: "connected",
  };
  const item = {
    cdp: {
      probeNewSession: async () => {
        probes++;
        return observation;
      },
    },
  };
  assert.deepEqual(await observeReconnectBaseline(item, false), { state: "disabled" });
  assert.equal(probes, 0);
  assert.deepEqual(await observeReconnectBaseline(item), { state: "disabled" });
  assert.equal(probes, 0);
  assert.deepEqual(await observeReconnectBaseline(item, true), observation);
  assert.equal(probes, 1);
});

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

test("a responsive fresh observer never converts the original reconnect failure into success", async () => {
  const failure = new Error("original failure");
  let reconnectCalls = 0;
  let observations = 0;
  const reports = [];
  const fresh = { state: "observed", browserResponsive: true, rendererResponsive: true, nativeStatus: "connected" };
  const item = {
    product: "api-studio",
    inspectCdpHost: async () => ({ state: "observed", listener: "owned" }),
    cdpBaseline: { ...fresh, nativeStatus: "starting" },
    cdp: {
      evaluate: async (expression) => {
        if (expression.includes("agent_reconnect")) reconnectCalls++;
        throw failure;
      },
      probeNewSession: async () => {
        observations++;
        return fresh;
      },
    },
  };
  await assert.rejects(
    reconnectAgent(item, "before-stop", (state) => reports.push(state)),
    (error) => error === failure,
  );
  assert.equal(reconnectCalls, 1);
  assert.equal(observations, 1);
  assert.equal(reports.at(-1).stage, "failed");
  assert.deepEqual(reports.at(-1).freshObserver, fresh);
  assert.deepEqual(reports.at(-1).freshObserverBaseline, item.cdpBaseline);
  assert.deepEqual(reports.at(-1).nativeObserver, { state: "observed", listener: "owned" });
});

test("foreground recovery is observed only after failure is recorded and never replays reconnect", async () => {
  const failure = new Error("original timeout"),
    reports = [];
  let focused = false,
    reconnects = 0;
  const item = {
    product: "api-studio",
    focusCdpHost: async () => {
      assert.equal(reports.at(-1).stage, "failed");
      assert.equal(reports.at(-1).rendererResponsive, false);
      focused = true;
      return { state: "focused", wasForeground: false, wasMinimized: false };
    },
    cdp: {
      evaluate: async (expression) => {
        if (expression.includes("agent_reconnect")) {
          reconnects++;
          throw failure;
        }
        if (!focused) throw failure;
        return expression === "1" ? 1 : "connected";
      },
      probeNewSession: async () => ({ state: focused ? "observed" : "open_failed" }),
    },
  };
  await assert.rejects(
    reconnectAgent(item, "before-stop", (state) => reports.push(state)),
    (error) => error === failure,
  );
  assert.equal(reconnects, 1);
  assert.deepEqual(reports.at(-1).foregroundControl.original, { rendererResponsive: true, nativeStatus: "connected" });
  assert.equal(reports.at(-1).foregroundControl.fresh.state, "observed");
  assert.equal(reports.at(-1).stage, "failed");
});

test("an unconfirmed foreground change cannot trigger recovery reads or hide the original failure", async () => {
  for (const rejected of [false, true]) {
    const failure = new Error("original timeout"),
      reports = [];
    let reads = 0;
    const item = {
      product: "api-studio",
      focusCdpHost: async () => {
        if (rejected) throw new Error("private");
        return { state: "not_focused", wasForeground: false, wasMinimized: false };
      },
      cdp: {
        evaluate: async () => {
          reads++;
          throw failure;
        },
      },
    };
    await assert.rejects(
      reconnectAgent(item, "before-stop", (value) => reports.push(value)),
      (error) => error === failure,
    );
    assert.equal(reads, 3);
    assert.equal(reports.at(-1).foregroundControl.original, undefined);
    assert.equal(reports.at(-1).foregroundControl.activation.state, rejected ? "probe_failed" : "not_focused");
  }
});

test("wait inspection follows persisted failure and cannot turn recovery into success", async () => {
  const reports = [];
  const failure = new Error("original timeout");
  let waits = 0;
  const item = {
    product: "api-studio",
    cdp: {
      evaluate: async () => {
        throw failure;
      },
    },
    inspectCdpWaits: async () => {
      assert.equal(reports.at(-1).stage, "failed");
      waits++;
      return { state: "observed", threads: [] };
    },
    focusCdpHost: async () => ({ state: "not_focused" }),
  };
  await assert.rejects(
    reconnectAgent(item, "before-stop", (value) => reports.push(structuredClone(value))),
    (error) => error === failure,
  );
  assert.equal(waits, 1);
  assert.equal(reports.at(-1).waitObserver.state, "observed");
  assert.equal(reports.at(-1).stage, "failed");
  assert.equal(reports[1].waitObserver, undefined);
  item.inspectCdpWaits = async () => {
    throw new Error("private detail");
  };
  await assert.rejects(
    reconnectAgent(item, "before-stop", (value) => reports.push(value)),
    (error) => error === failure,
  );
  assert.deepEqual(reports.at(-1).waitObserver, { state: "probe_failed" });
  item.cdp.evaluate = async () => "connected";
  item.inspectCdpWaits = async () => {
    assert.fail("success must not inspect waits");
  };
  await reconnectAgent(item, "before-stop", () => {});
});

test("snapshot stacks follow failure, never run on success, and preserve the original error", async () => {
  const reports = [],
    failure = new Error("original timeout");
  const item = {
    product: "api-studio",
    cdp: {
      evaluate: async () => {
        throw failure;
      },
    },
    inspectCdpStacks: async () => {
      assert.equal(reports.at(-1).stage, "failed");
      return { state: "observed", app: { state: "observed", frames: [] } };
    },
  };
  await assert.rejects(
    reconnectAgent(item, "before-stop", (value) => reports.push(structuredClone(value))),
    (error) => error === failure,
  );
  assert.equal(reports.at(-1).stackObserver.state, "observed");
  item.inspectCdpStacks = async () => {
    throw new Error("private detail");
  };
  await assert.rejects(
    reconnectAgent(item, "before-stop", (value) => reports.push(value)),
    (error) => error === failure,
  );
  assert.deepEqual(reports.at(-1).stackObserver, { state: "probe_failed" });
  item.cdp.evaluate = async () => "connected";
  item.inspectCdpStacks = async () => {
    assert.fail("successful reconnect must not capture a snapshot");
  };
  await reconnectAgent(item, "before-stop", () => {});
});
