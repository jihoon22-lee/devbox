import assert from "node:assert/strict";
export const scenarioIds = ["WORK-02", "WORK-03"];
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
    await ui.click({ role: "button", name: "변경 검토" });
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
    await ui.click({ role: "button", name: "버리기" });
    await ui.click({ role: "button", name: "버리기 확인" });
    await fixture.wait(
      async () => ownTrees(await fixture.registry()).length === 1,
      "owned task cleanup reflected in registry",
    );
    const cleaned = await fixture.registry();
    assert.deepEqual(cleaned.worktrees.map((tree) => tree.id).sort(), initial.worktrees.map((tree) => tree.id).sort());
    assert.equal((await fixture.context()).worktreeId, baseContext.worktreeId);
    await fixture.crashAndReopen();
    await fixture.wait(
      async () => (await fixture.context())?.worktreeId === baseContext.worktreeId,
      "cleaned selection restored after actual process restart",
    );
    assert.deepEqual(
      (await fixture.registry()).worktrees.map((tree) => tree.id).sort(),
      initial.worktrees.map((tree) => tree.id).sort(),
    );
    await ui.click({ role: "button", name: "소스" });
    await fixture.waitForText({ role: "textbox", name: "커밋 메시지" });
    screenshots.push(await ui.screenshot("workspace-agent-cleanup"));
    results.push(
      record(
        "WORK-03",
        "PASS",
        [
          "Actual Agent task published and selected its previously absent worktree without manual refresh",
          "Source review resolves the new native context immediately",
          "Cleanup removes only its task worktree; base and unrelated registered Windows root remain",
          "Actual process restart restores cleaned base selection and registry; Source accepts the restored context",
        ],
        [...screenshots],
      ),
    );
  } catch {
    for (const id of scenarioIds)
      if (!results.some((result) => result.id === id))
        results.push(
          record(
            id,
            "FAIL",
            ["Owned WSL Agent UI scenario did not complete"],
            screenshots,
            "workspace-agent-registry-ui-failed",
          ),
        );
  } finally {
    await fixture.cleanup();
  }
  return results;
}
