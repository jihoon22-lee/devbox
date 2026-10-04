import { boundedFailure } from "./user-flow-failure-evidence.mjs";
export async function navigateWorkspaceFiles(ui) {
  await ui.click({ role: "button", name: "파일" });
  await ui.waitForTarget({ role: "textbox", name: "열 파일 경로" });
}
export async function observeWorkspaceFailure(ui, id, error) {
  const result = { error: boundedFailure(error), screenshotPaths: [] };
  try {
    result.screenshotPaths.push(await ui.screenshot(`${id}-first-failure`));
  } catch {}
  return result;
}
export async function cleanupWorkspaceFixture(fixture, results) {
  try {
    await fixture.cleanup();
  } catch (error) {
    const failures = results.filter((result) => result.status === "FAIL");
    if (!failures.length) throw error;
    for (const result of failures) result.cleanupError = boundedFailure(error);
  }
}
export async function stopWorkspaceBeforeDisconnect(stop, disconnect) {
  let stopped = false;
  try {
    // Detaching a debugger can resume its paused reply and consume pending state.
    await stop();
    stopped = true;
  } finally {
    try {
      await disconnect();
    } catch (error) {
      if (stopped) throw error;
    }
  }
}
