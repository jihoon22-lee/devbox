// The caller owns a disposable installed Suite and captured native identities.
import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import { setTimeout as delay } from "node:timers/promises";
import { freePort } from "./workspace-cdp-fixture.mjs";
export async function exerciseAgentWebhooks({ api, call, closeApi, restartApi, connectionStatus, crashAgent, report }) {
  let current = api;
  const invoke = (method, args = {}) => call(current, method, args);
  const evidence = {};
  const progress = (stage) => {
    evidence.stage = stage;
    report(evidence);
  };
  const until = async (read, message) => {
    const deadline = Date.now() + 30000;
    do {
      if (await read()) return;
      await delay(100);
    } while (Date.now() < deadline);
    assert.fail(message);
  };
  const port = await freePort();
  const url = `http://127.0.0.1:${port}/agent-owned-${randomUUID()}`;
  let fixtureId;
  try {
    progress("start-listener");
    await invoke("stop_server");
    const lifecycle = await invoke("lifecycle_status");
    assert.equal(lifecycle.backgroundAvailable, true);
    assert.equal(lifecycle.trayAvailable, false);
    await invoke("set_close_policy", { policy: "keep-listening" });
    await invoke("start_server", { bind: "127.0.0.1", port, allowLan: false });
    await invoke("set_rule", {
      rule: {
        id: randomUUID(),
        priority: 0,
        method: "POST",
        path: new URL(url).pathname,
        status: 202,
        headers: [],
        body: "agent-fixture-response",
        delayMs: 0,
        sequence: [],
      },
      confirmConflicts: true,
    });
    progress("receive-without-api-ui");
    await closeApi(current);
    const received = await fetch(url, {
      method: "POST",
      body: '{"message":"owned fixture"}',
      signal: AbortSignal.timeout(5000),
    });
    assert.equal(received.status, 202);
    assert.equal(await received.text(), "agent-fixture-response");
    current = await restartApi();
    const capture = (await invoke("list_history")).find((item) => item.url === new URL(url).pathname);
    assert.ok(capture, "reopened API Studio lost the request received without its UI");
    const fixture = await invoke("save_fixture", { historyId: capture.id });
    fixtureId = fixture.id;
    assert.ok(fixtureId);
    assert.ok((await invoke("send_history_to_api", { historyId: capture.id })).handoffId);
    assert.ok((await invoke("send_history_to_log_lens", { historyId: capture.id })).handoffId);
    const connection = await connectionStatus(current);
    assert.equal(connection.connected, true);
    assert.equal(connection.connectionState, "connected");
    assert.equal(connection.mode, "auto");
    evidence.restartedApiAutomaticallyConnectsBeforeHandoff = true;
    evidence.closedUiKeepsListenerHistoryAndRules = true;
    evidence.nativeOwnerProjectionsKeepApiAndLogsHandoffs = true;

    progress("agent-restart");
    await crashAgent();
    await until(
      async () =>
        (await current.cdp.evaluate("window.__TAURI_INTERNALS__.invoke('plugin:product-shell|agent_status')")) ===
        "unavailable",
      "API Studio did not observe agent loss",
    );
    await current.cdp.evaluate("window.__TAURI_INTERNALS__.invoke('plugin:product-shell|agent_reconnect')", {
      timeoutMs: 35000,
    });
    const resumed = await invoke("server_status");
    assert.equal(resumed.running, true);
    assert.equal(resumed.address, `127.0.0.1:${port}`);
    assert.equal(resumed.issue, undefined);
    assert.deepEqual(await invoke("list_history"), []);
    assert.deepEqual(await invoke("list_rules"), []);
    assert.ok((await invoke("list_fixtures")).some((item) => item.id === fixtureId));
    const afterRestart = await fetch(url, { signal: AbortSignal.timeout(5000) });
    await afterRestart.text();
    evidence.restartResumesSavedBindAndPreservesFixtures = true;
    evidence.restartClearsOnlyTransientHistoryAndRules = true;

    progress("stop-on-close");
    await invoke("set_close_policy", { policy: "stop-on-close" });
    await closeApi(current);
    await assert.rejects(fetch(url, { signal: AbortSignal.timeout(1000) }));
    current = await restartApi();
    assert.equal((await invoke("server_status")).running, false);
    assert.equal((await invoke("lifecycle_status")).policy, "stop-on-close");
    evidence.explicitStopPolicyClosesListenerAndKeepsSavedChoice = true;
    await invoke("delete_fixture", { id: fixtureId });
    fixtureId = undefined;
    progress("passed");
    return { api: current, evidence };
  } finally {
    await invoke("stop_server").catch(() => {});
    if (fixtureId) await invoke("delete_fixture", { id: fixtureId }).catch(() => {});
  }
}
