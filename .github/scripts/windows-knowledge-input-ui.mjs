import { withOwnedNativeZoom } from "./windows-user-flow-window.mjs";
import { observeProductLayout, assertProductLayout } from "./browser-product-layout.mjs";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { writeProductInputObservation } from "./windows-suite-layout.mjs";
export async function observeKnowledgeInput({ ui, cdp, fixture, windowOwner }) {
  const notes = await fixture.prepareNotes();
  await fixture.navigate("notes");
  await fixture.openNote(notes.a);
  await fixture.disableAutosave();
  const read = (expression) => cdp.evaluate(expression);
  async function tabTo(predicate) {
    for (let i = 0; i < 160; i++) {
      if (await read(predicate)) return;
      await ui.press("Tab");
    }
    throw new Error("Knowledge control is not reachable by keyboard");
  }
  const renameName = `${path.basename(notes.a)} 이름 변경`;
  await tabTo(`document.activeElement?.getAttribute("aria-label")===${JSON.stringify(renameName)}`);
  let promptOpened = false;
  const unsubscribe = cdp.onEvent("Page.javascriptDialogOpening", () => {
    promptOpened = true;
  });
  await ui.press("Enter");
  await fixture.wait(async () => promptOpened, "actual keyboard rename prompt");
  await cdp.command("Page.handleJavaScriptDialog", {
    accept: true,
    promptText: notes.a.replace(/-A\.md$/, "-input.md"),
  });
  unsubscribe();
  await fixture.waitFor({ role: "dialog", name: "이름 변경 미리보기" });
  await ui.press("Tab");
  await ui.press("Shift+Tab");
  assert.equal(await read('Boolean(document.activeElement?.closest("[role=dialog]"))'), true);
  await ui.press("Escape");
  await fixture.wait(
    async () =>
      await read(
        `!document.querySelector(".rename-dialog")&&document.activeElement?.getAttribute("aria-label")===${JSON.stringify(renameName)}`,
      ),
    "rename focus returns to keyboard opener",
  );
  assert.equal(await readFile(notes.aFile, "utf8"), notes.aOriginal);
  await tabTo('document.activeElement?.getAttribute("aria-label")==="Markdown 본문"');
  await ui.press("Control+a");
  await cdp.command("Input.imeSetComposition", { text: "한글", selectionStart: 2, selectionEnd: 2 });
  await ui.press("Control+s");
  await delay(500);
  assert.equal(
    await readFile(notes.aFile, "utf8"),
    notes.aOriginal,
    "Save shortcut must not persist incomplete IME composition",
  );
  const screenshots = [await ui.screenshot("knowledge-input-ime-unsaved")];
  await cdp.command("Input.imeSetComposition", { text: "", selectionStart: 0, selectionEnd: 0 });
  const committed = "지식 IME 핵심 작업 확인\n";
  await ui.press("Control+a");
  await ui.typeText(committed);
  await ui.press("Control+s");
  await fixture.wait(async () => (await readFile(notes.aFile, "utf8")) === committed, "keyboard committed note saved");
  await withOwnedNativeZoom(windowOwner, cdp, async () => {
    assert.equal(await read('Boolean(document.activeElement?.closest(".cm-editor"))'), true);
    assertProductLayout(await cdp.evaluate(`(${observeProductLayout.toString()})()`), { editor: true });
    screenshots.push(await ui.screenshot("knowledge-input-scale-save"));
  });
  await ui.press("Control+a");
  await ui.typeText(notes.aOriginal);
  await ui.press("Control+s");
  await fixture.wait(
    async () => (await readFile(notes.aFile, "utf8")) === notes.aOriginal,
    "synthetic note baseline restored",
  );
  await writeProductInputObservation({
    product: "knowledge",
    checks: { keyboard: true, modalFocusReturn: true, ime: true, scale: true, taskComplete: true },
    assertions: [
      "Keyboard rename preview traps Tab and returns focus to its opener on Escape",
      "Save shortcut during actual IME composition preserves original disk bytes",
      "Completed Korean text saves through real keyboard input; enlarged renderer retains editor focus",
      "Owned synthetic baseline is restored through explicit save",
    ],
    screenshotPaths: screenshots,
  });
}
