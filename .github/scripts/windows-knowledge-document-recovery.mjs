import assert from "node:assert/strict";
import { readFile, unlink } from "node:fs/promises";
import { scenarios, editor } from "./windows-knowledge-flow-shared.mjs";
export const scenarioIds = ["DOC-01", "DOC-02", "DOC-03"];
export async function run(context) {
  return scenarios(context, [
    [
      "DOC-01",
      async ({ ui, fixture, assertions, screenshots }) => {
        const notes = await fixture.prepareNotes();
        await fixture.navigate("notes");
        await fixture.openNote(notes.a);
        await fixture.disableAutosave();
        await ui.fill(editor, "A의 새 내용\n");
        await ui.press("Control+z");
        assert.equal(await ui.text(editor), notes.aOriginal);
        await ui.press("Control+y");
        assert.equal(await ui.text(editor), "A의 새 내용\n");
        await ui.press("Control+s");
        await fixture.wait(async () => (await readFile(notes.aFile, "utf8")) === "A의 새 내용\n", "A saved");
        await fixture.openNote(notes.b);
        await ui.press("Control+z");
        assert.equal(await ui.text(editor), notes.bOriginal);
        await fixture.openNote(notes.a);
        await ui.press("Control+z");
        assert.equal(await ui.text(editor), "A의 새 내용\n");
        assert.equal(await readFile(notes.bFile, "utf8"), notes.bOriginal);
        screenshots.push(await ui.screenshot("DOC-01-isolated-history"));
        assertions.push(
          "Undo/Redo remains local to one document; switch/reopen cannot transplant another document's bytes; disk B preserved",
        );
      },
    ],
    [
      "DOC-02",
      async ({ ui, fixture, assertions, screenshots }) => {
        const notes = await fixture.prepareNotes();
        await fixture.navigate("notes");
        await fixture.openNote(notes.a);
        await fixture.disableAutosave();
        await ui.fill(editor, "");
        await fixture.waitJournal(notes.a, "");
        await ui.closeOwnedWindow();
        await ui.click({ role: "button", name: "종료 취소" });
        assert.equal(await ui.text(editor), "");
        assert.equal(await readFile(notes.aFile, "utf8"), notes.aOriginal);
        const switching = fixture.openNote(notes.b);
        await fixture.confirmDialog();
        await switching;
        await ui.fill(editor, "B의 보존할 복구본\n");
        await fixture.waitJournal(notes.b, "B의 보존할 복구본\n");
        await fixture.crashAndReopen();
        await fixture.navigate("notes");
        await fixture.waitFor({ role: "button", name: `${notes.a} 열어서 확인` });
        await ui.click({ role: "button", name: `${notes.a} 열어서 확인` });
        assert.equal(await ui.text(editor), "");
        await ui.closeOwnedWindow();
        await ui.click({ role: "button", name: "저장하지 않고 종료(복구본 유지)" });
        await fixture.reopenAfterClose();
        await fixture.navigate("notes");
        await fixture.waitFor({ role: "button", name: `${notes.a} 열어서 확인` });
        assert.equal(await readFile(notes.aFile, "utf8"), notes.aOriginal);
        await ui.click({ role: "button", name: `${notes.a} 열어서 확인` });
        await ui.closeOwnedWindow();
        await ui.click({ role: "button", name: "복구본 영구 삭제…" });
        assert.ok((await fixture.journal()).entries.some((e) => e.path === notes.a));
        await ui.click({ role: "button", name: "영구 삭제하고 종료" });
        await fixture.reopenAfterClose();
        await fixture.navigate("notes");
        const remaining = await fixture.journal();
        assert.ok(!remaining.entries.some((e) => e.path === notes.a));
        assert.ok(remaining.entries.some((e) => e.path === notes.b && e.content === "B의 보존할 복구본\n"));
        assert.equal(await readFile(notes.bFile, "utf8"), notes.bOriginal);
        screenshots.push(await ui.screenshot("DOC-02-empty-recovery"));
        assertions.push(
          "Empty and second-document journals survive switch/crash/keep quit; cancel preserves editor and disk; separate permanent confirmation removes only current journal",
        );
      },
    ],
    [
      "DOC-03",
      async ({ ui, fixture, assertions, screenshots }) => {
        const notes = await fixture.prepareNotes();
        await fixture.navigate("notes");
        await fixture.openNote(notes.a);
        await fixture.disableAutosave();
        await ui.fill(editor, "삭제 후 복구할 합성 본문\n");
        await fixture.waitJournal(notes.a, "삭제 후 복구할 합성 본문\n");
        await fixture.crashAndReopen();
        await unlink(notes.aFile);
        await fixture.navigate("notes");
        await fixture.waitFor({ role: "button", name: `${notes.a} 열어서 확인` });
        await ui.click({ role: "button", name: `${notes.a} 열어서 확인` });
        await fixture.waitBody("삭제 후 복구할 합성 본문");
        assert.equal(
          (await fixture.journal()).entries.find((e) => e.path === notes.a).content,
          "삭제 후 복구할 합성 본문\n",
        );
        const recreate = ui.click({ role: "button", name: "삭제된 노트 재생성" });
        await fixture.confirmDialog();
        await recreate;
        await fixture.wait(
          async () => (await readFile(notes.aFile, "utf8")) === "삭제 후 복구할 합성 본문\n",
          "missing note recreated",
        );
        await fixture.openNote(notes.b);
        await fixture.disableAutosave();
        await ui.fill(editor, "offline 합성 복구본\n");
        await fixture.waitJournal(notes.b, "offline 합성 복구본\n");
        await fixture.offlineAndReopen();
        await fixture.waitBody("로컬 복구본");
        await ui.click({ role: "button", name: "로컬 복구본 확인" });
        await fixture.waitBody("offline 합성 복구본");
        assert.equal(await fixture.offlineContents(notes.b), notes.bOriginal);
        screenshots.push(await ui.screenshot("DOC-03-offline-preview"));
        assertions.push(
          "Missing note keeps preview and journal until explicit recreation; offline startup exposes bounded read-only local recovery",
        );
        await fixture.restoreOnline();
      },
    ],
  ]);
}
