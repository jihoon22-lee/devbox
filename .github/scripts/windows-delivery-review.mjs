import assert from "node:assert/strict";
import { observeUntil } from "./windows-suite-ui-context.mjs";
export async function readRestoreInventory(center, observe = observeUntil, accept = () => true) {
  let inventory, firstBusy;
  try {
    await observe(async () => {
      try {
        inventory = await center.delivery("restore_inventory");
        return accept(inventory);
      } catch (error) {
        if (error?.message !== "Read-only delivery restore_inventory failed: unavailable/suite_update_busy")
          throw error;
        firstBusy ??= error;
        return false;
      }
    }, "owned restore inventory lock available");
  } catch (error) {
    if (error?.message === "Owned UI observation timed out: owned restore inventory lock available" && firstBusy)
      throw firstBusy;
    throw error;
  }
  return inventory;
}

export async function waitDeliveryInventoryReady(ui) {
  // This heading is rendered only once inventory exists, even during restore health.
  await ui.waitForTarget({ role: "heading", name: "제품 데이터 보존본" });
}
export async function executeReviewedDeliveryAction(ui, target, beforeExecute = async () => {}) {
  await ui.waitForTarget(target);
  await ui.click(target);
  const acknowledgement = { role: "checkbox", name: "선택한 작업과 제품 종료를 확인했습니다." };
  await ui.waitForTarget(acknowledgement);
  await ui.click(acknowledgement);
  const execute = { role: "button", name: "Control Center를 닫고 실행" };
  await ui.waitForTarget(execute);
  await beforeExecute();
  await ui.click(execute);
}
export function selectCurrentGenerationSnapshot(before, after, oldId) {
  const previous = before.checkpoints.find((item) => item.id === oldId);
  assert.ok(previous, "Pinned update checkpoint must remain recorded");
  assert.equal(previous.compatibility, "differentGeneration");
  assert.deepEqual(
    after.checkpoints.find((item) => item.id === oldId),
    previous,
  );
  const previousIds = new Set(before.checkpoints.map((item) => item.id));
  const added = after.checkpoints.filter((item) => !previousIds.has(item.id));
  assert.equal(added.length, 1, "Reviewed snapshot must create exactly one new checkpoint");
  assert.notEqual(added[0].id, oldId);
  assert.equal(added[0].compatibility, "currentGeneration");
  return added[0];
}
