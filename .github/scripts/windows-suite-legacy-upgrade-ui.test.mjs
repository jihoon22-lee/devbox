import assert from "node:assert/strict";
import test from "node:test";
import { validateOwnedLegacyRun, runLegacyUpgradeUserFlow } from "./windows-suite-legacy-upgrade-ui.mjs";
for (const guard of [validateOwnedLegacyRun, runLegacyUpgradeUserFlow])
  test(`${guard.name} rejects an unowned legacy fixture before changing installation environment`, async () => {
    const previous = process.env.DEVBOX_USER_FLOW_INSTALL_ROOT;
    await assert.rejects(guard(), /win32|true|undefined/);
    assert.equal(process.env.DEVBOX_USER_FLOW_INSTALL_ROOT, previous);
  });
