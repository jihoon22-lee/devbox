import assert from "node:assert/strict";
import { setTimeout as delay } from "node:timers/promises";
import { nativeWindowAction } from "./windows-user-flow-window.mjs";

export async function ownedFilePickerWhenReady(
  owner,
  filePath,
  pickerAction,
  { action = nativeWindowAction, wait = delay, timeoutMs = 10000 } = {},
) {
  const deadline = Date.now() + timeoutMs;
  let controls = null;
  while (true) {
    const observed = await action(owner, "Inspect");
    assert.equal(observed.processId, owner.identity.Pid, "Native picker owner changed");
    assert.ok(observed.nativeWindowCount <= 32, "Native picker inventory exceeded bound");
    const pickers = observed.nativeWindows.filter(
      (window) => window.visible && window.topLevel && window.className === "#32770",
    );
    assert.ok(pickers.length <= 1, "Ambiguous owned native file picker");
    assert.ok(
      pickers.every((window) => window.nativeProcessId === owner.identity.Pid),
      "Native picker owner changed",
    );
    if (pickers.length === 1) {
      const observation = await action(owner, "InspectFilePicker");
      controls = Object.fromEntries(
        ["pickerCount", "fieldCount", "editCount", "confirmCount"].map((key) => {
          const count = observation?.[key];
          return [key, Number.isSafeInteger(count) && count >= 0 && count <= 1024 ? count : null];
        }),
      );
      controls.filenameReady = observation?.filenameReady === true;
      controls.confirmationReady = observation?.confirmationReady === true;
      assert.ok(
        Object.values(controls).every((value) => value !== null),
        "Invalid native picker control counts",
      );
      assert.ok(
        [controls.pickerCount, controls.fieldCount, controls.editCount, controls.confirmCount].every(
          (count) => count <= 1,
        ),
        `Ambiguous native picker controls: ${JSON.stringify(controls)}`,
      );
      if (
        controls.pickerCount === 1 &&
        controls.fieldCount === 1 &&
        controls.confirmCount === 1 &&
        controls.filenameReady &&
        controls.confirmationReady
      )
        return await action(owner, pickerAction, { filePath });
    }
    assert.ok(
      Date.now() < deadline,
      `Owned native file picker controls did not become ready: ${JSON.stringify(controls)}`,
    );
    await wait(100);
  }
}
