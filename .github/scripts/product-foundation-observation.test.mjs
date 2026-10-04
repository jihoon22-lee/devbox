import assert from "node:assert/strict";
import test from "node:test";
import { runInNewContext } from "node:vm";
import { knowledgeStepObserver, foundationMode } from "./product-foundation-observation.mjs";
test("serialized marker records pending and rejected stages without values or errors", async () => {
  const marker = { steps: [] };
  const observe = runInNewContext(`(${knowledgeStepObserver.toString()})`)(marker);
  let finish;
  const pending = observe(
    "read-file",
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  assert.equal(marker.steps[0].state, "started");
  finish("private-result");
  assert.equal(await pending, "private-result");
  const failure = new Error("private-error");
  await assert.rejects(
    observe("tracking", async () => {
      throw failure;
    }),
    (error) => error === failure,
  );
  assert.deepEqual(
    marker.steps.map((step) => step.state),
    ["completed", "rejected"],
  );
  assert.equal(JSON.stringify(marker).includes("private"), false);
  assert.ok(marker.steps.every((step) => Number.isFinite(step.elapsedMs)));
});
test("diagnostic mode requires exact payload provenance and cannot mix scopes", () => {
  const env = { GITHUB_SHA: "b".repeat(40), DEVBOX_SUITE_ARTIFACT_SOURCE: "a".repeat(40) };
  const mode = foundationMode(["--knowledge-diagnostic"], env);
  assert.equal(mode.evidence.payloadSource, env.DEVBOX_SUITE_ARTIFACT_SOURCE);
  assert.equal(mode.evidence.fixtureSource, env.GITHUB_SHA);
  assert.equal(mode.evidence.promotionEvidence, false);
  assert.throws(() => foundationMode(["--knowledge-diagnostic"], {}));
  assert.throws(() => foundationMode(["--knowledge-diagnostic", "--smoke-only"], env));
  assert.deepEqual(foundationMode([], env).evidence, { source: env.GITHUB_SHA });
});

test("Knowledge batch observes all fourteen IPC calls with unchanged native deadlines", async () => {
  const { readFileSync } = await import("node:fs");
  const { typedComponentBridge } = await import("./typed-component-fixture.mjs");
  const source = readFileSync(new URL("./windows-product-foundation.mjs", import.meta.url), "utf8");
  // Locate the batch by its observer instead of another product's evaluate.
  const marker = source.indexOf("const marker = window.__devboxKnowledgeProbe");
  const begin = source.lastIndexOf("`(async () => {", marker);
  const end = source.indexOf("})()`);", marker) + 4;
  assert.ok(begin >= 0 && end > begin);
  const expression = source
    .slice(begin + 1, end)
    .replace("${knowledgeStepObserver.toString()}", knowledgeStepObserver.toString())
    .replace("${typedComponentBridge}", typedComponentBridge);
  const seen = [];
  const window = {
    __TAURI_INTERNALS__: {
      invoke: async (method, args) => {
        seen.push(method);
        if (!args) {
          if (method === "get_root") throw new Error("legacy denied");
          return { handshake: { installationId: "owned", sessionId: "owned" } };
        }
        assert.ok(args.request.header.deadlineMs - Date.now() <= 5000);
        return {
          operation: { outcome: { state: "succeeded" } },
          value: method.endsWith("notes") ? "notes-vault" : { rows: [], source: "files" },
        };
      },
    },
  };
  await runInNewContext(expression, { window, crypto: { randomUUID: () => "owned" } });
  assert.equal(seen.length, 14);
  assert.equal(window.__devboxKnowledgeProbe.steps.length, 14);
  assert.equal(window.__devboxKnowledgeProbe.steps.at(-1).step, "set_root");
});

test("full sequence diagnostic preserves product selection and cannot combine modes", () => {
  const env = { GITHUB_SHA: "b".repeat(40), DEVBOX_SUITE_ARTIFACT_SOURCE: "a".repeat(40) };
  const mode = foundationMode(["--product-sequence-diagnostic"], env);
  assert.equal(mode.diagnostic, true);
  assert.equal(mode.knowledgeOnly, false);
  assert.equal(mode.evidence.diagnosticOnly, true);
  assert.equal(mode.evidence.promotionEvidence, false);
  assert.equal(foundationMode(["--knowledge-diagnostic"], env).knowledgeOnly, true);
  assert.throws(() => foundationMode(["--product-sequence-diagnostic", "--knowledge-diagnostic"], env));
  assert.throws(() => foundationMode(["--product-sequence-diagnostic"], {}));
});

test("cleanup projection records exit evidence without exposing process error text", async () => {
  const { cleanupObservation } = await import("./product-foundation-observation.mjs");
  const result = {
    status: 1,
    signal: "SIGTERM",
    error: { code: "ENOENT", message: "private command", path: "private path" },
  };
  const observed = cleanupObservation(result, () => {
    throw Object.assign(new Error("private"), { code: "ESRCH" });
  });
  assert.deepEqual(observed, {
    taskkillAttempted: true,
    status: 1,
    signal: "SIGTERM",
    error: "ENOENT",
    rootPidExists: false,
  });
  assert.equal(cleanupObservation(undefined, () => {}).rootPidExists, true);
  assert.equal(
    cleanupObservation(undefined, () => {
      throw { code: "EPERM" };
    }).rootPidExists,
    true,
  );
  assert.equal(
    cleanupObservation(undefined, () => {
      throw new Error("unknown");
    }).rootPidExists,
    null,
  );
});
