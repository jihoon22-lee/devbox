import assert from "node:assert/strict";
import test from "node:test";
import { validateRetainedCommittedInstall } from "./verify-retained-committed-install.mjs";
function fixture() {
  const root = "/tmp/runner/devbox-suite-delivery-" + "a".repeat(32) + "/Suite UI Fixture";
  const installationKey = "b".repeat(64);
  return {
    root,
    runnerTemp: "/tmp/runner",
    staging: "/repo/candidate/delivery",
    sourceSha: "c".repeat(40),
    installationKey,
    owner: { schemaVersion: 1, root, staging: "/repo/candidate/delivery", sourceSha: "c".repeat(40), installationKey },
    registration: { installationKey },
    manifest: { installationId: "fixture", generation: "g1" },
    activation: { schemaVersion: 1, phase: "committed", installationId: "fixture", generation: "g1" },
  };
}
test("admits only the same owned committed retained fixture", () => {
  assert.equal(validateRetainedCommittedInstall(fixture()).phase, "committed");
});
test("rejects precommit phases and mismatched installation or generation", () => {
  for (const override of [
    { phase: "health" },
    { phase: "import" },
    { phase: "recover" },
    { installationId: "foreign" },
    { generation: "g2" },
  ]) {
    const input = fixture();
    Object.assign(input.activation, override);
    assert.throws(() => validateRetainedCommittedInstall(input));
  }
});
test("rejects foreign source, registration, staging, and unowned fixture paths", () => {
  for (const change of [
    (input) => (input.owner.sourceSha = "d".repeat(40)),
    (input) => (input.registration.installationKey = "e".repeat(64)),
    (input) => (input.owner.staging = "/foreign"),
    (input) => (input.root = "/tmp/foreign/Suite UI Fixture"),
  ]) {
    const input = fixture();
    change(input);
    assert.throws(() => validateRetainedCommittedInstall(input));
  }
});
