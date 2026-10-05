import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

// The plan job independently authenticates artifact provenance and unchanged
// inputs. Consumers bind that immutable receipt to this exact workflow/run.
export function validateRevalidationIdentity(proof, env = process.env) {
  assert.equal(env.GITHUB_EVENT_NAME, "workflow_dispatch");
  assert.equal(env.GITHUB_WORKFLOW, "Windows candidate revalidation");
  assert.equal(env.GITHUB_REF, "refs/heads/main");
  assert.equal(
    env.GITHUB_WORKFLOW_REF,
    `${proof.repository}/.github/workflows/windows-candidate-revalidation.yml@refs/heads/main`,
  );
  for (const name of Object.keys(env))
    if (name.startsWith("DEVBOX_USER_FLOW_DIAGNOSTIC")) assert.equal(env[name], undefined);
  assert.equal(proof.schemaVersion, 1);
  assert.equal(proof.purpose, "candidate-fixture-revalidation");
  assert.equal(proof.repository, env.GITHUB_REPOSITORY);
  assert.match(proof.sourceSha ?? "", /^[a-f0-9]{40}$/);
  assert.match(proof.fixtureSha ?? "", /^[a-f0-9]{40}$/);
  assert.equal(proof.fixtureSha, env.GITHUB_SHA);
  for (const key of ["buildRunId", "revalidationRunId", "artifactId"])
    assert.ok(Number.isSafeInteger(proof[key]) && proof[key] > 0);
  assert.equal(String(proof.revalidationRunId), env.GITHUB_RUN_ID);
  assert.notEqual(proof.buildRunId, proof.revalidationRunId);
  assert.match(proof.artifactDigest ?? "", /^sha256:[a-f0-9]{64}$/);
  assert.equal(proof.artifactName, `candidate-assembly-${proof.buildRunId}`);
  assert.equal(Object.keys(proof.assetDigests ?? {}).length, 7);
  for (const [name, digest] of Object.entries(proof.assetDigests)) {
    assert.match(name, /^[a-zA-Z0-9_.-]+$/);
    assert.match(digest, /^[a-f0-9]{64}$/);
  }
  assert.notEqual(proof.diagnosticOnly, true);
  assert.notEqual(proof.promotionEvidence, false);
  return proof;
}

export async function revalidationIdentity(env = process.env) {
  if (!env.DEVBOX_USER_FLOW_REVALIDATION_PROOF) return null;
  return validateRevalidationIdentity(
    JSON.parse((await readFile(env.DEVBOX_USER_FLOW_REVALIDATION_PROOF, "utf8")).replace(/^\uFEFF/u, "")),
    env,
  );
}

export function revalidationIdentitySync(env = process.env) {
  if (!env.DEVBOX_USER_FLOW_REVALIDATION_PROOF) return null;
  return validateRevalidationIdentity(
    JSON.parse(readFileSync(env.DEVBOX_USER_FLOW_REVALIDATION_PROOF, "utf8").replace(/^\uFEFF/u, "")),
    env,
  );
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const proof = await revalidationIdentity();
  assert.ok(proof, "Explicit revalidation proof required");
  process.stdout.write(JSON.stringify({ sourceSha: proof.sourceSha, buildRunId: proof.buildRunId }));
}
