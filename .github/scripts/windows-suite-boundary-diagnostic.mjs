// Focused hosted probes. These receipts never enter the packaged user-flow matrix.
import assert from "node:assert/strict";
import path from "node:path";
import { readFile, realpath, mkdir, writeFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";
import { packagedIdentity } from "./suite-user-flow-results.mjs";
import { createInstalledProductContext } from "./windows-suite-ui-context.mjs";
import { verifiedScope, nativeStatus, trayQuitReconnect, until } from "./windows-suite-agent-user-flows.mjs";
import { run as runDelivery } from "./windows-suite-delivery-user-flows.mjs";
import { runLegacyUpgradeUserFlow } from "./windows-suite-legacy-upgrade-ui.mjs";
import { withOwnedCleanup } from "./owned-fixture-cleanup.mjs";
import { boundedFailure } from "./user-flow-failure-evidence.mjs";

export async function exerciseOwnedTrayBoundary(
  context,
  app,
  { status = nativeStatus, tray = trayQuitReconnect, observe = until } = {},
) {
  return withOwnedCleanup(
    async () => {
      assert.equal(app.root, context.root);
      assert.equal(app.installationKey, context.installationKey);
      // Connection readiness is sufficient; no Workspace registry/project fixture
      // is needed to exercise the real tray quit and explicit reconnect controls.
      await observe(async () => (await status(app)) === "connected", "tray probe native connection ready");
      return tray(context, app);
    },
    () => app.close(),
  );
}
export async function runTrayBoundary(identity) {
  const root = await realpath(process.env.DEVBOX_USER_FLOW_INSTALL_ROOT);
  const read = async (name) => JSON.parse((await readFile(path.join(root, name), "utf8")).replace(/^\uFEFF/u, ""));
  const registration = await read("suite-registration.json");
  const manifest = await read("devbox-installation.json");
  const context = await verifiedScope({ root, manifest, identity, installationKey: registration.installationKey });
  const app = await createInstalledProductContext("workspace");
  return exerciseOwnedTrayBoundary(context, app);
}
export const runDeliveryBoundary = () => runDelivery({ checkpointOnly: true });
async function persistBoundary(receipt) {
  await mkdir("product-foundation-evidence", { recursive: true });
  await writeFile(`product-foundation-evidence/suite-boundary-${receipt.mode}.json`, JSON.stringify(receipt, null, 2), {
    flag: "wx",
  });
}
export async function executeBoundaryProbes(
  identity,
  {
    modes = ["tray", "delivery"],
    tray = runTrayBoundary,
    delivery = runDeliveryBoundary,
    legacy = runLegacyUpgradeUserFlow,
    persist = persistBoundary,
  } = {},
) {
  assert.equal(identity.diagnosticOnly, true, "Diagnostic-only payload identity required");
  assert.equal(identity.promotionEvidence, false, "Promotion evidence prohibited");
  const receipts = [];
  for (const mode of modes) {
    assert.ok(["tray", "delivery", "legacy"].includes(mode), "Exact boundary diagnostic mode required");
    const receipt = {
      ...identity,
      schemaVersion: 1,
      mode,
      diagnosticOnly: true,
      promotionEvidence: false,
      runnerSourceSha: process.env.GITHUB_SHA,
      runnerRunId: process.env.GITHUB_RUN_ID,
    };
    try {
      const result = await (mode === "tray" ? tray(identity) : mode === "legacy" ? legacy() : delivery());
      const passed =
        mode === "tray" ||
        (mode === "legacy"
          ? result?.id === "DELIVERY-01" && result.status === "PASS"
          : Array.isArray(result) &&
            result.length === 1 &&
            result[0].id === "CHECKPOINT-DIAGNOSTIC" &&
            result[0].status === "PASS");
      receipt.status = passed ? "PASS" : "FAIL";
      receipt.observation = result;
    } catch (error) {
      receipt.status = "FAIL";
      receipt.error = boundedFailure(error);
    }
    try {
      await persist(receipt);
    } catch (error) {
      receipt.status = "FAIL";
      receipt.evidenceWriteFailure = boundedFailure(error);
    }
    receipts.push(receipt);
  }
  return receipts;
}
export async function runBoundaryDiagnostic(mode = "both") {
  assert.ok(["both", "tray", "delivery", "legacy"].includes(mode), "Exact boundary diagnostic mode required");
  assert.equal(process.platform, "win32");
  assert.equal(process.env.GITHUB_ACTIONS, "true");
  assert.equal(process.env.RUNNER_ENVIRONMENT, "github-hosted");
  return executeBoundaryProbes(await packagedIdentity(), { modes: mode === "both" ? ["tray", "delivery"] : [mode] });
}
if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  const receipts = await runBoundaryDiagnostic(process.argv[2]);
  if (receipts.some((receipt) => receipt.status !== "PASS")) process.exitCode = 1;
}
