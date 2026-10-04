// Read-only admission for independent journeys after retained installer diagnosis.
import assert from "node:assert/strict";
import { readFile, realpath } from "node:fs/promises";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { diagnosticIdentity, installedFixtureIdentity } from "./suite-user-flow-results.mjs";

export function validateRetainedCommittedInstall({
  root,
  runnerTemp,
  staging,
  sourceSha,
  installationKey,
  owner,
  registration,
  manifest,
  activation,
}) {
  const scratch = path.dirname(root);
  const relative = path.relative(runnerTemp, scratch);
  assert.ok(relative && !relative.startsWith("..") && !path.isAbsolute(relative), "Owned hosted fixture root required");
  assert.match(path.basename(scratch), /^devbox-suite-delivery-[a-f0-9]{32}$/);
  assert.equal(path.basename(root), "Suite UI Fixture");
  assert.equal(owner.schemaVersion, 1);
  assert.equal(owner.root, root);
  assert.equal(owner.sourceSha, sourceSha);
  assert.equal(owner.staging, staging);
  assert.match(installationKey, /^[a-f0-9]{64}$/);
  assert.equal(owner.installationKey, installationKey);
  assert.equal(registration.installationKey, installationKey, "Owned installation registration changed");
  assert.equal(activation.schemaVersion, 1);
  assert.equal(activation.phase, "committed", "Independent retained journeys require committed installation");
  assert.ok(manifest.installationId && manifest.generation);
  assert.equal(activation.installationId, manifest.installationId);
  assert.equal(activation.generation, manifest.generation);
  return {
    installationKey,
    installationId: manifest.installationId,
    generation: manifest.generation,
    phase: activation.phase,
  };
}

export async function verifyRetainedCommittedInstall() {
  assert.equal(process.platform, "win32");
  assert.equal(process.env.GITHUB_ACTIONS, "true");
  assert.equal(process.env.RUNNER_ENVIRONMENT, "github-hosted");
  const diagnostic = await diagnosticIdentity();
  assert.equal(diagnostic?.diagnosticOnly, true, "Retained diagnostic identity required");
  assert.equal(diagnostic.promotionEvidence, false);
  const installationKey = await installedFixtureIdentity();
  const declaredRoot = process.env.DEVBOX_USER_FLOW_INSTALL_ROOT;
  const root = await realpath(declaredRoot);
  assert.equal(root.toLowerCase(), path.resolve(declaredRoot).toLowerCase(), "Fixture root must not redirect");
  const read = async (file) => JSON.parse((await readFile(file, "utf8")).replace(/^\uFEFF/u, ""));
  const [owner, registration, manifest, activation] = await Promise.all([
    read(path.join(path.dirname(root), "user-flow-owner.json")),
    read(path.join(root, "suite-registration.json")),
    read(path.join(root, "devbox-installation.json")),
    read(path.join(root, "devbox-activation.json")),
  ]);
  const admission = validateRetainedCommittedInstall({
    root: declaredRoot,
    runnerTemp: await realpath(process.env.RUNNER_TEMP),
    staging: path.resolve("candidate/delivery"),
    sourceSha: diagnostic.sourceSha,
    installationKey,
    owner,
    registration,
    manifest,
    activation,
  });
  return { ...diagnostic, ...admission };
}
if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  console.log(JSON.stringify(await verifyRetainedCommittedInstall()));
}
