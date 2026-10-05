import assert from "node:assert/strict";
import { test } from "node:test";
import { mkdtemp, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { validateRevalidationIdentity } from "./revalidation-identity.mjs";
import { fileDigest, packagedIdentity } from "./suite-user-flow-results.mjs";

const source = "a".repeat(40),
  fixture = "b".repeat(40);
const env = {
  GITHUB_EVENT_NAME: "workflow_dispatch",
  GITHUB_WORKFLOW: "Windows candidate revalidation",
  GITHUB_REPOSITORY: "fixture/devbox",
  GITHUB_SHA: fixture,
  GITHUB_RUN_ID: "456",
  GITHUB_REF: "refs/heads/main",
  GITHUB_WORKFLOW_REF: "fixture/devbox/.github/workflows/windows-candidate-revalidation.yml@refs/heads/main",
};
const proof = (assetDigests) => ({
  schemaVersion: 1,
  purpose: "candidate-fixture-revalidation",
  repository: env.GITHUB_REPOSITORY,
  sourceSha: source,
  fixtureSha: fixture,
  buildRunId: 123,
  revalidationRunId: 456,
  artifactId: 789,
  artifactName: "candidate-assembly-123",
  artifactDigest: `sha256:${"c".repeat(64)}`,
  assetDigests,
});

test("revalidation identity binds real build and fixture separately to the exact trusted execution", () => {
  const valid = proof(Object.fromEntries(Array.from({ length: 7 }, (_, i) => [`asset${i}`, "d".repeat(64)])));
  assert.equal(validateRevalidationIdentity(valid, env).sourceSha, source);
  for (const patch of [
    { GITHUB_WORKFLOW: "Product foundation acceptance" },
    { GITHUB_EVENT_NAME: "pull_request" },
    { GITHUB_SHA: source },
    { GITHUB_RUN_ID: "457" },
    { GITHUB_REPOSITORY: "foreign/devbox" },
    { DEVBOX_USER_FLOW_DIAGNOSTIC: "true" },
  ])
    assert.throws(() => validateRevalidationIdentity(valid, { ...env, ...patch }));
  for (const patch of [
    { purpose: "retained-installer-ui-diagnostic-only" },
    { diagnosticOnly: true },
    { promotionEvidence: false },
    { sourceSha: "short" },
    { artifactDigest: "" },
    { artifactName: "candidate-assembly-124" },
    { revalidationRunId: "456" },
    { buildRunId: 0 },
    { assetDigests: {} },
  ])
    assert.throws(() => validateRevalidationIdentity({ ...valid, ...patch }, env));
});

test("revalidation reads unchanged seven assets without relabeling their build SHA and rejects altered proof", async () => {
  const directory = await mkdtemp(path.join(tmpdir(), "devbox-revalidation-"));
  const receipt = path.join(directory, "proof.json"),
    assets = path.join(directory, "assets");
  const { mkdir } = await import("node:fs/promises");
  await mkdir(assets);
  const applied = { ...env, DEVBOX_USER_FLOW_ASSETS: assets, DEVBOX_USER_FLOW_REVALIDATION_PROOF: receipt };
  const previous = Object.fromEntries(Object.keys(applied).map((key) => [key, process.env[key]]));
  try {
    const products = ["workspace", "api-studio", "knowledge", "control-center"];
    const entries = [];
    for (const name of ["setup.exe", "notices.md", ...products.map((id) => `${id}.zip`)]) {
      await writeFile(path.join(assets, name), `synthetic ${name}`);
      entries.push({ name, sha256: await fileDigest(path.join(assets, name)) });
    }
    await writeFile(
      path.join(assets, "release-manifest.json"),
      JSON.stringify({
        schemaVersion: 2,
        sourceSha: source,
        setup: entries[0],
        notices: entries[1],
        products: products.map((id, i) => ({ id, portable: entries[i + 2] })),
      }),
    );
    const digests = Object.fromEntries(entries.map((entry) => [entry.name, entry.sha256]));
    digests["release-manifest.json"] = await fileDigest(path.join(assets, "release-manifest.json"));
    const valid = proof(digests);
    await writeFile(receipt, JSON.stringify(valid));
    Object.assign(process.env, applied);
    const actual = await packagedIdentity();
    assert.equal(actual.sourceSha, source);
    assert.equal(actual.fixtureSha, fixture);
    assert.deepEqual(actual.artifactDigests, digests);
    await assert.rejects(packagedIdentity(assets, fixture));
    valid.assetDigests["setup.exe"] = "e".repeat(64);
    await writeFile(receipt, JSON.stringify(valid));
    await assert.rejects(packagedIdentity(), /Revalidated bytes changed/);
    delete process.env.DEVBOX_USER_FLOW_REVALIDATION_PROOF;
    await assert.rejects(packagedIdentity(), /Expected values/);
  } finally {
    for (const [key, value] of Object.entries(previous)) {
      if (value === undefined) delete process.env[key];
      else process.env[key] = value;
    }
    await rm(directory, { recursive: true, force: true });
  }
});
