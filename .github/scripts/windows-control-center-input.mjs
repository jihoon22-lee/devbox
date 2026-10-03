import assert from "node:assert/strict";
import { observeUntil } from "./windows-suite-ui-context.mjs";
import { writeProductInputObservation } from "./windows-suite-layout.mjs";
import { observeProductLayout, assertProductLayout } from "./browser-product-layout.mjs";
export async function observeControlCenterInput({ cdp, ui }) {
  async function tabTo(expression) {
    for (let count = 0; count < 80; count++) {
      if (await cdp.evaluate(expression)) return;
      await ui.press("Tab");
    }
    throw new Error("Control Center keyboard target unreachable");
  }
  await tabTo('document.activeElement?.textContent?.trim()==="Launcher 열기"');
  await ui.press("Enter");
  await observeUntil(
    () => cdp.evaluate('Boolean(document.querySelector("dialog[open] #launcher-search"))'),
    "keyboard launcher",
  );
  await ui.press("Tab");
  assert.equal(await cdp.evaluate('Boolean(document.activeElement?.closest("dialog[open]"))'), true);
  await ui.press("Shift+Tab");
  await tabTo('document.activeElement?.id==="launcher-search"');
  await cdp.command("Input.imeSetComposition", {
    text: "한",
    selectionStart: 1,
    selectionEnd: 1,
    replacementStart: 0,
    replacementEnd: 0,
  });
  await ui.press("Enter");
  assert.equal(
    await cdp.evaluate('Boolean(document.querySelector("dialog[open] #launcher-search"))'),
    true,
    "Composing Enter must not launch a command or close modal",
  );
  await cdp.command("Input.insertText", { text: "한글" });
  assert.ok((await cdp.evaluate('document.querySelector("#launcher-search").value')).includes("한글"));
  await ui.press("Escape");
  await observeUntil(() => cdp.evaluate('!document.querySelector("dialog[open]")'), "launcher Escape close");
  assert.equal(await cdp.evaluate("document.activeElement?.textContent?.trim()"), "Launcher 열기");
  const original = await cdp.evaluate("devicePixelRatio");
  await cdp.command("Input.dispatchKeyEvent", {
    type: "keyDown",
    key: "+",
    code: "Equal",
    windowsVirtualKeyCode: 187,
    modifiers: 2,
  });
  await cdp.command("Input.dispatchKeyEvent", {
    type: "keyUp",
    key: "+",
    code: "Equal",
    windowsVirtualKeyCode: 187,
    modifiers: 2,
  });
  await observeUntil(async () => (await cdp.evaluate("devicePixelRatio")) > original, "actual WebView text scale");
  assertProductLayout(await cdp.evaluate(`(${observeProductLayout.toString()})()`));
  const screenshots = [await ui.screenshot("UI-02-control-center-zoom")];
  await ui.press("Control+0");
  await tabTo('document.activeElement?.textContent?.trim()==="데이터 및 복구"');
  await ui.press("Enter");
  await observeUntil(
    () => cdp.evaluate('document.body.innerText.includes("현재 데이터 보존")'),
    "keyboard recovery navigation",
  );
  await writeProductInputObservation({
    product: "control-center",
    checks: { keyboard: true, modalFocusReturn: true, ime: true, scale: true, taskComplete: true },
    assertions: [
      "Keyboard-only Launcher open, modal Tab/ShiftTab, Korean composition Enter suppression and Escape focus return",
      "Real WebView zoom retains primary controls and keyboard navigation reaches data recovery",
    ],
    screenshotPaths: screenshots,
  });
}
