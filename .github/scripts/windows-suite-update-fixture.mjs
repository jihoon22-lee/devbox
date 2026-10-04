// L4 preparation of a distinct retained generation with the exact candidate images.
// This is not evidence of interactive installation or published-version download.
import assert from "node:assert/strict";
import { mkdir, readFile, writeFile, copyFile } from "node:fs/promises";
import path from "node:path";
import { spawn } from "node:child_process";
import { waitForFixtureChildExit } from "./fixture-child-exit.mjs";
import { fileDigest } from "./suite-user-flow-results.mjs";
import { completeInstalledHealth } from "./windows-suite-health-actions.mjs";
import { observeAgentUpdateQuiesce } from "./windows-suite-agent-user-flows.mjs";
export async function runFixtureHelper(image, args) {
  const child = spawn(image, args, { stdio: ["ignore", "pipe", "pipe"], windowsHide: true });
  let output = "",
    problem = "";
  child.stdout.on("data", (chunk) => {
    output += chunk.toString();
    if (output.length > 1024 * 1024) child.kill();
  });
  child.stderr.on("data", (chunk) => {
    problem = (problem + chunk.toString()).slice(-1000);
  });
  const [code] = await waitForFixtureChildExit(child, 180000);
  assert.equal(code, 0, `Owned fixture helper failed: ${problem}`);
  return output.trim() ? JSON.parse(output) : null;
}
export async function prepareDistinctGeneration(identity) {
  const root = process.env.DEVBOX_USER_FLOW_INSTALL_ROOT,
    scratch = path.dirname(root);
  const original = JSON.parse(await readFile(path.join(root, "devbox-installation.json"), "utf8"));
  const registration = JSON.parse(await readFile(path.join(root, "suite-registration.json"), "utf8"));
  const directory = path.join(scratch, "alternate-payload");
  await mkdir(directory);
  const staging = path.resolve("candidate/delivery");
  const originalBytes = await readFile(path.join(staging, "suite-payload.json"), "utf8");
  const payload = JSON.parse(originalBytes);
  assert.equal(payload.sourceSha, identity.sourceSha);
  for (const product of payload.products) {
    const destination = path.join(directory, product.portable.name);
    await copyFile(path.join(staging, product.portable.name), destination);
    assert.equal(await fileDigest(destination), product.portable.sha256);
  }
  const helper = path.join(directory, "devbox-suite-bootstrap.exe");
  await copyFile(path.join(staging, "devbox-suite-bootstrap.exe"), helper);
  const expectedHelper = payload.products
    .find((p) => p.id === "control-center")
    .files.find((f) => f.name === "resources/suite/devbox-suite-bootstrap.exe");
  assert.equal(await fileDigest(helper), expectedHelper.sha256);
  const next = path.join(directory, "suite-payload.json");
  await writeFile(next, originalBytes + "\n ", { flag: "wx" });
  const prepared = await runFixtureHelper(helper, ["--prepare-update", root, next]);
  assert.equal(prepared.state, "updatePrepared");
  await runFixtureHelper(helper, ["--apply-update", root, next, prepared.operationId]);
  const manifest = JSON.parse(await readFile(path.join(root, "devbox-installation.json"), "utf8"));
  assert.equal(manifest.installationId, original.installationId);
  assert.notEqual(manifest.generation, original.generation);
  const screenshots = await completeInstalledHealth("업데이트 확정", {
    beforeCommit: async ({ center, reviewScreenshot }) => {
      await observeAgentUpdateQuiesce({
        root,
        manifest,
        identity,
        installationKey: registration.installationKey,
        action: "actual-control-center-update",
        actualUpdateHealthGate: true,
        previousInstallationId: original.installationId,
        center,
        reviewScreenshot,
      });
    },
  });
  return {
    manifest,
    screenshots,
    fixturePreparation:
      "exact candidate image bytes, alternate retained payload encoding; native prepare/apply, visible reviewed health/commit",
  };
}
