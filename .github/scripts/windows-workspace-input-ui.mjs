import { withOwnedNativeZoom } from "./windows-user-flow-window.mjs";
import { observeProductLayout, assertProductLayout } from "./browser-product-layout.mjs";
import assert from "node:assert/strict";
import { writeProductInputObservation } from "./windows-suite-layout.mjs";
import { observeUntil } from "./windows-suite-ui-context.mjs";
export async function leaveWorkspaceEditorByKeyboard(ui, read) {
  if (await read('Boolean(document.activeElement?.closest(".cm-editor"))')) {
    // CodeMirror permits the next Tab to leave its indentation keymap for
    // two seconds after Escape; send the two native keys consecutively.
    await ui.press("Escape");
    await ui.press("Tab");
  }
}
export async function observeWorkspaceInput({ ui, cdp, fileName, windowOwner }) {
  const read = (expression) => cdp.evaluate(expression);
  let editorFocused = false;
  for (let i = 0; i < 128; i++) {
    if (await read('Boolean(document.activeElement?.closest(".cm-editor"))')) {
      editorFocused = true;
      break;
    }
    await ui.press("Tab");
  }
  assert.ok(editorFocused, "Owned editor must be reachable by keyboard");
  const oldFont = await read('parseFloat(getComputedStyle(document.querySelector(".cm-content")).fontSize)');
  await ui.press("Control+p");
  await observeUntil(
    () => read('document.activeElement?.getAttribute("aria-label")==="파일 검색"'),
    "keyboard Quick Open focus",
  );
  await ui.press("Tab");
  await ui.press("Shift+Tab");
  assert.equal(await read('document.activeElement?.getAttribute("aria-label")'), "파일 검색");
  await cdp.command("Input.imeSetComposition", { text: "한글", selectionStart: 2, selectionEnd: 2 });
  await ui.press("Enter");
  assert.equal(
    await read('Boolean(document.querySelector(".quick-open-dialog"))'),
    true,
    "Composition Enter must not select a file",
  );
  await cdp.command("Input.imeSetComposition", { text: "한글", selectionStart: 2, selectionEnd: 2 });
  await ui.press("Escape");
  assert.equal(
    await read('Boolean(document.querySelector(".quick-open-dialog"))'),
    true,
    "IME must not open a file or close its modal",
  );
  assert.equal(await read('document.activeElement?.getAttribute("aria-label")'), "파일 검색");
  const screenshots = [await ui.screenshot("workspace-input-ime-modal")];
  await cdp.command("Input.imeSetComposition", { text: "", selectionStart: 0, selectionEnd: 0 });
  await ui.press("Escape");
  await observeUntil(
    () => read('!document.querySelector(".quick-open-dialog")&&Boolean(document.activeElement?.closest(".cm-editor"))'),
    "modal return to editor",
  );
  await ui.press("Control+p");
  await observeUntil(
    () => read('document.activeElement?.getAttribute("aria-label")==="파일 검색"'),
    "second keyboard search",
  );
  await ui.press("Control+a");
  await ui.typeText(fileName);
  await observeUntil(
    () => read('document.querySelectorAll(".quick-open-item").length===1'),
    "unique owned file search",
  );
  await ui.press("Enter");
  await observeUntil(
    () => read('!document.querySelector(".quick-open-dialog")&&Boolean(document.querySelector(".cm-content"))'),
    "keyboard file opened",
  );
  // The real product's font control, reached through keyboard focus, enlarges text.
  await leaveWorkspaceEditorByKeyboard(ui, read);
  let found = false;
  for (let i = 0; i < 128; i++) {
    if (await read('document.activeElement?.getAttribute("aria-label")==="편집기 글꼴 크기 확대"')) {
      found = true;
      break;
    }
    await ui.press("Tab");
  }
  assert.ok(found, "Editor font control unavailable through keyboard");
  for (let i = 0; i < 3; i++) await ui.press("Enter");
  await observeUntil(
    async () => (await read('parseFloat(getComputedStyle(document.querySelector(".cm-content")).fontSize)')) > oldFont,
    "actual enlarged editor text",
  );
  await withOwnedNativeZoom(windowOwner(), cdp, async () => {
    assertProductLayout(await cdp.evaluate(`(${observeProductLayout.toString()})()`), { editor: true });
    screenshots.push(await ui.screenshot("workspace-input-enlarged-editor"));
  });
  await writeProductInputObservation({
    product: "workspace",
    checks: { keyboard: true, modalFocusReturn: true, ime: true, scale: true, taskComplete: true },
    assertions: [
      "Keyboard Quick Open traps Tab/Shift+Tab and Escape returns focus to editor",
      "Actual IME composition Enter/Escape neither opens a file nor closes the modal",
      "Keyboard-only search opens the unique owned Korean filename",
      "Real editor font controls and renderer scale enlarge the task surface",
    ],
    screenshotPaths: screenshots,
  });
}
