import assert from "node:assert/strict";
export const scenarioIds = ["RUNTIME-01", "RUNTIME-02", "LSP-01", "DEPS-01"];
export async function run(context) {
  const { ui, sourceSha, fixtureSha, artifactDigests, workspaceFixture: fixture } = context;
  const results = [];
  for (const [id, method] of [
    ["RUNTIME-01", "runtimeLostReply"],
    ["RUNTIME-02", "terminalLifecycle"],
    ["LSP-01", "managedLspLifecycle"],
    ["DEPS-01", "dependencyRefresh"],
  ]) {
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
    } catch {
      result.status = "FAIL";
      result.failureCode = `${id.toLowerCase()}-ui-failed`;
      result.assertions = ["Owned packaged UI scenario failed before all required assertions completed"];
    }
    results.push(result);
  }
  return results;
}
