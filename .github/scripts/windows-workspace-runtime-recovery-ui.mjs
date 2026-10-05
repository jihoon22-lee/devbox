import { boundedFailure } from "./user-flow-failure-evidence.mjs";
import { observeWorkspaceFailure } from "./windows-workspace-ui-observations.mjs";
import assert from "node:assert/strict";
export const scenarioIds = ["RUNTIME-01", "RUNTIME-02", "LSP-01", "DEPS-01"];
export async function run(context) {
  assert.ok(!context.dependenciesOnly || context.diagnosticOnly === true, "Explicit retained diagnostic required");
  const { ui, sourceSha, fixtureSha, artifactDigests, workspaceFixture: fixture } = context;
  const results = [];
  for (const [id, method] of [
    ["RUNTIME-01", "runtimeLostReply"],
    ["RUNTIME-02", "terminalLifecycle"],
    ["LSP-01", "managedLspLifecycle"],
    ["DEPS-01", "dependencyRefresh"],
  ]) {
    if (context.dependenciesOnly && id !== "DEPS-01") continue;
    const result = {
      id,
      status: "NOT_RUN",
      sourceSha,
      fixtureSha,
      artifactDigests,
      evidenceKind: "packaged-ui",
      assertions: [],
      screenshotPaths: [],
      failureCode: `${id.toLowerCase()}-owned-adapter-required`,
    };
    try {
      if (typeof fixture?.[method] !== "function") {
        result.assertions = [
          "Required actual UI scenario adapter is unavailable; prerequisite cannot be treated as acceptance",
        ];
      } else {
        const proof = await fixture[method](ui);
        if (proof?.notRun) {
          result.assertions = [proof.notRun];
        } else {
          assert.ok(Array.isArray(proof?.assertions) && proof.assertions.length >= 3);
          assert.ok(proof.screenshots?.length > 0);
          result.status = "PASS";
          result.failureCode = null;
          result.assertions = proof.assertions;
          result.screenshotPaths = proof.screenshots;
        }
      }
    } catch (error) {
      Object.assign(result, await observeWorkspaceFailure(ui, id, error));
      result.status = "FAIL";
      result.failureCode = `${id.toLowerCase()}-ui-failed`;
      result.assertions = ["Owned packaged UI scenario failed before all required assertions completed"];
      if (id === "LSP-01" && fixture.closeFailedLsp) {
        try {
          await fixture.closeFailedLsp();
        } catch (cleanupError) {
          result.cleanupError = boundedFailure(cleanupError);
        }
      }
    }
    results.push(result);
  }
  return results;
}
