import assert from "node:assert/strict";
import test from "node:test";
import { observeKnowledgeStartupFailure, observeKnowledgeStartupTargets } from "./knowledge-startup-observation.mjs";

test("startup diagnosis preserves the original failure when every observer fails", async () => {
  const original = new Error("original startup deadline");
  const evidence = {};
  const fail = async () => {
    throw new Error("private observer detail");
  };
  await assert.rejects(
    observeKnowledgeStartupFailure(original, { child: { exitCode: null, signalCode: null } }, 9222, evidence, {
      host: fail,
      waits: fail,
      targets: fail,
    }),
    (error) => error === original,
  );
  assert.equal(evidence.startupFailure.childAlive, true);
  assert.deepEqual(evidence.startupFailure.nativeObserver, { state: "probe_failed" });
  assert.ok(!JSON.stringify(evidence).includes("private"));
});

test("target snapshot retains blank/title and local URL categories without private values", async () => {
  const result = await observeKnowledgeStartupTargets(9222, async () => ({
    status: 200,
    ok: true,
    text: async () =>
      JSON.stringify([
        { type: "page", title: "about:blank", url: "about:blank" },
        { type: "page", title: "Devbox Knowledge", url: "http://tauri.localhost/index.html?secret=private" },
        { type: "page", title: "private note", url: "file:///private/note" },
      ]),
  }));
  assert.deepEqual(result.pages, [
    { title: "about:blank", url: "about:blank" },
    { title: "Devbox Knowledge", url: "http://tauri.localhost/index.html" },
    { title: "other", url: "other" },
  ]);
  assert.ok(!JSON.stringify(result).includes("private"));
});

test("retained lifecycle mode separates verified payload from fixture source and rejects mixed modes", async () => {
  const { knowledgeLifecycleMode } = await import("./knowledge-startup-observation.mjs");
  const env = { GITHUB_SHA: "a".repeat(40), DEVBOX_SUITE_ARTIFACT_SOURCE: "b".repeat(40) };
  assert.deepEqual(knowledgeLifecycleMode([], env).evidence, { source: env.GITHUB_SHA });
  assert.deepEqual(knowledgeLifecycleMode(["--retained-diagnostic"], env).evidence, {
    source: env.DEVBOX_SUITE_ARTIFACT_SOURCE,
    payloadSource: env.DEVBOX_SUITE_ARTIFACT_SOURCE,
    fixtureSource: env.GITHUB_SHA,
    diagnosticOnly: true,
    promotionEvidence: false,
  });
  assert.throws(() => knowledgeLifecycleMode(["--retained-diagnostic", "--packaged-ui"], env));
  assert.throws(() => knowledgeLifecycleMode(["--unknown"], env));
  assert.throws(() => knowledgeLifecycleMode(["--retained-diagnostic"], { GITHUB_SHA: env.GITHUB_SHA }));
  assert.equal(env.GITHUB_SHA, "a".repeat(40));
});
