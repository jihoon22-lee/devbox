import {
  navigateWorkspaceFiles,
  observeWorkspaceFailure,
  cleanupWorkspaceFixture,
} from "./windows-workspace-ui-observations.mjs";
import assert from "node:assert/strict";
import { mkdtemp, writeFile, readFile } from "node:fs/promises";
import path from "node:path";
export const scenarioIds = ["WORK-01"];
export async function run(context) {
  const { ui, fixtureRoot, sourceSha, fixtureSha, artifactDigests, workspaceFixture: fixture } = context;
  const results = [],
    screenshots = [];
  const record = (id, status, assertions, failureCode = null) => ({
    id,
    status,
    sourceSha,
    fixtureSha,
    artifactDigests,
    evidenceKind: "packaged-ui",
    assertions,
    screenshotPaths: [...screenshots],
    failureCode,
  });
  if (
    ![
      "prepare",
      "crashAndReopen",
      "context",
      "recovery",
      "failRecoveryWriter",
      "reopenAfterClose",
      "observeInput",
    ].every((key) => typeof fixture?.[key] === "function")
  )
    return scenarioIds.map((id) =>
      record(
        id,
        "NOT_RUN",
        ["Owned native lifecycle, recovery failure and Agent review adapters required"],
        "workspace-owned-fixture-required",
      ),
    );
  const root = await mkdtemp(path.join(fixtureRoot, "workspace-draft-"));
  try {
    const file = path.join(root, "한글.txt"),
      original = Buffer.from("원본\r\n");
    await writeFile(file, original);
    await fixture.prepare(root);
    const open = async () => {
      await navigateWorkspaceFiles(ui);
      await ui.fill({ role: "textbox", name: "열 파일 경로" }, file);
      await ui.click({ role: "button", name: "파일 열기" });
      await fixture.waitForText({ role: "textbox", name: "" });
    };
    const journal = async (text) =>
      fixture.wait(
        async () => (await fixture.recovery()).entries.some((entry) => entry.path === file && entry.content === text),
        "dirty journal persisted",
      );
    await open();
    await fixture.observeInput(path.basename(file));
    await ui.fill({ role: "textbox", name: "" }, "");
    await journal("");
    await ui.closeOwnedWindow();
    await ui.click({ role: "button", name: "종료 취소" });
    assert.equal(await ui.text({ role: "textbox", name: "" }), "");
    assert.deepEqual(await readFile(file), original);
    screenshots.push(await ui.screenshot("workspace-close-cancel"));
    await fixture.crashAndReopen();
    await fixture.waitForText({ role: "button", name: "복구 (1)" });
    await ui.click({ role: "button", name: "복구 (1)" });
    assert.equal(await ui.text({ role: "textbox", name: "" }), "");
    assert.deepEqual(await readFile(file), original);
    await journal("");
    // Metadata-only dirty recovery must preserve BOM and line ending across a crash.
    await ui.fill({ role: "textbox", name: "" }, "원본\n");
    await fixture.selectOption({ role: "combobox", name: "저장 인코딩" }, 1);
    await fixture.selectOption({ role: "combobox", name: "줄바꿈 변환" }, 0);
    await fixture.wait(
      async () =>
        (await fixture.recovery()).entries.some(
          (entry) => entry.path === file && entry.encoding?.bom === true && entry.line_ending === "lf",
        ),
      "encoding-only journal metadata",
    );
    await fixture.crashAndReopen();
    await ui.click({ role: "button", name: "복구 (1)" });
    await ui.closeOwnedWindow();
    await ui.click({ role: "button", name: "파일 저장 후 종료" });
    await fixture.reopenAfterClose();
    assert.deepEqual(await readFile(file), Buffer.concat([Buffer.from([0xef, 0xbb, 0xbf]), Buffer.from("원본\n")]));
    assert.equal((await fixture.recovery()).entries.length, 0);
    await open();
    await ui.fill({ role: "textbox", name: "" }, "폐기할 초안");
    await journal("폐기할 초안");
    const saved = await readFile(file);
    await ui.closeOwnedWindow();
    await ui.click({ role: "button", name: "파일 변경 폐기 후 종료" });
    await fixture.reopenAfterClose();
    assert.deepEqual(await readFile(file), saved);
    assert.equal((await fixture.recovery()).entries.length, 0);
    // Real owned filesystem failure: writer/normal-close must retain the live draft.
    await open();
    await ui.fill({ role: "textbox", name: "" }, "실패 전 초안");
    await journal("실패 전 초안");
    await fixture.failRecoveryWriter();
    await ui.fill({ role: "textbox", name: "" }, "실패 후 보존할 초안");
    await ui.closeOwnedWindow();
    await ui.click({ role: "button", name: "파일 저장 후 종료" });
    await fixture.waitForText({ role: "alert", name: "" });
    assert.deepEqual(await readFile(file), saved);
    screenshots.push(await ui.screenshot("workspace-writer-failure-retained"));
    await ui.click({ role: "button", name: "종료 취소" });
    assert.equal(await ui.text({ role: "textbox", name: "" }), "실패 후 보존할 초안");
    await fixture.restoreRecoveryWriter();
    await ui.closeOwnedWindow();
    await ui.click({ role: "button", name: "파일 변경 폐기 후 종료" });
    await fixture.reopenAfterClose();
    results.push(
      record("WORK-01", "PASS", [
        "Normal native close cancel/save/discard preserve drafts or exact original bytes as reviewed",
        "Crash restores empty and encoding/BOM/line-ending metadata into dirty buffers; journal removed only after save/discard",
        "Owned writer failure blocks close and retains unsaved text without rewriting original",
      ]),
    );
  } catch (error) {
    const failure = await observeWorkspaceFailure(ui, "WORK-01", error);
    screenshots.push(...failure.screenshotPaths);
    for (const id of scenarioIds)
      if (!results.some((result) => result.id === id))
        results.push({
          ...record(id, "FAIL", ["Owned Workspace draft UI scenario did not complete"], "workspace-draft-ui-failed"),
          error: failure.error,
        });
  } finally {
    await cleanupWorkspaceFixture(fixture, results);
  }
  return results;
}
