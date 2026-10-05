import assert from "node:assert/strict";
import test from "node:test";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { exerciseAgentRuntime, selectAgentRuntimeProject } from "./windows-agent-runtime.mjs";

test("service preparation failure retains its exact checkpoint before any lifetime checks", async () => {
  const directory = mkdtempSync(path.join(tmpdir(), "devbox-agent-stage-"));
  const reports = [];
  const failure = new Error("synthetic service failure");
  let calls = 0;
  let selectedThroughUi = false;
  try {
    await assert.rejects(
      exerciseAgentRuntime({
        directory,
        workspace: {
          cdp: {
            evaluate: async () => {
              calls++;
              if (calls === 3) {
                assert.equal(
                  selectedThroughUi,
                  true,
                  "Native context selection bypassed renderer description publication",
                );
                throw failure;
              }
              return { operation: { outcome: { state: "succeeded" } }, value: { previewId: "fixture", context: {} } };
            },
          },
        },
        selectWorkspaceProject: async (_workspace, root) => {
          assert.equal(root, directory);
          selectedThroughUi = true;
        },
        report: (state) => reports.push({ ...state }),
      }),
      (error) => error === failure,
    );
    assert.equal(calls, 3);
    assert.equal(reports.at(-1)?.stage, "prepare-service");
    assert.deepEqual(
      reports.map((state) => state.stage),
      ["prepare-project", "prepare-service"],
    );
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("agent fixture selects the exact worktree and waits for the renderer publication without replay", async () => {
  const actions = [];
  let reads = 0;
  const directory = "C:\\owned\\agent-root";
  await selectAgentRuntimeProject(
    {
      ui: {
        click: async (target) => actions.push(target),
        waitForTarget: async (target) => {
          if (target.name === "프로젝트 선택") assert.deepEqual(target.scope, { role: "group", name: directory });
        },
      },
      cdp: {
        evaluate: async (expression) => {
          assert.ok(expression.includes(JSON.stringify(directory)));
          assert.ok(expression.includes("현재 선택한 작업 폴더:"));
          return ++reads === 2;
        },
      },
    },
    directory,
  );
  assert.equal(reads, 2);
  assert.deepEqual(
    actions.map((target) => target.name),
    ["개요", "목록 새로 고침", "프로젝트 선택"],
  );
});
