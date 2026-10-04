import assert from "node:assert/strict";
import test from "node:test";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { boundedFailure, preserveUserFlowFailure } from "./user-flow-failure-evidence.mjs";
test("failure evidence omits assertion dumps and quoted credentials", () => {
  const error = new Error('failed token=synthetic-secret "private-body"\nactual: private-response');
  error.stack += "\nactual: private-response";
  const result = JSON.stringify(boundedFailure(error));
  for (const value of ["synthetic-secret", "private-body", "private-response"])
    assert.equal(result.includes(value), false);
  assert.ok(result.includes("failed"));
});
test("captures before preserving first failure and refuses replacement", async () => {
  const root = await mkdtemp(path.join(tmpdir(), "devbox-failure-"));
  try {
    let captured = 0;
    const ui = {
      screenshot: async () => {
        captured++;
        return "/synthetic.png";
      },
    };
    await preserveUserFlowFailure("workspace", new Error("Original failure"), { ui, evidenceRoot: root });
    assert.equal(captured, 1);
    const saved = JSON.parse(await readFile(path.join(root, "workspace-user-flow-failure.json")));
    assert.equal(saved.status, "FAIL");
    assert.equal(saved.screenshotPath, "/synthetic.png");
    await assert.rejects(preserveUserFlowFailure("workspace", new Error("Later failure"), { ui, evidenceRoot: root }), {
      code: "EEXIST",
    });
    assert.equal(captured, 1, "First screenshot must not be replaced");
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
