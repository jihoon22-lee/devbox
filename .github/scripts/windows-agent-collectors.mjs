// Only called by the hosted disposable Suite fixture; no user data is sampled.
import assert from "node:assert/strict";
import { createOwnedActivityWindow } from "./windows-owned-activity-window.mjs";
import { mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { setTimeout as delay } from "node:timers/promises";
export async function exerciseAgentCollectors({
  knowledge,
  directory,
  call,
  closeKnowledge,
  restartKnowledge,
  agentIdentity,
  report,
}) {
  let current = knowledge;
  const invoke = (component, method, args = {}) => call(current, component, method, args);
  const evidence = {};
  const root = path.join(directory, "collector-root");
  mkdirSync(root);
  const identity = agentIdentity();
  const sameAgent = () => {
    const observed = agentIdentity();
    assert.equal(observed.Pid, identity.Pid);
    assert.equal(observed.Created, identity.Created);
  };
  let added = false;
  let foreground;
  const oldThreshold = await invoke("activity", "get_idle_threshold");
  try {
    assert.equal(await invoke("activity", "is_tracking"), false, "collection starts only with explicit consent");
    await invoke("search_settings", "add_root", { path: root, indexContent: false });
    added = true;
    const deadline = Date.now() + 30000;
    while ((await invoke("search", "index_status")).indexing && Date.now() < deadline) await delay(100);
    assert.equal((await invoke("search", "index_status")).indexing, false);
    await invoke("activity", "start_tracking");
    assert.equal(await invoke("activity", "is_tracking"), true);
    await invoke("activity", "set_idle_threshold", { thresholdMs: 60000 });
    foreground = await createOwnedActivityWindow(path.join(directory, "owned-foreground"));
    await foreground.input();
    await delay(4000);
    const lastInput = await foreground.input();
    await delay(65000);
    const day = new Date(lastInput),
      dayStart = new Date(day.getFullYear(), day.getMonth(), day.getDate()).getTime(),
      dayEnd = new Date(day.getFullYear(), day.getMonth(), day.getDate() + 1).getTime();
    const sessions = (await invoke("activity", "timeline", { dayStart, dayEnd })).filter((row) =>
      row.title.startsWith(foreground.marker),
    );
    assert.ok(sessions.length > 0, "owned HWND must produce a real native session");
    assert.ok(
      sessions.every((row) => row.end_ts >= row.start_ts && row.end_ts <= lastInput + 2500),
      "idle tail must stop at owned input",
    );
    await foreground.close();
    foreground = null;
    await closeKnowledge(current);
    const changedAt = Date.now();
    writeFileSync(path.join(root, "agentclosedwindowfixture.txt"), "synthetic background index fixture\n", {
      flag: "wx",
    });
    await delay(10000);
    sameAgent();
    const reopenedAt = Date.now();
    current = await restartKnowledge();
    sameAgent();
    assert.equal(await invoke("activity", "is_tracking"), true);
    const status = await invoke("search", "index_status");
    assert.ok(
      status.last_indexed_at >= changedAt && status.last_indexed_at < reopenedAt,
      "the agent must process the file event before Knowledge is reopened",
    );
    let result = await invoke("search", "source_query", {
      source: "files",
      query: "agentclosedwindowfixture",
      mode: "name",
      limit: 20,
      filter: {},
    });
    const generation = result.generation;
    for (let attempt = 0; attempt < 60 && result.state === "running"; attempt++) {
      await delay(80);
      result = await invoke("search", "source_poll", { generation });
    }
    assert.equal(result.state, "complete");
    assert.ok(result.rows.some((row) => row.value.name === "agentclosedwindowfixture.txt"));
    await invoke("search", "source_cancel", { generation });
    await invoke("activity", "stop_tracking");
    assert.equal(await invoke("activity", "is_tracking"), false);
    evidence.consentAndPauseAreShared = true;
    evidence.closeExitsKnowledgeAndKeepsSameAgent = true;
    evidence.fileWatcherCompletesBeforeKnowledgeReopens = true;
    evidence.reopenedSearchReadsTheBackgroundIndex = true;
    evidence.physicalForegroundSessionCapture = "owned HWND plus actual input and native persisted session";
    evidence.idleBoundaryExcludesTail = true;
    evidence.evidenceKind = "native-boundary";
    report(evidence);
    return { knowledge: current, evidence };
  } finally {
    if (foreground) await foreground.close();
    await invoke("activity", "set_idle_threshold", { thresholdMs: oldThreshold }).catch(() => {});
    await invoke("activity", "stop_tracking").catch(() => {});
    if (added) await invoke("search_settings", "remove_root", { path: root }).catch(() => {});
  }
}
