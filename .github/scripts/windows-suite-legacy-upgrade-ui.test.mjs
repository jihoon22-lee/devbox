import assert from "node:assert/strict";
import test from "node:test";
import {
  validateOwnedLegacyRun,
  runLegacyUpgradeUserFlow,
  legacyOperationFailure,
} from "./windows-suite-legacy-upgrade-ui.mjs";
test("legacy subprocess evidence exposes bounded operation and issue without arbitrary stdout", () => {
  const error = legacyOperationFailure(
    "prepare-pinned-import",
    1,
    null,
    JSON.stringify({ issue: "bootstrap_owner_invalid", token: "synthetic-private-token" }),
    "Bearer synthetic-private-token",
  );
  assert.equal(error.operation.issue, "bootstrap_owner_invalid");
  assert.equal(error.operation.exitCode, 1);
  assert.equal(error.operation.operation, "prepare-pinned-import");
  assert.equal(JSON.stringify(error.operation).includes("synthetic-private-token"), false);
  const timeout = legacyOperationFailure("cleanup-owned-installation", null, "SIGTERM", "private-stdout", "");
  assert.equal(timeout.operation.signal, "SIGTERM");
  assert.equal(timeout.operation.issue, null);
});
for (const guard of [validateOwnedLegacyRun, runLegacyUpgradeUserFlow])
  test(`${guard.name} rejects an unowned legacy fixture before changing installation environment`, async () => {
    const previous = process.env.DEVBOX_USER_FLOW_INSTALL_ROOT;
    await assert.rejects(guard(), /win32|true|undefined/);
    assert.equal(process.env.DEVBOX_USER_FLOW_INSTALL_ROOT, previous);
  });
