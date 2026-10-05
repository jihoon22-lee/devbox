import assert from "node:assert/strict";
import { test } from "node:test";
import { mkdtemp, mkdir, writeFile, readFile, rm, symlink } from "node:fs/promises";
import { createHash } from "node:crypto";
import { tmpdir } from "node:os";
import path from "node:path";
import {
  chooseApiFileWhenReady,
  markApiCleanupFailure,
  saveApiFileWhenReady,
  verifyApiInstallation,
} from "./windows-api-user-flow-adapter.mjs";

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
      if (action === "InspectFilePicker")
        return {
          pickerCount: 1,
          fieldCount: 1,
          editCount: 0,
          confirmCount: 1,
          filenameReady: true,
          confirmationReady: true,
        };
      assert.equal(args.filePath, "owned-fixture.json");
    },
    wait: async () => {},
  });
  assert.deepEqual(events, ["Inspect", "Inspect", "InspectFilePicker", "SaveFile"]);
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

test("native executable picker readiness precedes its one ChooseFile input", async () => {
  const events = [];
  let count = 0;
  await chooseApiFileWhenReady({ identity: { Pid: 42 } }, "owned.exe", {
    action: async (_owner, action, args) => {
      events.push(action);
      if (action === "Inspect")
        return {
          processId: 42,
          nativeWindowCount: 2,
          nativeWindows:
            ++count === 1 ? [] : [{ nativeProcessId: 42, visible: true, topLevel: true, className: "#32770" }],
        };
      if (action === "InspectFilePicker")
        return {
          pickerCount: 1,
          fieldCount: 1,
          editCount: 0,
          confirmCount: 1,
          filenameReady: true,
          confirmationReady: true,
        };
      assert.equal(args.filePath, "owned.exe");
    },
    wait: async () => {},
  });
  assert.deepEqual(events, ["Inspect", "Inspect", "InspectFilePicker", "ChooseFile"]);
});
test("cleanup rejects a passing journey but never overwrites an earlier failure", () => {
  const error = { name: "Error", message: "first failure" };
  const failed = { status: "FAIL", failureCode: "first-error", error };
  markApiCleanupFailure(failed, "handoff-owned-cleanup-failed");
  assert.equal(failed.failureCode, "first-error");
  assert.equal(failed.error, error);
  assert.equal(failed.cleanupFailureCode, "handoff-owned-cleanup-failed");
  const passed = { status: "PASS", failureCode: null };
  markApiCleanupFailure(passed, "api-owned-process-cleanup-failed");
  assert.equal(passed.status, "FAIL");
  assert.equal(passed.failureCode, "api-owned-process-cleanup-failed");
});

test("owned window alone never permits input before its filename and confirmation controls are ready", async () => {
  const events = [];
  let probes = 0;
  await chooseApiFileWhenReady({ identity: { Pid: 42 } }, "owned.exe", {
    action: async (_owner, action) => {
      events.push(action);
      if (action === "Inspect")
        return {
          processId: 42,
          nativeWindowCount: 2,
          nativeWindows: [{ nativeProcessId: 42, visible: true, topLevel: true, className: "#32770" }],
        };
      if (action === "InspectFilePicker")
        return ++probes === 1
          ? {
              pickerCount: 1,
              fieldCount: 0,
              editCount: 0,
              confirmCount: 0,
              filenameReady: false,
              confirmationReady: false,
            }
          : {
              pickerCount: 1,
              fieldCount: 1,
              editCount: 1,
              confirmCount: 1,
              filenameReady: true,
              confirmationReady: true,
            };
    },
    wait: async () => {},
  });
  assert.deepEqual(events, ["Inspect", "InspectFilePicker", "Inspect", "InspectFilePicker", "ChooseFile"]);
});
test("control-readiness timeout records only fixed counts and never performs input", async () => {
  const events = [];
  await assert.rejects(
    chooseApiFileWhenReady({ identity: { Pid: 42 } }, "private-owned-path", {
      action: async (_owner, action) => {
        events.push(action);
        if (action === "Inspect")
          return {
            processId: 42,
            nativeWindowCount: 2,
            nativeWindows: [{ nativeProcessId: 42, visible: true, topLevel: true, className: "#32770" }],
          };
        return {
          pickerCount: 1,
          fieldCount: 0,
          editCount: 0,
          confirmCount: 1,
          filenameReady: false,
          confirmationReady: true,
          name: "private-document",
        };
      },
      timeoutMs: 0,
    }),
    (error) => {
      assert.match(error.message, /"fieldCount":0/);
      assert.ok(!error.message.includes("private"));
      return true;
    },
  );
  assert.deepEqual(events, ["Inspect", "InspectFilePicker"]);
});

