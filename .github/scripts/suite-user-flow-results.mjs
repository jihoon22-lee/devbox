// Shared identity and output format for real packaged user-flow runners.
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { lstat, mkdir, readFile, readdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { revalidationIdentity } from "./revalidation-identity.mjs";
export async function fileDigest(file) {
  const stat = await lstat(file);
  assert.ok(stat.isFile() && !stat.isSymbolicLink(), "Regular evidence/package file required");
  const hash = createHash("sha256");
  for await (const chunk of createReadStream(file)) hash.update(chunk);
  return hash.digest("hex");
}
export async function diagnosticIdentity() {
  const source = process.env.DEVBOX_USER_FLOW_DIAGNOSTIC_SOURCE;
  const run = process.env.DEVBOX_USER_FLOW_DIAGNOSTIC_RUN;
  const receiptPath = process.env.DEVBOX_USER_FLOW_DIAGNOSTIC_RECEIPT;
  const mode = process.env.DEVBOX_USER_FLOW_DIAGNOSTIC;
  if ([source, run, receiptPath, mode].every((value) => value === undefined)) return null;
  assert.equal(mode, "true", "Explicit retained UI diagnostic mode required");
  assert.equal(process.env.GITHUB_EVENT_NAME, "workflow_dispatch");
  assert.equal(process.env.GITHUB_WORKFLOW, "Product foundation acceptance");
  assert.match(source ?? "", /^[a-f0-9]{40}$/);
  assert.match(run ?? "", /^[0-9]+$/);
  assert.ok(receiptPath);
  const receipt = JSON.parse((await readFile(receiptPath, "utf8")).replace(/^\uFEFF/u, ""));
  assert.equal(receipt.purpose, "retained-installer-ui-diagnostic-only");
  assert.equal(receipt.runnerSourceSha, process.env.GITHUB_SHA);
  assert.equal(receipt.runnerRunId, process.env.GITHUB_RUN_ID);
  assert.equal(receipt.payloadSourceSha, source);
  assert.equal(receipt.payloadRunId, run);
  assert.equal(receipt.repository, process.env.GITHUB_REPOSITORY);
  assert.equal(receipt.sourceWorkflow, ".github/workflows/windows-package-candidate.yml");
  assert.equal(receipt.assemblySucceeded, true);
  assert.equal(receipt.diagnosticOnly, true);
  assert.equal(receipt.promotionEvidence, false);
  return {
    sourceSha: source,
    runnerSourceSha: receipt.runnerSourceSha,
    runnerRunId: receipt.runnerRunId,
    payloadSourceSha: source,
    payloadRunId: run,
    diagnosticOnly: true,
    promotionEvidence: false,
  };
}
export async function packagedIdentity(assets = process.env.DEVBOX_USER_FLOW_ASSETS, sourceSha) {
  const diagnostic = await diagnosticIdentity();
  const revalidation = await revalidationIdentity();
  sourceSha ??= diagnostic?.sourceSha ?? revalidation?.sourceSha ?? process.env.GITHUB_SHA;
  if (diagnostic) assert.equal(sourceSha, diagnostic.sourceSha);
  if (revalidation) assert.equal(sourceSha, revalidation.sourceSha);
  assert.ok(assets, "Exact candidate assets required for packaged user flows");
  assert.match(sourceSha ?? "", /^[a-f0-9]{40}$/);
  const root = path.resolve(assets);
  const manifest = JSON.parse(await readFile(path.join(root, "release-manifest.json"), "utf8"));
  assert.equal(manifest.sourceSha, sourceSha);
  assert.equal(manifest.schemaVersion, 2);
  assert.deepEqual(
    manifest.products.map((p) => p.id),
    ["workspace", "api-studio", "knowledge", "control-center"],
  );
  const entries = [manifest.setup, manifest.notices, ...manifest.products.map((p) => p.portable)];
  const names = [...entries.map((item) => item.name), "release-manifest.json"];
  assert.equal(new Set(names).size, 7);
  assert.ok(names.every((name) => typeof name === "string" && /^[a-zA-Z0-9_.-]+$/.test(name)));
  assert.deepEqual((await readdir(root)).sort(), [...names].sort());
  const artifactDigests = {};
  for (const name of names) artifactDigests[name] = await fileDigest(path.join(root, name));
  for (const entry of entries) assert.equal(artifactDigests[entry.name], entry.sha256, "Candidate bytes changed");
  if (revalidation) assert.deepEqual(artifactDigests, revalidation.assetDigests, "Revalidated bytes changed");
  return { sourceSha, fixtureSha: revalidation?.fixtureSha ?? sourceSha, artifactDigests, ...(diagnostic ?? {}) };
}
export async function installedFixtureIdentity() {
  const root = process.env.DEVBOX_USER_FLOW_INSTALL_ROOT;
  assert.ok(root, "Installed fixture identity required");
  const owner = JSON.parse(
    (await readFile(path.join(path.dirname(root), "user-flow-owner.json"), "utf8")).replace(/^\uFEFF/, ""),
  );
  assert.equal(owner.root, root);
  const diagnostic = await diagnosticIdentity();
  const revalidation = await revalidationIdentity();
  assert.equal(owner.sourceSha, diagnostic?.sourceSha ?? revalidation?.sourceSha ?? process.env.GITHUB_SHA);
  assert.match(owner.installationKey, /^[a-f0-9]{64}$/);
  return owner.installationKey;
}
export async function writeUserFlowResults(name, results, evidenceRoot = "product-foundation-evidence/user-flows") {
  assert.match(name, /^[a-z0-9-]+$/);
  assert.ok(Array.isArray(results) && results.length, "Empty user-flow result set");
  await mkdir(evidenceRoot, { recursive: true });
  const output = path.join(evidenceRoot, `${name}.json`);
  const normalized = [];
  const installationKey = await installedFixtureIdentity();
  const diagnostic = await diagnosticIdentity();
  for (const result of results) {
    if (diagnostic) {
      assert.equal(result.sourceSha, diagnostic.sourceSha);
      assert.equal(result.fixtureSha, diagnostic.sourceSha);
    }
    const screenshots = [];
    for (const screenshot of result.screenshotPaths ?? []) {
      const relative = path.relative(path.resolve(evidenceRoot), path.resolve(screenshot));
      assert.ok(
        relative && !relative.startsWith("..") && !path.isAbsolute(relative),
        "Screenshot must be inside user-flow evidence",
      );
      await fileDigest(path.resolve(screenshot));
      screenshots.push(relative.split(path.sep).join("/"));
    }
    normalized.push({ ...result, ...(diagnostic ?? {}), installationKey, screenshotPaths: screenshots });
  }
  // Refuse to replace first failure evidence with a later lucky retry.
  await writeFile(output, `${JSON.stringify({ schemaVersion: 1, results: normalized }, null, 2)}\n`, { flag: "wx" });
  return output;
}
