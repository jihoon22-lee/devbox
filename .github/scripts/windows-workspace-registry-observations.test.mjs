import assert from "node:assert/strict";
import test from "node:test";
import { selectRegisteredWorkspaceRoot } from "./windows-workspace-registry-observations.mjs";
test("automatic registration waits exact root persistence and native project card before one selection", async () => {
  const actions = [],
    root = "C:\\owned\\new-root";
  const registry = async () => ({
    projects: [{ id: "own", name: "Native project name" }],
    worktrees: [{ projectId: "own", binding: { root } }],
  });
  const project = await selectRegisteredWorkspaceRoot(
    {
      waitForTarget: async (target) => {
        actions.push(["ready", target]);
      },
      click: async (target) => {
        actions.push(["click", target]);
      },
    },
    registry,
    async (check) => assert.equal(await check(), true),
    root,
  );
  assert.equal(project.name, "Native project name");
  assert.deepEqual(
    actions.map(([event]) => event),
    ["ready", "click"],
  );
  assert.equal(actions[0][1].scope.name, project.name);
});
test("missing native card readiness rejects before any selection", async () => {
  let clicks = 0;
  const failure = new Error("card not ready");
  await assert.rejects(
    selectRegisteredWorkspaceRoot(
      {
        waitForTarget: async () => {
          throw failure;
        },
        click: async () => clicks++,
      },
      async () => ({
        projects: [{ id: "p", name: "owned" }],
        worktrees: [{ projectId: "p", binding: { root: "/home/owned" } }],
      }),
      async (check) => assert.equal(await check(), true),
      "/home/owned",
    ),
    (error) => error === failure,
  );
  assert.equal(clicks, 0);
});
