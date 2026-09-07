// Opt-in baseline measurements on disposable Windows only. The caller retains
// the packaged harness's process identities, data transaction and cleanup.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { cpus, release, totalmem } from "node:os";
import path from "node:path";
const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

export function performanceHost() {
  return { osRelease: release(), logicalCpus: cpus().length, cpuModel: cpus()[0]?.model,
    totalMemoryBytes: totalmem(), runnerImage: process.env.ImageOS ?? null,
    runnerImageVersion: process.env.ImageVersion ?? null };
}

export function loadPerformanceConfig(file, tag, commit, hosted) {
  assert.equal(hosted, true, "performance mutation fixtures require disposable hosted Windows");
  const config = JSON.parse(readFileSync(file, "utf8"));
  assert.equal(config.schemaVersion, 1);
  assert.equal(config.baselineTag, tag);
  assert.equal(config.baselineCommit, commit);
  assert.equal(config.idleSampleMs, 5000);
  assert.deepEqual(config.knownBaselineFailures, [{ app: "run-manager", command: "run_job_now", code: "run-execution-failed", runFailureCode: "spawn-failed", evidenceRun: 34099044832, trackingIssue: 547 }]);
  assert.deepEqual([...config.apps].sort(), ["workbench", "api-playground", "knowledge-base", "devbox-manager", "everything-plus", "run-manager", "wsl-desktop"].sort());
  for (const value of Object.values(config.budgets)) assert.ok(Number.isSafeInteger(value) && value > 0);
  assert.ok(Array.isArray(config.unmeasured) && config.unmeasured.length > 0);
  return config;
}

export function evaluateBudgets(measured, config, appId = null) {
  const checks = {
    coldRendererReadyMs: measured.coldRendererReadyMs,
    firstKeyboardEventMs: measured.firstKeyboardEventMs,
    warmExistingWindowMs: measured.warmExistingWindowMs,
    idleCpuPercentOfMachine: measured.idle.cpuPercentOfMachine,
    idleWorkingSetSumBytes: measured.idle.workingSetSumBytes,
    idleProcessCount: measured.idle.processCount,
  };
  const workload = measured.workload;
  if (workload?.indexMs !== undefined) checks.index500FilesMs = workload.indexMs;
  if (workload?.searchMs) checks.searchMs = Math.max(...workload.searchMs);
  if (workload?.completeMs !== undefined) checks.ownedTaskCompleteMs = workload.completeMs;
  if (workload?.profileReadbackMs !== undefined) checks.profileReadbackMs = workload.profileReadbackMs;
  const violations = Object.entries(checks).filter(([name, value]) => !Number.isFinite(value) || value < 0 || !Number.isFinite(config.budgets[name]) || value > config.budgets[name]).map(([name]) => name);
  if (measured.idle.cohortChanged) violations.push("idle-process-cohort-changed");
  if (workload?.result === "failed") violations.push("owned-task-failed");
  const known = config.knownBaselineFailures?.find((entry) => entry.app === appId);
  const knownBaselineFailureObserved = violations.length === 1 && violations[0] === "owned-task-failed"
    && config.baselineTag === "v0.7.0" && config.baselineCommit === "3a23f49c85aa3c3d04b86f227e8aa184ef964085"
    && known !== undefined && workload.failure?.command === known.command
    && workload.failure?.code === known.code && workload.failure?.runFailureCode === known.runFailureCode;
  return { passed: violations.length === 0, violations, knownBaselineFailureObserved,
    r24: "not-complete; failed legacy owned-task latency, live PTY restore and integrated comparison remain unmeasured" };
}

export function summarizeIdle(before, after, elapsedMs, logicalCpus) {
  assert.ok(Number.isFinite(elapsedMs) && elapsedMs > 0 && Number.isInteger(logicalCpus) && logicalCpus > 0);
  assert.ok(before.length > 0 && after.length > 0);
  for (const sample of [...before, ...after]) {
    assert.ok(Number.isInteger(sample.pid) && sample.pid > 0 && typeof sample.created === "string" && sample.created.length > 0);
    assert.ok(Number.isFinite(sample.cpuMs) && sample.cpuMs >= 0 && Number.isSafeInteger(sample.workingSetBytes) && sample.workingSetBytes >= 0);
  }
  const key = (p) => `${p.pid}:${p.created}`;
  const previous = new Map(before.map((p) => [key(p), p]));
  assert.equal(previous.size, before.length);
  let delta = 0, workingSet = 0, changed = before.length !== after.length;
  const seen = new Set();
  for (const sample of after) {
    assert.ok(Number.isInteger(sample.pid) && sample.pid > 0 && typeof sample.created === "string");
    assert.ok(Number.isFinite(sample.cpuMs) && sample.cpuMs >= 0 && Number.isSafeInteger(sample.workingSetBytes) && sample.workingSetBytes >= 0);
    assert.ok(!seen.has(key(sample))); seen.add(key(sample));
    const old = previous.get(key(sample));
    if (!old) changed = true;
    else { assert.ok(sample.cpuMs >= old.cpuMs); delta += sample.cpuMs - old.cpuMs; }
    workingSet += sample.workingSetBytes;
  }
  return {
    sampleMs: Math.round(elapsedMs), logicalCpus, processCount: after.length,
    cpuPercentOfMachine: Math.round(10000 * delta / elapsedMs / logicalCpus) / 100,
    workingSetSumBytes: workingSet, cohortChanged: changed,
    memoryBoundary: "sum of process working sets; shared pages may be counted more than once",
  };
}

