// Only reached after PowerShell preserves exit 1 and revalidates native root identity,
// unchanged registered files, no live products, and no pending/partial removal.
import assert from "node:assert/strict";
import path from "node:path";
import { readFile, writeFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";
import { runVisibleRemoval } from "./windows-suite-installer-actions.mjs";
export function validateUninstallDiagnostic(
  observation,
  root,
  evidence,
  env = process.env,
  platform = process.platform,
) {
  assert.equal(platform, "win32");
  assert.equal(env.GITHUB_ACTIONS, "true");
  assert.equal(env.RUNNER_ENVIRONMENT, "github-hosted");
  assert.equal(root, env.DEVBOX_USER_FLOW_INSTALL_ROOT);
  assert.equal(path.win32.basename(root), "Suite UI Fixture");
  assert.match(path.win32.basename(path.win32.dirname(root)), /^devbox-suite-delivery-[a-f0-9]{32}$/);
  assert.equal(
    path.win32.dirname(path.win32.dirname(root)).toLowerCase(),
    env.RUNNER_TEMP.replace(/[\\/]$/, "").toLowerCase(),
  );
  assert.match(path.win32.basename(evidence), /^owned-cleanup-[a-f0-9]{64}\.json$/);
  assert.equal(observation.schemaVersion, 1);
  assert.equal(observation.stage, "wait-uninstaller");
  assert.equal(observation.status, "failed");
  assert.equal(observation.exitCode, 1);
  for (const key of ["updatePending", "restorePending", "uninstallPending", "removalReceipt"])
    assert.equal(observation[key], false);
}
export async function diagnoseUninstall(root, evidence) {
  const original = JSON.parse(await readFile(evidence, "utf8"));
  validateUninstallDiagnostic(original, root, evidence);
  const output = evidence.replace(/\.json$/, "-visible.json");
  // Exclusive receipt prevents repeated removal attempts, including interrupted diagnostics.
  await writeFile(output, JSON.stringify({ schemaVersion: 1, status: "running", originalExitCode: 1 }), { flag: "wx" });
  try {
    await runVisibleRemoval(root, {
      failureObservation: async (observation) => {
        await writeFile(
          output,
          JSON.stringify({ schemaVersion: 1, status: "FAIL", originalExitCode: 1, ...observation }, null, 2),
        );
      },
    });
    await writeFile(output, JSON.stringify({ schemaVersion: 1, status: "completed", originalExitCode: 1 }));
  } catch {
    /* Original cleanup remains FAIL; owned removal already preserves fixed evidence and releases UI. */
  }
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href)
  await diagnoseUninstall(...process.argv.slice(2));
