import assert from "node:assert/strict";
import test from "node:test";
import { validateUninstallDiagnostic } from "./windows-suite-uninstall-diagnostic.mjs";
const env = { GITHUB_ACTIONS: "true", RUNNER_ENVIRONMENT: "github-hosted", RUNNER_TEMP: "C:\\fixture" };
const root = `C:\\fixture\\devbox-suite-delivery-${"a".repeat(32)}\\Suite UI Fixture`;
env.DEVBOX_USER_FLOW_INSTALL_ROOT = root;
const evidence = `C:\\evidence\\owned-cleanup-${"b".repeat(64)}.json`;
const failed = {
  schemaVersion: 1,
  stage: "wait-uninstaller",
  status: "failed",
  exitCode: 1,
  updatePending: false,
  restorePending: false,
  uninstallPending: false,
  removalReceipt: false,
};
test("only original exit 1 before any partial removal permits one visible diagnostic", () => {
  validateUninstallDiagnostic(failed, root, evidence, env, "win32");
  for (const patch of [
    { exitCode: 0 },
    { exitCode: 2 },
    { status: "completed" },
    { stage: "start-uninstall" },
    { updatePending: true },
    { restorePending: true },
    { uninstallPending: true },
    { removalReceipt: true },
  ])
    assert.throws(() => validateUninstallDiagnostic({ ...failed, ...patch }, root, evidence, env, "win32"));
  assert.throws(() => validateUninstallDiagnostic(failed, root, evidence, env, "linux"));
  assert.throws(() =>
    validateUninstallDiagnostic(
      failed,
      root,
      evidence,
      { ...env, DEVBOX_USER_FLOW_INSTALL_ROOT: root + "foreign" },
      "win32",
    ),
  );
});

import { runUninstallDiagnostic } from "./windows-suite-uninstall-diagnostic.mjs";
test("acquisition failure replaces running diagnostic with fixed failure receipt", async () => {
  const receipts = [];
  await runUninstallDiagnostic(
    root,
    evidence,
    async () => {
      throw new Error("private path");
    },
    async (receipt) => {
      receipts.push(receipt);
    },
  );
  assert.deepEqual(receipts, [{ schemaVersion: 1, status: "FAIL", originalExitCode: 1, inspectionUnavailable: true }]);
});
test("a captured native failure survives outer diagnostic catch", async () => {
  const receipts = [];
  await runUninstallDiagnostic(
    root,
    evidence,
    async (_root, { failureObservation }) => {
      await failureObservation({ stage: "removal execution", issues: ["bootstrap_owner_changed"] });
      throw new Error("first");
    },
    async (receipt) => {
      receipts.push(receipt);
    },
  );
  assert.equal(receipts.length, 1);
  assert.deepEqual(receipts[0].issues, ["bootstrap_owner_changed"]);
});
