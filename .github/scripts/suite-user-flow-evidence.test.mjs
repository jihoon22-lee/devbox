import assert from "node:assert/strict";
import test from "node:test";
import { summarizeEvidence, requireCompleteEvidence } from "./suite-user-flow-evidence.mjs";
const sha = "a".repeat(40),
  digest = "b".repeat(64);
const matrix = [
  {
    id: "INSTALL-01",
    ownerWorkItem: "R07",
    products: ["control-center"],
    evidenceKind: "packaged-ui",
    module: "windows-suite-user-flow.mjs",
  },
];
const result = {
  id: "INSTALL-01",
  status: "PASS",
  sourceSha: sha,
  fixtureSha: sha,
  artifactDigests: { setup: digest },
  evidenceKind: "packaged-ui",
  assertions: ["committed"],
  screenshotPaths: ["evidence/install.png"],
  failureCode: null,
};
const expected = {
  expectedSource: sha,
  expectedFixture: sha,
  expectedDigests: { setup: digest },
  requiredMatrix: matrix,
};
test("matching package evidence is required; missing, stale, mock, failed results cannot promote", () => {
  assert.equal(summarizeEvidence(matrix, [result], expected).ready, true);
  for (const results of [
    [],
    [result, result],
    [{ ...result, status: "NOT_RUN" }],
    [{ ...result, sourceSha: "c".repeat(40) }],
    [{ ...result, fixtureSha: "c".repeat(40) }],
    [{ ...result, artifactDigests: {} }],
    [{ ...result, evidenceKind: "native-boundary" }],
    [{ ...result, assertions: [] }],
    [{ ...result, screenshotPaths: [] }],
  ]) {
    assert.equal(summarizeEvidence(matrix, results, expected).ready, false);
    assert.throws(() => requireCompleteEvidence({ ...expected, results }));
  }
});
test("subset ready is scoped and cannot stand for complete release", () => {
  const other = { ...matrix[0], id: "DOC-01", ownerWorkItem: "R01" };
  const subset = summarizeEvidence([...matrix, other], [result], { ...expected, ownerWorkItem: "R07" });
  assert.equal(subset.ready, true);
  assert.equal(subset.scope, "work-item:R07");
  assert.equal(summarizeEvidence([...matrix, other], [result], expected).ready, false);
});