function sampleProcesses(identities) {
  assert.equal(process.platform, "win32");
  assert.ok(identities.length > 0 && identities.length <= 64);
  for (const identity of identities) assert.ok(Number.isInteger(identity.Pid) && identity.Pid > 0);
  const encoded = Buffer.from(JSON.stringify(identities)).toString("base64");
  const script = `$ErrorActionPreference='Stop'; $identities=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('${encoded}')) | ConvertFrom-Json; $rows=@(); foreach($identity in $identities) { $record=Get-CimInstance Win32_Process -Filter ('ProcessId='+$identity.Pid); if($null -eq $record) { continue }; $created=[string]$record.CreationDate; if($created -cne $identity.Created) { throw 'process identity changed during measurement' }; $item=Get-Process -Id $identity.Pid; $rows += @{pid=$identity.Pid; created=$created; cpuMs=$item.TotalProcessorTime.TotalMilliseconds; workingSetBytes=$item.WorkingSet64} }; ConvertTo-Json -InputObject $rows -Compress`;
  const result = spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", script], { encoding: "utf8", timeout: 15000, windowsHide: true });
  assert.equal(result.status, 0, "owned process measurement failed");
  return JSON.parse(result.stdout.replace(/^\uFEFF/, ""));
}

export async function measureInput(cdp) {
  await cdp.evaluate(`(() => {
    window.__devboxFoundationInput = false;
    const onKey = (event) => { if (event.code === 'F24') { window.__devboxFoundationInput = true; window.removeEventListener('keydown', onKey, true); } };
    window.addEventListener('keydown', onKey, true);
  })()`);
  const start = performance.now();
  await cdp.send("Input.dispatchKeyEvent", { type: "keyDown", key: "F24", code: "F24", windowsVirtualKeyCode: 135, nativeVirtualKeyCode: 135 });
  await cdp.send("Input.dispatchKeyEvent", { type: "keyUp", key: "F24", code: "F24", windowsVirtualKeyCode: 135, nativeVirtualKeyCode: 135 });
  const accepted = await cdp.evaluate("(() => { const accepted = window.__devboxFoundationInput; delete window.__devboxFoundationInput; return accepted; })()");
  assert.equal(accepted, true, "renderer did not accept the inert keyboard probe");
  return Math.round(performance.now() - start);
}

export async function measureIdle(getIdentities, sampleMs) {
  const before = sampleProcesses(getIdentities());
  const start = performance.now();
  await delay(sampleMs);
  const after = sampleProcesses(getIdentities());
  return summarizeIdle(before, after, performance.now() - start, cpus().length);
}

