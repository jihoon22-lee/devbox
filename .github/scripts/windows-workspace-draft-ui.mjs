import {
  navigateWorkspaceFiles,
  observeWorkspaceFailure,
  cleanupWorkspaceFixture,
} from "./windows-workspace-ui-observations.mjs";
import assert from "node:assert/strict";
import { mkdtemp, writeFile, readFile } from "node:fs/promises";
import path from "node:path";
export const scenarioIds = ["WORK-01"];
export async function assertWorkspaceEditorText(cdp, expected) {
  // Chromium AX adds a presentation newline for CodeMirror's empty <br> line.
  // Compare the actual rendered lines, preserving whitespace and blank lines;
  // the scenario separately checks exact native recovery content and file bytes.
  const editors = await cdp.evaluate(
    `Array.from(document.querySelectorAll('.workspace-feature-files .cm-content[role="textbox"]')).filter(editor=>editor.getClientRects().length>0).map(editor=>Array.from(editor.children).filter(line=>line.classList.contains('cm-line')).map(line=>line.textContent))`,
  );
  assert.deepEqual(editors, [expected.split("\n")], "Rendered owned editor lines must match the exact draft");
}
export async function waitForWorkspaceEditorText(cdp, wait, expected) {
  await wait(async () => {
    await assertWorkspaceEditorText(cdp, expected);
    return true;
  }, "exact owned editor draft rendered");
  await assertWorkspaceEditorText(cdp, expected);
}
export async function reviewRecoveryWriterFailure({ ui, fixture, readBytes, before, after }) {
  assert.deepEqual(await readBytes(), before, "Failed journal must not save the file implicitly");
  await ui.closeOwnedWindow();
  await ui.waitForTarget({ role: "button", name: "파일 저장 후 종료" });
  await ui.click({ role: "button", name: "파일 저장 후 종료" });
  await fixture.waitForText({ role: "alert", name: "", scope: { role: "dialog", name: "Workspace 종료 검토" } });
  assert.deepEqual(await readBytes(), after, "Explicit save commits file bytes before recovery flush fails");
}
export async function run(context) {
  const { ui, cdp, fixtureRoot, sourceSha, fixtureSha, artifactDigests, workspaceFixture: fixture } = context;
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
      "selectWindows",
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
      await fixture.selectWindows();
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
    await ui.waitForTarget({ role: "button", name: "종료 취소" });
    await ui.click({ role: "button", name: "종료 취소" });
    await waitForWorkspaceEditorText(cdp, fixture.wait, "");
    await journal("");
    assert.deepEqual(await readFile(file), original);
    screenshots.push(await ui.screenshot("workspace-close-cancel"));
    await fixture.crashAndReopen();
    await fixture.selectWindows();
    await ui.click({ role: "button", name: "파일" });
    await fixture.waitForText({ role: "button", name: "복구 (1)" });
    await ui.click({ role: "button", name: "복구 (1)" });
    await waitForWorkspaceEditorText(cdp, fixture.wait, "");
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
    await fixture.selectWindows();
    await ui.click({ role: "button", name: "파일" });
    await fixture.waitForText({ role: "button", name: "복구 (1)" });
    await ui.click({ role: "button", name: "복구 (1)" });
    await waitForWorkspaceEditorText(cdp, fixture.wait, "원본\n");
    await ui.closeOwnedWindow();
    await ui.waitForTarget({ role: "button", name: "파일 저장 후 종료" });
    await ui.click({ role: "button", name: "파일 저장 후 종료" });
    await fixture.reopenAfterClose();
    await fixture.selectWindows();
    assert.deepEqual(await readFile(file), Buffer.concat([Buffer.from([0xef, 0xbb, 0xbf]), Buffer.from("원본\n")]));
    assert.equal((await fixture.recovery()).entries.length, 0);
    await open();
    await ui.fill({ role: "textbox", name: "" }, "폐기할 초안");
    await journal("폐기할 초안");
    const saved = await readFile(file);
    await ui.closeOwnedWindow();
    await ui.waitForTarget({ role: "button", name: "파일 변경 폐기 후 종료" });
    await ui.click({ role: "button", name: "파일 변경 폐기 후 종료" });
    await fixture.reopenAfterClose();
    await fixture.selectWindows();
    assert.deepEqual(await readFile(file), saved);
    assert.equal((await fixture.recovery()).entries.length, 0);
    // Real owned filesystem failure: writer/normal-close must retain the live draft.
    await open();
    await ui.fill({ role: "textbox", name: "" }, "실패 전 초안");
    await journal("실패 전 초안");
    await fixture.failRecoveryWriter();
    await ui.fill({ role: "textbox", name: "" }, "실패 후 보존할 초안");
    await reviewRecoveryWriterFailure({
      ui,
      fixture,
      readBytes: () => readFile(file),
      before: saved,
      after: Buffer.concat([Buffer.from([0xef, 0xbb, 0xbf]), Buffer.from("실패 후 보존할 초안")]),
    });
    screenshots.push(await ui.screenshot("workspace-writer-failure-retained"));
    await ui.waitForTarget({ role: "button", name: "종료 취소" });
    await ui.click({ role: "button", name: "종료 취소" });
    await waitForWorkspaceEditorText(cdp, fixture.wait, "실패 후 보존할 초안");
    await fixture.restoreRecoveryWriter();
    await ui.closeOwnedWindow();
    await ui.waitForTarget({ role: "button", name: "종료" });
    await ui.click({ role: "button", name: "종료" });
    await fixture.reopenAfterClose();
    await fixture.selectWindows();
    assert.equal((await fixture.recovery()).entries.length, 0);
    results.push(
      record("WORK-01", "PASS", [
        "Normal native close cancel/save/discard preserve drafts or exact original bytes as reviewed",
        "Crash restores empty and encoding/BOM/line-ending metadata into dirty buffers; journal removed only after save/discard",
        "Recovery writer failure blocks close; only explicit save updates file bytes, and repairing the journal permits clean close",
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
