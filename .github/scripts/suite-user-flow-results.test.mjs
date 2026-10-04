import assert from "node:assert/strict";
import { test } from "node:test";
import { mkdtemp, writeFile, readFile, rm } from "node:fs/promises";
import path from "node:path";
import os from "node:os";
import { diagnosticIdentity } from "./suite-user-flow-results.mjs";

test("retained UI diagnostics require a consistent explicit dispatch receipt", async () => {
  const names = [
    "DEVBOX_USER_FLOW_DIAGNOSTIC",
    "DEVBOX_USER_FLOW_DIAGNOSTIC_SOURCE",
    "DEVBOX_USER_FLOW_DIAGNOSTIC_RUN",
    "DEVBOX_USER_FLOW_DIAGNOSTIC_RECEIPT",
    "GITHUB_EVENT_NAME",
    "GITHUB_WORKFLOW",
    "GITHUB_SHA",
    "GITHUB_RUN_ID",
    "GITHUB_REPOSITORY",
  ];
  const previous = Object.fromEntries(names.map((name) => [name, process.env[name]]));
  const directory = await mkdtemp(path.join(os.tmpdir(), "devbox-diagnostic-identity-"));
  try {
    for (const name of names) delete process.env[name];
    assert.equal(await diagnosticIdentity(), null);
    process.env.DEVBOX_USER_FLOW_DIAGNOSTIC_SOURCE = "a".repeat(40);
    await assert.rejects(diagnosticIdentity());
    Object.assign(process.env, {
      DEVBOX_USER_FLOW_DIAGNOSTIC: "true",
      DEVBOX_USER_FLOW_DIAGNOSTIC_RUN: "123",
      DEVBOX_USER_FLOW_DIAGNOSTIC_RECEIPT: path.join(directory, "receipt.json"),
      GITHUB_EVENT_NAME: "workflow_dispatch",
      GITHUB_WORKFLOW: "Product foundation acceptance",
      GITHUB_SHA: "b".repeat(40),
      GITHUB_RUN_ID: "456",
      GITHUB_REPOSITORY: "fixture/devbox",
    });
    const workflow = await readFile(new URL("../workflows/product-foundation.yml", import.meta.url), "utf8");
    const purpose = workflow.match(/purpose = '([^']+)'; appsOnly = \$appsOnly/);
    assert.ok(purpose, "Installed diagnostic modes must emit the shared receipt purpose");
    const receipt = {
      purpose: purpose[1],
      appsOnly: true,
      runnerSourceSha: process.env.GITHUB_SHA,
      runnerRunId: "456",
      payloadSourceSha: "a".repeat(40),
      payloadRunId: "123",
      repository: "fixture/devbox",
      sourceWorkflow: ".github/workflows/windows-package-candidate.yml",
      assemblySucceeded: true,
      diagnosticOnly: true,
      promotionEvidence: false,
    };
    const save = () => writeFile(process.env.DEVBOX_USER_FLOW_DIAGNOSTIC_RECEIPT, JSON.stringify(receipt));
    await save();
    const identity = await diagnosticIdentity();
    assert.equal(identity.runnerSourceSha, "b".repeat(40));
    assert.equal(identity.sourceSha, "a".repeat(40));
    assert.equal(identity.diagnosticOnly, true);
    assert.equal(identity.promotionEvidence, false);
    receipt.payloadRunId = "124";
    await save();
    await assert.rejects(diagnosticIdentity());
    receipt.payloadRunId = "123";
    receipt.promotionEvidence = true;
    await save();
    await assert.rejects(diagnosticIdentity());
  } finally {
    for (const [name, value] of Object.entries(previous)) {
      if (value === undefined) delete process.env[name];
      else process.env[name] = value;
    }
    await rm(directory, { recursive: true, force: true });
  }
});
