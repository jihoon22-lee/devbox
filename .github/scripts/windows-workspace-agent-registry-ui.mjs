import { observeWorkspaceFailure, cleanupWorkspaceFixture } from "./windows-workspace-ui-observations.mjs";
import assert from "node:assert/strict";
export const scenarioIds = ["WORK-02", "WORK-03"];
export async function dismissWorkspaceUndo(ui, visible, wait) {
  if (!(await visible())) return;
  await ui.click({ role: "button", name: "알림 닫기" });
  await wait(async () => !(await visible()), "notification dismissed before Agent input");
}
export async function discardAgentTask(ui) {
  await ui.waitForTarget({ role: "button", name: "버리기" });
  await ui.click({ role: "button", name: "버리기" });
  await ui.waitForTarget({ role: "button", name: "버리기 확인" });
  await ui.click({ role: "button", name: "버리기 확인" });
}
export async function reviewAgentWorktree(ui, fixture) {
  await fixture.waitForAgentSourceIdle();
  await ui.waitForTarget({ role: "button", name: "변경 검토" });
  await ui.click({ role: "button", name: "변경 검토" });
  await ui.waitForTarget({ role: "region", name: "Git 실행 승인" });
  await fixture.trustSource();
}
export async function reviewReopenedSource(fixture, root, worktreeId) {
  await fixture.selectRoot(root);
  assert.equal((await fixture.context()).worktreeId, worktreeId);
  await fixture.trustSource();
}
export async function run(context) {
  const { ui, sourceSha, fixtureSha, artifactDigests, workspaceFixture: fixture } = context;
  const record = (id, status, assertions = [], screenshotPaths = [], failureCode = null) => ({
    id,
    status,
    sourceSha,
    fixtureSha,
    artifactDigests,
    evidenceKind: "packaged-ui",
    assertions,
    screenshotPaths,
    failureCode,
  });
  if (
    ![
      "prepareAgent",
      "configureAgent",
      "registry",
      "context",
      "wait",
      "trustSource",
      "attemptAgentReview",
      "crashAndReopen",
    ].every((key) => typeof fixture?.[key] === "function")
  )
    return scenarioIds.map((id) =>
      record(
        id,
        "NOT_RUN",
        ["Owned isolated WSL Git repository and actual Agent UI adapters required"],
        [],
        "workspace-owned-agent-fixture-required",
      ),
    );
  const results = [],
    screenshots = [];
  try {
    await fixture.prepareAgent();
    const baseContext = await fixture.context(),
      initial = await fixture.registry();
    const ownTrees = (registry) => registry.worktrees.filter((tree) => tree.projectId === baseContext.projectId);
    assert.equal(ownTrees(initial).length, 1, "Only the owned base may be registered before task creation");
    await ui.click({ role: "button", name: "에이전트" });
    await ui.waitForTarget({ role: "textbox", name: "제목" });
    await ui.fill({ role: "textbox", name: "제목" }, "owned registry reconciliation");
    await fixture.configureAgent();
    await ui.click({ role: "button", name: "작업 만들기" });
    await fixture.wait(
      async () =>
        ownTrees(await fixture.registry()).length === 2 &&
        (await fixture.context()).worktreeId !== baseContext.worktreeId,
      "new task registered and selected",
    );
    const selected = await fixture.context();
    await reviewAgentWorktree(ui, fixture);
    assert.equal((await fixture.context()).worktreeId, selected.worktreeId);
    await fixture.waitForText({ role: "textbox", name: "커밋 메시지" });
    screenshots.push(await ui.screenshot("workspace-agent-new-worktree"));
    // The actual Source buffer causes the real native context operation to stop.
    await ui.fill({ role: "textbox", name: "커밋 메시지" }, "owned Source draft");
    const cause = await fixture.attemptAgentReview();
    assert.match(cause, /Source 초안/);
    assert.deepEqual(await fixture.context(), selected);
    await ui.click({ role: "button", name: "소스" });
    assert.equal(await ui.text({ role: "textbox", name: "커밋 메시지" }), "owned Source draft");
    screenshots.push(await ui.screenshot("workspace-source-context-guard"));
    results.push(
      record(
        "WORK-02",
        "PASS",
        [
          "Actual Agent review rejected specifically due to dirty Source commit draft",
          "Native context and live draft remain unchanged",
        ],
        [...screenshots],
      ),
    );
    await ui.fill({ role: "textbox", name: "커밋 메시지" }, "");
    await ui.click({ role: "button", name: "에이전트" });
    await discardAgentTask(ui);
    await fixture.wait(
      async () => ownTrees(await fixture.registry()).length === 1,
      "owned task cleanup reflected in registry",
    );
    const cleaned = await fixture.registry();
    assert.deepEqual(cleaned.worktrees.map((tree) => tree.id).sort(), initial.worktrees.map((tree) => tree.id).sort());
    assert.equal((await fixture.context()).worktreeId, baseContext.worktreeId);
    await fixture.crashAndReopen();
    assert.deepEqual(
      (await fixture.registry()).worktrees.map((tree) => tree.id).sort(),
      initial.worktrees.map((tree) => tree.id).sort(),
    );
    const retainedBase = cleaned.worktrees.find((tree) => tree.id === baseContext.worktreeId);
    assert.ok(retainedBase, "Owned base remains registered");
    await reviewReopenedSource(fixture, retainedBase.binding.root, baseContext.worktreeId);
    screenshots.push(await ui.screenshot("workspace-agent-cleanup"));
    results.push(
      record(
        "WORK-03",
        "PASS",
        [
          "Actual Agent task published and selected its previously absent worktree without manual refresh",
          "Source review resolves the new native context immediately",
          "Cleanup removes only its task worktree; base and unrelated registered Windows root remain",
          "Actual process restart preserves the cleaned registry; explicit owned base selection restores Source context",
        ],
        [...screenshots],
      ),
    );
  } catch (error) {
    const failure = await observeWorkspaceFailure(ui, "WORK-02", error);
    let operations;
    if (fixture.agentFailureOperations) {
      try {
        operations = await fixture.agentFailureOperations();
      } catch {
        operations = { unavailable: true };
      }
    }
    screenshots.push(...failure.screenshotPaths);
    for (const id of scenarioIds)
      if (!results.some((result) => result.id === id))
        results.push({
          ...record(
            id,
            "FAIL",
            ["Owned WSL Agent UI scenario did not complete"],
            screenshots,
            "workspace-agent-registry-ui-failed",
          ),
          error: failure.error,
          ...(operations === undefined ? {} : { agentFailureOperations: operations }),
        });
  } finally {
    await cleanupWorkspaceFixture(fixture, results);
  }
  return results;
}
