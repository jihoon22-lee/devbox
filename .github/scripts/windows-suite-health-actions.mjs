import {
  waitForOwnedHealthConnections,
  ownedConnectionStatus,
  projectHealthRows,
} from "./windows-suite-health-readiness.mjs";
// Complete the native health stage through the installed Control Center controls.
import { randomUUID } from "node:crypto";
import { readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { createInstalledProductContext, observeUntil } from "./windows-suite-ui-context.mjs";
import { captureWindowOwner, nativeWindowAction } from "./windows-user-flow-window.mjs";
import { allWindowsProcesses } from "./windows-packaged-smoke.mjs";
import { preserveReviewedCommitFailure } from "./windows-reviewed-helper-evidence.mjs";
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
  const observationId = randomUUID();
  let center;
  let failure;
  let stage = "openProducts";
  try {
    center = await createInstalledProductContext("control-center");
    live.push(center);
    await center.ui.click({ role: "button", name: "데이터 및 복구", scope: { role: "navigation", name: "제품 화면" } });
    for (const product of ["workspace", "api-studio", "knowledge"])
      live.push(await createInstalledProductContext(product));
    stage = "nativeConnectionReadiness";
    await waitForOwnedHealthConnections(live, observeUntil);
    stage = "nativeHealthObservations";
    await center.ui.click({ role: "button", name: "네 제품 상태 확인" });
    await observeUntil(
      async () => (await center.body()).split("응답·저장소 선택 확인됨").length - 1 === 4,
      "four native health observations",
    );
    stage = "recordedHealth";
    await center.ui.click({ role: "button", name: "상태 기록" });
    // Recording acquires the same native journal lock as inventory. Observe
    // completion of the one UI action before polling that read-only projection.
    await center.ui.waitForTarget({ role: "button", name: "상태 기록" });
    await observeUntil(
      async () => (await center.delivery("restore_inventory")).installation.freshHealth,
      "fresh owner health recorded",
    );
    stage = "reviewedCommit";
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
  } catch (error) {
    failure = error;
    if (center) {
      try {
        await center.ui.screenshot(`health-first-failure-${observationId}`);
      } catch {
        // A screenshot failure must never replace the original health failure.
      }
      try {
        const rows = await center.cdp.evaluate(
          `Array.from(document.querySelectorAll('[aria-label="제품 상태 확인"] tbody tr')).slice(0,4).map(row=>Array.from(row.cells).map(cell=>cell.textContent.trim()))`,
        );
        const connections = await Promise.all(
          live.map(async (context) => {
            try {
              return await ownedConnectionStatus(context);
            } catch {
              return { observationUnavailable: true };
            }
          }),
        );
        await writeFile(
          `product-foundation-evidence/health-first-failure-${observationId}.json`,
          JSON.stringify({ stage, rows: projectHealthRows(rows), connections }, null, 2),
          { flag: "wx" },
        );
      } catch {
        // Bounded supplemental observations never replace the first failure.
      }
      try {
        await preserveReviewedCommitFailure(center, error, observationId);
      } catch {
        // Evidence failure never replaces the first user journey failure.
      }
    }
    throw error;
  } finally {
    for (const item of live.reverse()) {
      try {
        if (item.child.exitCode === null) await item.close();
        else item.dispose();
      } catch (error) {
        if (!failure) failure = error;
      }
    }
    if (failure) throw failure;
  }
}
