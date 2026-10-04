import assert from "node:assert/strict";
import test from "node:test";
import {
  beforeAgentProductPreparation,
  afterAgentProductPreparation,
  afterAgentCommit,
  observeAgentUpdateQuiesce,
  until,
} from "./windows-suite-agent-user-flows.mjs";
for (const hook of [
  beforeAgentProductPreparation,
  afterAgentProductPreparation,
  afterAgentCommit,
  observeAgentUpdateQuiesce,
])
  test(`${hook.name} rejects missing actual installed ownership before side effects`, async () => {
    await assert.rejects(hook({}), /win32|true|realpath|sourceSha|Cannot|undefined/);
  });
test("bounded Agent observations do not accept a missing condition", async () => {
  await assert.rejects(
    until(() => false, "real condition missing", 1),
    /real condition missing/,
  );
});
test("Agent close/update receipt aggregation rejects a different installation or scope", async () => {
  const { validateAgentOwnershipReceipts } = await import("./windows-suite-agent-user-flows.mjs");
  const identity = {
      sourceSha: "a".repeat(40),
      fixtureSha: "a".repeat(40),
      artifactDigests: { image: "b".repeat(64) },
    },
    context = { identity, installationKey: "c".repeat(64), manifest: { installationId: "owned" } },
    common = {
      ...identity,
      installationKey: context.installationKey,
      screenshotPaths: ["product-foundation-evidence/user-flows/screenshots/agent/proof.png"],
    },
    portable = { ...common, portableLocalOwnerExited: true, installedAgentUnchanged: true },
    update = {
      ...common,
      updateQuiesced: true,
      previousInstallationId: "owned",
      proofScope: "actual-candidate-update-review-and-health-gate",
    };
  assert.throws(() =>
    validateAgentOwnershipReceipts(context, { ...portable, installationKey: "d".repeat(64) }, update),
  );
  assert.throws(() => validateAgentOwnershipReceipts(context, portable, { ...update, proofScope: "native-update" }));
  assert.throws(() =>
    validateAgentOwnershipReceipts(context, { ...portable, screenshotPaths: ["outside.png"] }, update),
  );
});
test("Agent business job waits lazy create and saved card readiness before returning", async () => {
  const { createAgentBusinessJob } = await import("./windows-suite-agent-user-flows.mjs");
  const events = [],
    job = { id: "owned", name: "Owned Agent job" };
  const app = {
    cdp: { evaluate: async () => ({ operation: { outcome: { state: "succeeded" } }, value: [job] }) },
    ui: {
      click: async (target) => {
        events.push(["click", target.name]);
      },
      fill: async () => {},
      waitForTarget: async (target) => {
        events.push(["ready", target.name]);
        if (target.name === "지금 실행") assert.deepEqual(target.scope, { role: "article", name: job.name });
      },
    },
  };
  assert.equal(
    await createAgentBusinessJob(app, { name: job.name, command: "owned command", directory: "owned directory" }),
    job,
  );
  assert.deepEqual(events, [
    ["click", "작업 및 서비스"],
    ["ready", "+ 새 작업"],
    ["click", "+ 새 작업"],
    ["click", "작업 저장"],
    ["ready", "지금 실행"],
  ]);
});

test("paused Agent receipt is read in its exact call frame without scheduling renderer execution", async () => {
  const { readPausedAgentRequest } = await import("./windows-suite-agent-user-flows.mjs");
  const calls = [];
  const cdp = {
    evaluate: async () => {
      throw new Error("paused Runtime evaluation would deadlock");
    },
    command: async (method, args) => {
      calls.push(method);
      assert.equal(method, "Debugger.evaluateOnCallFrame");
      assert.equal(args.callFrameId, "owned-frame");
      assert.equal(args.returnByValue, true);
      assert.ok(args.expression.includes("devbox-runtime-pending:installed-owner:run_job_now:owned-job"));
      assert.ok(!args.expression.includes("Object.keys"));
      return {
        result: { value: [{ operationId: "aaaaaaaa-1111-2222-3333-444444444444", args: { id: "owned-job" } }] },
      };
    },
  };
  assert.deepEqual(await readPausedAgentRequest(cdp, "owned-frame", "owned-job", "installed-owner"), {
    operationId: "aaaaaaaa-1111-2222-3333-444444444444",
    args: { id: "owned-job" },
  });
  assert.deepEqual(calls, ["Debugger.evaluateOnCallFrame"]);
  await assert.rejects(readPausedAgentRequest(cdp, null, "owned-job", "installed-owner"), /call frame/);
  for (const response of [
    { exceptionDetails: {} },
    { result: { value: [] } },
    { result: { value: [{}, {}] } },
    { result: { value: [{}] } },
    { result: { value: [{ operationId: "aaaaaaaa-1111-2222-3333-444444444444", args: { id: "foreign-job" } }] } },
  ]) {
    await assert.rejects(
      readPausedAgentRequest({ command: async () => response }, "owned-frame", "owned-job", "installed-owner"),
    );
  }
});
