import assert from "node:assert/strict";
import { test } from "node:test";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { writeInstallerFailure, reportInstallerResults } from "./windows-suite-installer-evidence.mjs";

test("pre-registration failure survives missing installed-identity reporting without inventing a key", async () => {
  const directory = await mkdtemp(path.join(tmpdir(), "devbox-installer-evidence-"));
  const error = new Error("Interactive installer timed out: installer welcome");
  const results = [{ id: "INSTALL-01", status: "FAIL", assertions: [error.message] }];
  try {
    await writeInstallerFailure(
      {
        stage: "installer-welcome-ready",
        error,
        identity: { sourceSha: "a".repeat(40), diagnosticOnly: true },
        results,
        observation: { kind: "owned-nsis-uia", window: { name: "NSIS owned fixture" } },
      },
      directory,
    );
    const evidence = JSON.parse(await readFile(path.join(directory, "interactive-installer-failure.json"), "utf8"));
    assert.equal(evidence.error.message, error.message);
    assert.equal(evidence.stage, "installer-welcome-ready");
    assert.equal(evidence.status, "FAIL");
    assert.equal(Object.hasOwn(evidence, "installationKey"), false);
    await assert.rejects(
      reportInstallerResults(results, async () => {
        throw new Error("ENOENT user-flow-owner.json");
      }),
      /installer welcome.*ENOENT user-flow-owner/u,
    );
    await assert.rejects(
      writeInstallerFailure({ stage: "later", error, identity: {}, results, observation: null }, directory),
      /EEXIST/u,
    );
  } finally {
    await rm(directory, { recursive: true });
  }
});
test("success reporting failure remains failure and is never converted to acceptance", async () => {
  const error = new Error("report unavailable");
  await assert.rejects(
    reportInstallerResults([{ status: "PASS" }], async () => {
      throw error;
    }),
    (cause) => cause === error,
  );
});
