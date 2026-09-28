import assert from "node:assert/strict";
import test from "node:test";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { exerciseAgentRuntime } from "./windows-agent-runtime.mjs";

test("service preparation failure retains its exact checkpoint before any lifetime checks", async () => {
  const directory = mkdtempSync(path.join(tmpdir(), "devbox-agent-stage-"));
  const reports = [];
  const failure = new Error("synthetic service failure");
  let calls = 0;
  try {
    await assert.rejects(
      exerciseAgentRuntime({
        directory,
        workspace: {
          cdp: {
            evaluate: async () => {
              calls++;
              if (calls === 4) throw failure;
              return { operation: { outcome: { state: "succeeded" } }, value: { previewId: "fixture", context: {} } };
            },
          },
        },
        report: (state) => reports.push({ ...state }),
      }),
      (error) => error === failure,
    );
    assert.equal(calls, 4);
    assert.equal(reports.at(-1)?.stage, "prepare-service");
    assert.deepEqual(
      reports.map((state) => state.stage),
      ["prepare-project", "prepare-service"],
    );
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});
