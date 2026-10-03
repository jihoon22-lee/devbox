// Complete the native health stage through the installed Control Center controls.
import { randomUUID } from "node:crypto";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { createInstalledProductContext, observeUntil } from "./windows-suite-ui-context.mjs";
import { captureWindowOwner, nativeWindowAction } from "./windows-user-flow-window.mjs";
import { allWindowsProcesses } from "./windows-packaged-smoke.mjs";
export async function closeAutomaticallyOpenedCenter(root) {
  const manifest = JSON.parse(await readFile(path.join(root, "devbox-installation.json"), "utf8"));
  const image = path.resolve(root, manifest.members.find((p) => p.product === "control-center").executable);
  for (const item of allWindowsProcesses().filter((p) => p.Path.toLowerCase() === image.toLowerCase())) {
    nativeWindowAction(captureWindowOwner(item, path.dirname(root)), "Close");
    await observeUntil(
      () => !allWindowsProcesses().some((p) => p.Pid === item.Pid && p.Created === item.Created),
      "automatic Center normal close",
    );
  }
}
export async function completeInstalledHealth(label, { beforeCommit } = {}) {
  const live = [];
  let center;
  try {
    center = await createInstalledProductContext("control-center");
    live.push(center);
    await center.ui.click({ role: "button", name: "데이터 및 복구" });
    for (const product of ["workspace", "api-studio", "knowledge"])
      live.push(await createInstalledProductContext(product));
    await center.ui.click({ role: "button", name: "네 제품 상태 확인" });
    await observeUntil(
      async () => (await center.body()).split("응답·저장소 선택 확인됨").length - 1 === 4,
      "four native health observations",
    );
    await center.ui.click({ role: "button", name: "상태 기록" });
    await observeUntil(
      async () => (await center.delivery("restore_inventory")).installation.freshHealth,
      "fresh owner health recorded",
    );
    const observationId = randomUUID();
    const screenshot = await center.ui.screenshot(`health-${observationId}`);
    for (const item of live.slice(1).reverse()) await item.close();
    await center.ui.click({ role: "button", name: label });
    await center.ui.click({ role: "checkbox", name: "선택한 작업과 제품 종료를 확인했습니다." });
    const reviewScreenshot = await center.ui.screenshot(`review-${observationId}`);
    if (beforeCommit) await beforeCommit({ center, screenshot, reviewScreenshot });
    await center.ui.click({ role: "button", name: "Control Center를 닫고 실행" });
    await observeUntil(() => center.child.exitCode !== null, "reviewed commit Center shutdown");
    center.dispose();
    await observeUntil(
      async () => {
        try {
          return (
            JSON.parse(await readFile(path.join(center.root, "devbox-activation.json"), "utf8")).phase === "committed"
          );
        } catch {
          return false;
        }
      },
      "reviewed installation committed",
      90000,
    );
    await observeUntil(
      () => allWindowsProcesses().some((p) => p.Path.toLowerCase() === center.executable.toLowerCase()),
      "commit reopened Center",
      30000,
    );
    await closeAutomaticallyOpenedCenter(center.root);
    return [screenshot, reviewScreenshot];
  } finally {
    for (const item of live.reverse()) {
      if (item.child.exitCode === null) await item.close();
      else item.dispose();
    }
  }
}
