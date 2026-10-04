import { withOwnedNativeZoom } from "./windows-user-flow-window.mjs";
import assert from "node:assert/strict";
import { until, button } from "./windows-api-user-flow-actions.mjs";
import { writeProductInputObservation } from "./windows-suite-layout.mjs";

export async function observeEnvironmentIme(context) {
  const before = await context.document("environments");
  await context.ui.click({ role: "textbox", name: "환경 변수 1 이름" });
  await context.ui.press("Control+a");
  await context.cdp.command("Input.imeSetComposition", {
    text: "한글",
    selectionStart: 2,
    selectionEnd: 2,
    replacementStart: 0,
    replacementEnd: 0,
  });
  await context.ui.press("Enter");
  assert.equal(
    await context.cdp.evaluate("document.activeElement?.getAttribute('aria-label')"),
    "환경 변수 1 이름",
    "Enter during composition blurred the variable name",
  );
  assert.deepEqual(await context.document("environments"), before, "Composition shortcut persisted an unfinished name");
  const screenshot = await context.ui.screenshot("api-ime-composing");
  await context.cdp.command("Input.imeSetComposition", { text: "", selectionStart: 0, selectionEnd: 0 });
  // The existing rename journey replaces composition and verifies its commit.
  context.inputObservation = {
    ime: true,
    assertions: ["Actual Korean IME composition plus Enter retains focus and does not persist unfinished text"],
    screenshotPaths: [screenshot],
  };
}

export async function observePipelineKeyboardModal(context) {
  let focused = false;
  for (let i = 0; i < 160; i++) {
    if (
      await context.cdp.evaluate(
        "document.activeElement?.matches('.smart-workflow-pipeline .api-handoff-button')===true",
      )
    ) {
      focused = true;
      break;
    }
    await context.ui.press("Tab");
  }
  assert.ok(focused, "Keyboard traversal did not reach the pipeline handoff");
  await context.ui.press("Enter");
  await until(
    async () => await context.cdp.evaluate("Boolean(document.querySelector('.api-handoff-dialog'))"),
    "Keyboard did not open the handoff modal",
  );
  for (let i = 0; i < 12; i++) {
    await context.ui.press(i % 2 ? "Shift+Tab" : "Tab");
    assert.equal(
      await context.cdp.evaluate("Boolean(document.activeElement?.closest('.api-handoff-dialog'))"),
      true,
      "Modal keyboard focus escaped",
    );
  }
  const screenshot = await context.ui.screenshot("api-keyboard-modal");
  await context.ui.press("Escape");
  assert.equal(await context.cdp.evaluate("Boolean(document.querySelector('.api-handoff-dialog'))"), false);
  assert.equal(
    await context.cdp.evaluate(
      "document.activeElement?.matches('.smart-workflow-pipeline .api-handoff-button')===true",
    ),
    true,
    "Modal close did not return focus",
  );
  assert.ok(context.inputObservation?.ime, "IME observation missing");
  Object.assign(context.inputObservation, { keyboard: true, modalFocusReturn: true });
  context.inputObservation.assertions.push(
    "Actual Tab/Enter opens the pipeline preview, modal Tab/Shift+Tab retains focus, and Escape returns focus to its sender",
  );
  context.inputObservation.screenshotPaths.push(screenshot);
  await context.ui.click(button("Requests로 보내기", { role: "region", name: "타입 지정 파이프라인" }));
}

export async function finishApiInputObservation(context) {
  const observed = context.inputObservation;
  assert.ok(observed?.keyboard && observed.modalFocusReturn && observed.ime);
  await withOwnedNativeZoom(context.windowOwner, context.cdp, async () => {
    assert.equal(
      await context.cdp.evaluate("document.documentElement.scrollWidth<=innerWidth+1"),
      true,
      "Scaled API content clips horizontally",
    );
    assert.equal(await context.cdp.evaluate("Boolean(document.querySelector('main'))"), true);
    observed.screenshotPaths.push(await context.ui.screenshot("api-scaled-completed-task"));
    observed.assertions.push(
      "Actual Ctrl+Plus changes device scale after completed pipeline persistence and leaves API main content without horizontal clipping",
    );
    await writeProductInputObservation({
      product: "api-studio",
      checks: { keyboard: true, modalFocusReturn: true, ime: true, scale: true, taskComplete: true },
      assertions: observed.assertions,
      screenshotPaths: observed.screenshotPaths,
    });
  });
}
