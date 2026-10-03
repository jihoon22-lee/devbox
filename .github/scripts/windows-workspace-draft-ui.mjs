import assert from "node:assert/strict";
import { mkdtemp, writeFile, readFile, rm } from "node:fs/promises";
import path from "node:path";
export const scenarioIds = ["WORK-01", "WORK-02"];
export async function run(context) {
  const { ui, fixtureRoot, sourceSha, fixtureSha, artifactDigests, workspaceFixture } = context;
  const record = (id, status, assertions, screenshotPaths, failureCode = null) => ({
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
  if (!workspaceFixture?.prepare || !workspaceFixture?.crashAndReopen || !workspaceFixture?.context) {
    return scenarioIds.map((id) =>
      record(
        id,
        "NOT_RUN",
        ["Owned Workspace namespace, crash/reopen and context observation adapters required"],
        [],
        "workspace-owned-fixture-required",
      ),
    );
  }
  const root = await mkdtemp(path.join(fixtureRoot, "workspace-draft-"));
  const screenshots = [];
  try {
    const file = path.join(root, "한글.txt");
    await writeFile(file, "원본\r\n", "utf8");
    await workspaceFixture.prepare(root);
    await ui.click({ role: "button", name: "Files" });
    await ui.fill({ role: "textbox", name: "열 파일 경로" }, file);
    await ui.click({ role: "button", name: "파일 열기" });
    await ui.fill({ role: "textbox", name: "" }, "");
    await workspaceFixture.waitForText({ role: "status", name: "복구 내용을 기록했습니다." });
    await ui.closeOwnedWindow();
    await ui.click({ role: "button", name: "종료 취소" });
    assert.equal(await ui.text({ role: "textbox", name: "" }), "");
    assert.equal(await readFile(file, "utf8"), "원본\r\n");
    screenshots.push(await ui.screenshot("workspace-close-cancel"));
    await workspaceFixture.crashAndReopen();
    await ui.click({ role: "button", name: "복구 (1)" });
    assert.equal(await ui.text({ role: "textbox", name: "" }), "");
    screenshots.push(await ui.screenshot("workspace-empty-recovery"));
    const first = record(
      "WORK-01",
      "PASS",
      [
        "Normal native close cancel preserved empty dirty text and original file",
        "Owned crash and reopen restored the confirmed empty recovery snapshot",
      ],
      [...screenshots],
    );
    // Prepared fixture supplies a second owned registered worktree, never user state.
    await ui.click({ role: "button", name: "Source" });
    await ui.fill({ role: "textbox", name: "커밋 메시지" }, "owned Source draft");
    const before = await workspaceFixture.context();
    await ui.click({ role: "button", name: "Agents" });
    await ui.click({ role: "button", name: "검토" });
    assert.deepEqual(await workspaceFixture.context(), before);
    await ui.click({ role: "button", name: "Source" });
    assert.equal(await ui.text({ role: "textbox", name: "커밋 메시지" }), "owned Source draft");
    return [
      first,
      record(
        "WORK-02",
        "PASS",
        ["Agent review kept the native context and actual Source commit draft"],
        [await ui.screenshot("workspace-source-context-guard")],
      ),
    ];
  } catch {
    return scenarioIds.map((id) =>
      record(
        id,
        "FAIL",
        ["Owned Workspace draft UI scenario did not complete"],
        screenshots,
        "workspace-draft-ui-failed",
      ),
    );
  } finally {
    await workspaceFixture.cleanup?.();
    await rm(root, { recursive: true, force: true });
  }
}
