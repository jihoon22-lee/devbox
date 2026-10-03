import assert from "node:assert/strict";
import test from "node:test";
import {
  beforeAgentProductPreparation,
  afterAgentProductPreparation,
  afterAgentCommit,
  observeAgentUpdateQuiesce,
  until,
} from "./windows-suite-agent-user-flows.mjs";
for (const hook of [
  beforeAgentProductPreparation,
  afterAgentProductPreparation,
  afterAgentCommit,
  observeAgentUpdateQuiesce,
])
  test(`${hook.name} rejects missing actual installed ownership before side effects`, async () => {
    await assert.rejects(hook({}), /win32|true|realpath|sourceSha|Cannot|undefined/);
  });
test("bounded Agent observations do not accept a missing condition", async () => {
  await assert.rejects(
    until(() => false, "real condition missing", 1),
    /real condition missing/,
  );
});
test("Agent close/update receipt aggregation rejects a different installation or scope", async () => {
  const { validateAgentOwnershipReceipts } = await import("./windows-suite-agent-user-flows.mjs");
  const identity = {
      sourceSha: "a".repeat(40),
      fixtureSha: "a".repeat(40),
      artifactDigests: { image: "b".repeat(64) },
    },
    context = { identity, installationKey: "c".repeat(64), manifest: { installationId: "owned" } },
    common = {
      ...identity,
      installationKey: context.installationKey,
      screenshotPaths: ["product-foundation-evidence/user-flows/screenshots/agent/proof.png"],
    },
    portable = { ...common, portableLocalOwnerExited: true, installedAgentUnchanged: true },
    update = {
      ...common,
      updateQuiesced: true,
      previousInstallationId: "owned",
      proofScope: "actual-candidate-update-review-and-health-gate",
    };
  assert.throws(() =>
    validateAgentOwnershipReceipts(context, { ...portable, installationKey: "d".repeat(64) }, update),
  );
  assert.throws(() => validateAgentOwnershipReceipts(context, portable, { ...update, proofScope: "native-update" }));
  assert.throws(() =>
    validateAgentOwnershipReceipts(context, { ...portable, screenshotPaths: ["outside.png"] }, update),
  );
});
