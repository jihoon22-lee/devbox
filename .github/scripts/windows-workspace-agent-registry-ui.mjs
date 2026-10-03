import assert from "node:assert/strict";
export const scenarioIds = ["WORK-03"];
export async function run(context) {
  const { ui, sourceSha, fixtureSha, artifactDigests, workspaceFixture } = context;
  const result = {
    id: "WORK-03",
    status: "NOT_RUN",
    sourceSha,
    fixtureSha,
    artifactDigests,
    evidenceKind: "packaged-ui",
    assertions: [],
    screenshotPaths: [],
    failureCode: "workspace-owned-agent-fixture-required",
  };
  if (
    !workspaceFixture?.prepareAgent ||
    !workspaceFixture?.registry ||
    !workspaceFixture?.context ||
    !workspaceFixture?.waitForAgent
  ) {
    result.assertions = ["An owned isolated WSL repository with only its base registered is required"];
    return [result];
  }
  try {
    await workspaceFixture.prepareAgent();
    const initial = await workspaceFixture.registry();
    assert.equal(initial.worktrees.length, 1);
    await ui.click({ role: "button", name: "Agents" });
    await ui.fill({ role: "textbox", name: "제목" }, "owned registry reconciliation");
    await ui.click({ role: "button", name: "작업 만들기" });
    await workspaceFixture.waitForAgent();
    const registered = await workspaceFixture.registry();
    assert.equal(registered.worktrees.length, 2);
    const selected = await workspaceFixture.context();
    assert.notEqual(selected.worktreeId, initial.worktrees[0].id);
    await ui.click({ role: "button", name: "변경 검토" });
    assert.equal((await workspaceFixture.context()).worktreeId, selected.worktreeId);
    result.screenshotPaths.push(await ui.screenshot("workspace-agent-new-worktree"));
    await ui.click({ role: "button", name: "Agents" });
    await ui.click({ role: "button", name: "버리기" });
    await ui.click({ role: "button", name: "버리기 확인" });
    await workspaceFixture.waitForAgent();
    const cleaned = await workspaceFixture.registry();
    assert.deepEqual(
      cleaned.worktrees.map((tree) => tree.id),
      initial.worktrees.map((tree) => tree.id),
    );
    result.screenshotPaths.push(await ui.screenshot("workspace-agent-cleanup"));
    result.status = "PASS";
    result.failureCode = null;
    result.assertions = [
      "Creating an Agent task published and selected its new registered worktree without manual refresh",
      "Owned cleanup removed only the new worktree and preserved the base registry entry",
    ];
  } catch {
    result.status = "FAIL";
    result.failureCode = "workspace-agent-registry-ui-failed";
    result.assertions = ["Owned Agent registry UI flow failed"];
  } finally {
    await workspaceFixture.cleanup?.();
  }
  return [result];
}
