import assert from "node:assert/strict";
import { test } from "node:test";
import { mkdtemp, mkdir, writeFile, readFile, rm, symlink } from "node:fs/promises";
import { createHash } from "node:crypto";
import { tmpdir } from "node:os";
import path from "node:path";
import { saveApiFileWhenReady, verifyApiInstallation } from "./windows-api-user-flow-adapter.mjs";

const digest = (bytes) => createHash("sha256").update(bytes).digest("hex");
async function fixture() {
  const scratch = await mkdtemp(path.join(tmpdir(), "devbox-suite-delivery-api-test-"));
  const root = path.join(scratch, "Fixture"),
    assets = path.join(scratch, "assets"),
    sourceSha = "a".repeat(40);
  await mkdir(root);
  await mkdir(assets);
  const image = Buffer.from("owned synthetic executable bytes");
  const member = "generations/g/products/api-studio/devbox-api-studio.exe";
  await mkdir(path.dirname(path.join(root, member)), { recursive: true });
  await writeFile(path.join(root, member), image);
  const asset = async (name) => {
    const bytes = Buffer.from(`asset ${name}`);
    await writeFile(path.join(assets, name), bytes);
    return { name, sha256: digest(bytes) };
  };
  const products = [];
  for (const id of ["workspace", "api-studio", "knowledge", "control-center"])
    products.push({
      id,
      portable: await asset(`${id}.zip`),
      files: [{ name: `devbox-${id}.exe`, sha256: digest(image) }],
    });
  const manifest = {
    schemaVersion: 2,
    sourceSha,
    products,
    setup: await asset("setup.exe"),
    notices: await asset("notices.txt"),
  };
  await writeFile(path.join(assets, "release-manifest.json"), JSON.stringify(manifest));
  await writeFile(
    path.join(root, "devbox-installation.json"),
    JSON.stringify({
      schemaVersion: 1,
      protocolVersion: 1,
      generation: "g",
      members: [{ product: "api-studio", executable: member, sha256: digest(image) }],
    }),
  );
  await writeFile(
    path.join(root, "suite-registration.json"),
    JSON.stringify({ schemaVersion: 1, installationKey: "b".repeat(64) }),
  );
  await writeFile(path.join(root, "suite-payload.json"), JSON.stringify({ sourceSha }));
  return { scratch, root, assets, sourceSha, member };
}
test("installed member uses declared executable digest rather than ZIP digest", async () => {
  const value = await fixture();
  try {
    const verified = await verifyApiInstallation(value.root, value.assets, value.sourceSha);
    assert.equal(verified.installationKey, "b".repeat(64));
    assert.equal(verified.sourceSha, value.sourceSha);
    assert.equal(Object.keys(verified.artifactDigests).length, 7);
    await writeFile(path.join(value.root, value.member), "changed image");
    await assert.rejects(() => verifyApiInstallation(value.root, value.assets, value.sourceSha));
  } finally {
    await rm(value.scratch, { recursive: true });
  }
});
test("installed adapter rejects mismatched source and linked member before launch", async () => {
  const value = await fixture();
  try {
    await writeFile(path.join(value.root, "suite-payload.json"), JSON.stringify({ sourceSha: "c".repeat(40) }));
    await assert.rejects(() => verifyApiInstallation(value.root, value.assets, value.sourceSha));
    await writeFile(path.join(value.root, "suite-payload.json"), JSON.stringify({ sourceSha: value.sourceSha }));
    const image = path.join(value.root, value.member),
      other = path.join(value.scratch, "other.exe");
    await writeFile(other, await readFile(image));
    await rm(image);
    await symlink(other, image);
    await assert.rejects(() => verifyApiInstallation(value.root, value.assets, value.sourceSha));
  } finally {
    await rm(value.scratch, { recursive: true });
  }
});

test("installed adapter resolves validated diagnostic payload source without replacing runner SHA", async () => {
  const value = await fixture();
  const environment = {
    DEVBOX_USER_FLOW_DIAGNOSTIC: "true",
    DEVBOX_USER_FLOW_DIAGNOSTIC_SOURCE: value.sourceSha,
    DEVBOX_USER_FLOW_DIAGNOSTIC_RUN: "123",
    DEVBOX_USER_FLOW_DIAGNOSTIC_RECEIPT: path.join(value.scratch, "receipt.json"),
    GITHUB_EVENT_NAME: "workflow_dispatch",
    GITHUB_WORKFLOW: "Product foundation acceptance",
    GITHUB_SHA: "c".repeat(40),
    GITHUB_RUN_ID: "456",
    GITHUB_REPOSITORY: "fixture/devbox",
  };
  const previous = Object.fromEntries(Object.keys(environment).map((name) => [name, process.env[name]]));
  try {
    Object.assign(process.env, environment);
    await writeFile(
      environment.DEVBOX_USER_FLOW_DIAGNOSTIC_RECEIPT,
      JSON.stringify({
        purpose: "retained-installer-ui-diagnostic-only",
        runnerSourceSha: environment.GITHUB_SHA,
        runnerRunId: "456",
        payloadSourceSha: value.sourceSha,
        payloadRunId: "123",
        repository: "fixture/devbox",
        sourceWorkflow: ".github/workflows/windows-package-candidate.yml",
        assemblySucceeded: true,
        diagnosticOnly: true,
        promotionEvidence: false,
      }),
    );
    const verified = await verifyApiInstallation(value.root, value.assets);
    assert.equal(verified.sourceSha, value.sourceSha);
    assert.equal(verified.runnerSourceSha, environment.GITHUB_SHA);
    assert.equal(verified.diagnosticOnly, true);
    assert.equal(process.env.GITHUB_SHA, environment.GITHUB_SHA);
    await assert.rejects(() => verifyApiInstallation(value.root, value.assets, environment.GITHUB_SHA));
  } finally {
    for (const [name, prior] of Object.entries(previous)) {
      if (prior === undefined) delete process.env[name];
      else process.env[name] = prior;
    }
    await rm(value.scratch, { recursive: true });
  }
});

test("native export observes owned picker readiness then saves exactly once", async () => {
  const events = [];
  let observations = 0;
  const owner = { identity: { Pid: 42 } };
  await saveApiFileWhenReady(owner, "owned-fixture.json", {
    action: async (_owner, action, args) => {
      events.push(action);
      if (action === "Inspect")
        return {
          processId: 42,
          nativeWindowCount: 2,
          nativeWindows:
            ++observations === 1 ? [] : [{ nativeProcessId: 42, visible: true, topLevel: true, className: "#32770" }],
        };
      assert.equal(args.filePath, "owned-fixture.json");
    },
    wait: async () => {},
  });
  assert.deepEqual(events, ["Inspect", "Inspect", "SaveFile"]);
});
test("ambiguous or foreign native picker never receives save input", async () => {
  for (const windows of [
    Array.from({ length: 2 }, () => ({ nativeProcessId: 42, visible: true, topLevel: true, className: "#32770" })),
    [{ nativeProcessId: 43, visible: true, topLevel: true, className: "#32770" }],
  ]) {
    const events = [];
    await assert.rejects(
      saveApiFileWhenReady({ identity: { Pid: 42 } }, "owned.json", {
        action: async (_owner, action) => {
          events.push(action);
          return { processId: 42, nativeWindowCount: 2, nativeWindows: windows };
        },
        wait: async () => {},
        timeoutMs: 0,
      }),
    );
    assert.deepEqual(events, ["Inspect"]);
  }
});