export async function measureWorkload(app, cdp, isolatedRoot, onStage = () => {}) {
  const invoke = async (command, args = {}) => {
    onStage(`workload:${command}`);
    const result = await cdp.evaluate(`(async () => {
      try { return { ok: true, value: await window.__TAURI_INTERNALS__.invoke(${JSON.stringify(command)}, ${JSON.stringify(args)}) }; }
      catch (error) { return { ok: false, code: ['run-execution-failed', 'run-storage-failed', 'job-schedule-invalid', 'scheduler-unavailable'].includes(error) ? error : 'native-rejected' }; }
    })()`);
    if (result?.ok !== true) {
      onStage(`workload:${command}:${result?.code ?? 'invalid-result'}`);
      const error = new Error(`baseline native command failed: ${command} (${result?.code ?? 'invalid-result'})`);
      error.name = 'AcceptanceError';
      error.nativeCommand = command;
      error.code = result?.code ?? 'invalid-result';
      throw error;
    }
    return result.value;
  };
  if (app.id === "everything-plus") {
    const root = path.join(isolatedRoot, "search-performance-fixture"); mkdirSync(root);
    for (let i = 0; i < 500; i++) writeFileSync(path.join(root, `fixture-${String(i).padStart(4, "0")}.txt`), `synthetic foundation record ${i}\n`.repeat(20), { flag: "wx" });
    const indexingStart = performance.now();
    await invoke("add_root", { path: root, indexContent: true });
    let status;
    while (performance.now() - indexingStart < 30000) {
      status = await invoke("index_status");
      if (!status.indexing && status.total_files === 500) break;
      await delay(100);
    }
    assert.ok(status && !status.indexing && status.total_files === 500, "synthetic index did not complete");
    const indexMs = Math.round(performance.now() - indexingStart), searchMs = [];
    for (let i = 0; i < 10; i++) {
      const start = performance.now();
      const results = await invoke("search_files", { query: "fixture-0042", limit: 10 });
      assert.equal(results.length, 1, "synthetic search result mismatch");
      searchMs.push(Math.round(performance.now() - start));
    }
    return { kind: "500-file-native-index-and-10-searches", result: "measured", indexMs, searchMs };
  }
  if (app.id === "run-manager") {
    const job = await invoke("create_job", { input: { name: "Foundation fixture", command: "exit /b 0", cwd: isolatedRoot, targetKind: "windows", targetDistro: null, cronExpr: "0 0 0 1 1 *", enabled: false, overlapPolicy: "skip", catchUp: false } });
    const start = performance.now();
    try {
      let started;
      try {
        started = await invoke("run_job_now", { id: job.id });
      } catch (error) {
        if (error.nativeCommand !== 'run_job_now' || error.code !== 'run-execution-failed') throw error;
        // Characterize the immutable v0.7 binary's confirmed spawn failure.
        // This remains a failed workload/budget, never a successful latency.
        const runs = await invoke('list_runs', { jobId: job.id, limit: 10 });
        assert.ok(runs.length === 1 && runs[0].jobId === job.id && runs[0].status === 'failed'
          && runs[0].endedAt != null && runs[0].failureCode === 'spawn-failed', 'unexpected legacy execution failure');
        return { kind: "disabled-windows-job-explicit-owned-execution", result: "failed",
          observedFailureMs: Math.round(performance.now() - start),
          failure: { command: error.nativeCommand, code: error.code, runFailureCode: runs[0].failureCode } };
      }
      let run;
      while (performance.now() - start < 30000) {
        run = await invoke("get_run", { id: started.id });
        if (run?.endedAt !== null && run?.endedAt !== undefined) break;
        await delay(50);
      }
      assert.ok(run?.endedAt != null && run.exitCode === 0, "synthetic owned task did not finish successfully");
      return { kind: "disabled-windows-job-explicit-owned-execution", result: "measured", completeMs: Math.round(performance.now() - start) };
    } finally {
      await invoke("stop_active_run", { id: job.id });
    }
  }
  if (app.id === "wsl-desktop") {
    const existingSessions = await invoke("list_sessions");
    const profile = { id: "", name: "Foundation fixture", tabs: [{ id: "fixture-tab", title: "Foundation fixture", customTitle: true, layout: "grid", paneKeys: ["fixture-pane"], sizing: { columns: [1], rows: [1] } }], panes: [{ key: "fixture-pane", distro: "devbox-foundation-unavailable", cwd: "/tmp/devbox-foundation", startCommand: null, multiplexer: "native" }], activeTabId: "fixture-tab", activePaneKey: "fixture-pane" };
    const saved = await invoke("save_workspace_profile", { profile });
    const start = performance.now();
    const restored = await invoke("list_workspace_profiles");
    assert.ok(restored.some((p) => p.id === saved.id && p.panes.length === 1), "terminal profile readback failed");
    const profileReadbackMs = Math.round(performance.now() - start);
    const sessions = await invoke("list_sessions");
    assert.deepEqual(sessions.map((s) => s.id).sort(), existingSessions.map((s) => s.id).sort(), "profile restoration must not create a PTY");
    // Availability is separate from persisted-profile correctness. Hosted
    // Windows may lack a working WSL installation; never report that as zero
    // installed distributions or as a completed live PTY restoration.
    const distributions = await cdp.evaluate(`(async () => {
      try { const values = await window.__TAURI_INTERNALS__.invoke('list_distros'); return { status: 'available', count: values.length }; }
      catch { return { status: 'unavailable', count: null }; }
    })()`);
    return { kind: "saved-terminal-profile-readback-without-execution", result: "measured", profileReadbackMs, distributions, livePtyRestore: "not-run; this fixture measures persisted metadata only" };
  }
  return null;
}