test("API context binds split fixture source only through the current revalidation proof", async () => {
  const { requireApiContext } = await import("./windows-api-user-flow-actions.mjs");
  const scratch = await mkdtemp(path.join(tmpdir(), "devbox-suite-delivery-api-proof-"));
  const sourceSha = "a".repeat(40),
    fixtureSha = "b".repeat(40);
  const artifactDigests = Object.fromEntries(Array.from({ length: 7 }, (_, i) => [`asset${i}`, "c".repeat(64)]));
  const root = path.join(scratch, "Fixture");
  const context = {
    root,
    fixtureRoot: root,
    sourceSha,
    fixtureSha,
    artifactDigests,
    installationKey: "d".repeat(64),
    namespace: `com.devbox.v08.apistudio.i${"d".repeat(64)}`,
    ui: Object.fromEntries(
      ["click", "fill", "press", "screenshot", "confirmDialog", "closeOwnedWindow"].map((name) => [name, () => {}]),
    ),
    cdp: { evaluate() {} },
    nativeCall() {},
  };
  const env = {
    GITHUB_EVENT_NAME: "workflow_dispatch",
    GITHUB_WORKFLOW: "Windows candidate revalidation",
    GITHUB_REF: "refs/heads/main",
    GITHUB_WORKFLOW_REF: "fixture/devbox/.github/workflows/windows-candidate-revalidation.yml@refs/heads/main",
    GITHUB_REPOSITORY: "fixture/devbox",
    GITHUB_SHA: fixtureSha,
    GITHUB_RUN_ID: "456",
    DEVBOX_USER_FLOW_REVALIDATION_PROOF: path.join(scratch, "proof.json"),
  };
  const previous = Object.fromEntries(Object.keys(env).map((key) => [key, process.env[key]]));
  try {
    delete process.env.DEVBOX_USER_FLOW_REVALIDATION_PROOF;
    assert.throws(() => requireApiContext(context));
    requireApiContext({ ...context, fixtureSha: sourceSha });
    await writeFile(
      env.DEVBOX_USER_FLOW_REVALIDATION_PROOF,
      JSON.stringify({
        schemaVersion: 1,
        purpose: "candidate-fixture-revalidation",
        repository: env.GITHUB_REPOSITORY,
        sourceSha,
        fixtureSha,
        buildRunId: 123,
        revalidationRunId: 456,
        artifactId: 789,
        artifactName: "candidate-assembly-123",
        artifactDigest: `sha256:${"e".repeat(64)}`,
        assetDigests: artifactDigests,
      }),
    );
    Object.assign(process.env, env);
    requireApiContext(context);
    assert.throws(() => requireApiContext({ ...context, sourceSha: fixtureSha }));
    assert.throws(() => requireApiContext({ ...context, fixtureSha: sourceSha }));
    assert.throws(() =>
      requireApiContext({ ...context, artifactDigests: { ...artifactDigests, asset0: "f".repeat(64) } }),
    );
    process.env.GITHUB_RUN_ID = "457";
    assert.throws(() => requireApiContext(context));
  } finally {
    for (const [key, value] of Object.entries(previous)) {
      if (value === undefined) delete process.env[key];
      else process.env[key] = value;
    }
    await rm(scratch, { recursive: true, force: true });
  }
});
