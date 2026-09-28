// Installed-only lifetime acceptance. The caller owns the disposable install,
// captured process identities, and hosted Windows teardown.
import assert from "node:assert/strict";
import { mkdirSync, writeFileSync, existsSync, readFileSync } from "node:fs";
import { randomUUID } from "node:crypto";
import path from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { workspaceRequestExpression } from "./windows-workspace-registration.mjs";
import { prepareRuntimeCrash, verifyRuntimeCrash } from "./windows-workspace-runtime-crash.mjs";

export async function exerciseAgentRuntime({
  workspace,
  directory,
  closeWorkspace,
  restartWorkspace,
  agentIdentity,
  crashAgent,
  report,
}) {
  let current = workspace;
  const success = (result) => {
    assert.equal(result.operation.outcome.state, "succeeded", JSON.stringify(result));
    return result.value;
  };
  const call = async (component, method, args = {}) =>
    success(
      await current.cdp.evaluate(workspaceRequestExpression(component, method, args, 29000), { timeoutMs: 35000 }),
    );
  const runtime = (method, args) => call("workspace.runtime", method, args);
  const terminal = (method, args) => call("workspace.terminal", method, args);
  const registry = (method, args) => call("workspace.registry", method, args);
  const until = async (read, message, timeoutMs = 30000) => {
    const end = Date.now() + timeoutMs;
    do {
      const value = await read();
      if (value) return value;
      await delay(100);
    } while (Date.now() < end);
    assert.fail(message);
  };
  const evidence = { stage: "prepare" };
  const stage = (value) => {
    evidence.stage = value;
    report(evidence);
  };
  const script = path.join(directory, "agent-service.cjs");
  writeFileSync(
    script,
    "console.log('agent-session-ready');setInterval(()=>console.log('agent-session-tick'),100);\n",
    { flag: "wx" },
  );
  stage("prepare-project");
  const preview = await registry("preview_windows", { root: directory });
  const context = (
    await registry("apply_registration", {
      previewId: preview.previewId,
      name: "Owned agent lifetime fixture",
      action: "register",
    })
  ).context;
  await registry("select_project", { context });
  stage("prepare-service");
  const service = await runtime("create_service", {
    input: {
      name: "Owned agent service",
      command: `"${process.execPath}" "${script}"`,
      cwd: directory,
      targetKind: "windows",
      targetDistro: null,
      environment: { action: "clear" },
      restartPolicy: "never",
      autoStart: false,
      healthTcpAddress: null,
      healthTcpPort: null,
    },
  });
  const sessionIds = [];
  const sessions = async () => (await terminal("development_sessions")).sessions;
  const startSession = async () => {
    const plan = await terminal("prepare_development_session", {
      operationId: randomUUID(),
      jobs: [service.id],
      terminalProfile: null,
    });
    assert.equal(plan.preflight.executionBlocked, false, JSON.stringify(plan.preflight));
    assert.ok(
      plan.jobs.every((job) => !("environment" in job)),
      "no decrypted environment in preparation metadata",
    );
    sessionIds.push(plan.session.id);
    await terminal("start_development_session", {
      id: plan.session.id,
      revision: plan.session.revision,
      planRevision: plan.session.planRevision,
      mode: "startReviewed",
    });
    await until(
      async () => (await sessions()).find((session) => session.id === plan.session.id && session.phase === "active"),
      "agent Session did not become active",
    );
    return plan.session.id;
  };
  let crashFixture;
  let scheduledJob;
  try {
    stage("prepare-sessions");
    const creator = await startSession();
    const beforeService = await runtime("get_service_instance", { id: service.id });
    const borrower = await startSession();
    assert.equal((await runtime("get_service_instance", { id: service.id })).generation, beforeService.generation);
    const serviceRun = await until(
      async () => (await runtime("service_observability", { id: service.id })).current,
      "service run missing",
    );
    const tail = (cursor) =>
      runtime("tail_log", { input: { runId: serviceRun.id, stream: "stdout", cursor, maxBytes: 8192 } });
    const beforeLog = await until(async () => {
      const batch = await tail(null);
      return Buffer.from(batch.data).toString("utf8").includes("agent-session-ready") && batch;
    }, "service log did not start");
    stage("prepare-crash-recovery");
    const crashDirectory = path.join(directory, "crash");
    mkdirSync(crashDirectory);
    crashFixture = await prepareRuntimeCrash(current.cdp, crashDirectory);
    const beforeJob = await runtime("get_active_run", { id: crashFixture.jobId });
    assert.ok(beforeJob);
    const beforeAgent = agentIdentity();
    const scheduleScript = path.join(directory, "scheduled.cjs");
    const scheduleRecord = path.join(directory, "scheduled.jsonl");
    writeFileSync(
      scheduleScript,
      `require('node:fs').appendFileSync(${JSON.stringify(scheduleRecord)},JSON.stringify({at:Date.now()})+'\\n');`,
      { flag: "wx" },
    );
    scheduledJob = await runtime("create_job", {
      input: {
        name: "Owned agent schedule",
        command: `"${process.execPath}" "${scheduleScript}"`,
        cwd: directory,
        targetKind: "windows",
        targetDistro: null,
        cronExpr: "* * * * *",
        enabled: true,
        environment: { action: "clear" },
      },
    });
    stage("close-workspace");
    await closeWorkspace(current);
    assert.equal(
      await (await fetch(`http://127.0.0.1:${crashFixture.child.port}`, { signal: AbortSignal.timeout(2000) })).text(),
      "fixture",
    );
    const retainedAgent = agentIdentity();
    assert.equal(retainedAgent.Pid, beforeAgent.Pid);
    assert.equal(retainedAgent.Created, beforeAgent.Created);
    evidence.uiExitKeepsAgentAndOwnedJob = true;
    const closedAt = Date.now();
    stage("schedule-without-workspace");
    await until(
      async () =>
        existsSync(scheduleRecord) &&
        readFileSync(scheduleRecord, "utf8")
          .trim()
          .split("\n")
          .some((line) => JSON.parse(line).at >= closedAt),
      "agent did not run a schedule while Workspace was closed",
      75000,
    );
    evidence.scheduledExecutionWithoutUi = true;

    stage("reopen-workspace");
    current = await restartWorkspace();
    await registry("select_project", { context });
    const scheduledRuns = await runtime("list_runs", { jobId: scheduledJob.id, limit: 4 });
    assert.ok(scheduledRuns.length > 0, "scheduled execution must have durable history");
    await runtime("delete_job", { id: scheduledJob.id });
    scheduledJob = undefined;
    const recovered = await sessions();
    for (const id of [creator, borrower]) assert.equal(recovered.find((session) => session.id === id)?.phase, "active");
    assert.equal((await runtime("get_active_run", { id: crashFixture.jobId })).id, beforeJob.id);
    assert.equal((await runtime("get_service_instance", { id: service.id })).generation, beforeService.generation);
    await until(
      async () =>
        Buffer.from((await tail(beforeLog.nextCursor)).data)
          .toString("utf8")
          .includes("agent-session-tick"),
      "same service log did not continue after UI restart",
    );
    evidence.freshUiReattachesSessionsAndLogs = true;
    await terminal("stop_development_session", { id: creator });
    await until(
      async () => (await sessions()).find((session) => session.id === creator && session.phase === "stopped"),
      "creator Session did not release",
    );
    assert.equal((await runtime("get_service_instance", { id: service.id })).state, "running");
    await terminal("stop_development_session", { id: borrower });
    await until(
      async () => (await sessions()).find((session) => session.id === borrower && session.phase === "stopped"),
      "last borrowed Session did not stop",
    );
    assert.equal((await runtime("get_service_instance", { id: service.id })).state, "stopped");
    evidence.sharedCreatorAuthoritySurvivesUiRestart = true;
    if (scheduledJob) await runtime("delete_job", { id: scheduledJob.id }).catch(() => {});
    for (const id of sessionIds) await terminal("archive_development_session", { id });
    sessionIds.length = 0;
    await runtime("delete_service", { id: service.id });

    stage("agent-crash");
    await crashAgent(agentIdentity());
    stage("wait-agent-disconnect");
    await until(async () => {
      const status = await current.cdp.evaluate(
        "window.__TAURI_INTERNALS__.invoke('plugin:product-shell|agent_status')",
      );
      return status === "unavailable";
    }, "disconnected native owner was not observed");
    stage("agent-reconnect");
    assert.equal(
      await current.cdp.evaluate("window.__TAURI_INTERNALS__.invoke('plugin:product-shell|agent_reconnect')", {
        timeoutMs: 35000,
      }),
      "connected",
    );
    Object.assign(
      evidence,
      await verifyRuntimeCrash(current.cdp, crashFixture, (step) => stage(`runtime-recovery-${step}`)),
    );
    crashFixture = undefined;
    const replacement = agentIdentity();
    assert.ok(
      replacement.Pid !== beforeAgent.Pid || replacement.Created !== beforeAgent.Created,
      "agent was not replaced after its verified crash",
    );
    evidence.agentRestartReconcilesDurableControl = true;
    stage("passed");
    return { workspace: current, evidence };
  } finally {
    // Reconcile only this fixture's explicit resources. The outer harness still
    // owns process/namespace cleanup if the connection or generation has failed.
    if (scheduledJob) await runtime("delete_job", { id: scheduledJob.id }).catch(() => {});
    for (const id of sessionIds) await terminal("stop_development_session", { id }).catch(() => {});
    if (crashFixture)
      await runtime("runtime_control", {
        operationId: randomUUID(),
        method: "stop_active_run",
        args: { id: crashFixture.jobId },
      }).catch(() => {});
    await runtime("runtime_control", {
      operationId: randomUUID(),
      method: "stop_service",
      args: { id: service.id },
    }).catch(() => {});
  }
}
