import assert from "node:assert/strict";
import test from "node:test";
import {
  selectRegisteredWorkspaceRoot,
  waitForSelectedWorkspaceRoot,
} from "./windows-workspace-registry-observations.mjs";
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
  assert.deepEqual(actions[0][1].scope, { role: "region", name: project.name });
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

test("selection observation rejects a stale same-kind context until the exact owned revision attaches", async () => {
  const tree = { id: "owned-tree", projectId: "owned-project", revision: 3, binding: { root: "/tmp/owned-project" } };
  let selected = { projectId: "wrong-project", worktreeId: "wrong-tree", revision: 3, target: { kind: "wsl" } };
  const results = [];
  await waitForSelectedWorkspaceRoot(
    async () => selected,
    async () => ({ worktrees: [tree] }),
    async (check) => {
      results.push(await check());
      selected = { ...selected, projectId: tree.projectId, worktreeId: tree.id, revision: 2 };
      results.push(await check());
      selected = { ...selected, revision: tree.revision };
      results.push(await check());
    },
    tree.binding.root,
  );
  assert.deepEqual(results, [false, false, true]);
});

test("sibling worktrees use the exact native root group instead of ambiguous project region", async () => {
  let clicked;
  await selectRegisteredWorkspaceRoot(
    {
      waitForTarget: async () => {},
      click: async (target) => {
        clicked = target;
      },
    },
    async () => ({
      projects: [{ id: "p", name: "same project" }],
      worktrees: [
        { projectId: "p", binding: { root: "/owned/base" } },
        { projectId: "p", binding: { root: "/owned/task" } },
      ],
    }),
    async (check) => assert.equal(await check(), true),
    "/owned/base",
  );
  assert.deepEqual(clicked.scope, { role: "group", name: "/owned/base" });
});
