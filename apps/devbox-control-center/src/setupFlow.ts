import type { RestoreInventory } from "@devbox/control-center-features/generated/RestoreInventory";
import type { RecoveryStatus } from "@devbox/control-center-features/generated/RecoveryStatus";
export type SetupStage = "prepareStores" | "review" | "activating" | "health" | "committing" | "complete" | "recover";
export interface SetupView {
  stage: SetupStage;
  next: "openOwner" | "recordHealth" | "activateClean" | "commitClean" | null;
  blockedReason: string | null;
}
export function deriveSetupView(inventory: RestoreInventory, recovery: RecoveryStatus): SetupView {
  const current = inventory.installation;
  if (inventory.activeOperation || inventory.update || current.reinstall)
    return {
      stage: "recover",
      next: null,
      blockedReason: "진행 중인 업데이트 또는 데이터 복원 작업을 아래에서 확인해 주세요.",
    };
  if (current.committed) return { stage: "complete", next: null, blockedReason: null };
  if (current.phase === "recover" || (recovery.state === "recorded" && recovery.failure))
    return {
      stage: "recover",
      next: null,
      blockedReason: "설치 기록에 중단된 작업이 있습니다. 아래 복구 기록을 확인해 주세요.",
    };
  if (["health", "commit"].includes(current.phase))
    return {
      stage: current.freshHealth ? "committing" : "health",
      next: current.freshHealth ? "commitClean" : "recordHealth",
      blockedReason: current.freshHealth ? null : "네 제품을 열고 최신 실행 상태를 기록해 주세요.",
    };
  if (["snapshot", "import", "validate", "quiesce", "activate"].includes(current.phase)) {
    if (current.clean && current.recordedOwners === 4)
      return { stage: "review", next: "activateClean", blockedReason: null };
    return {
      stage: "prepareStores",
      next: "openOwner",
      blockedReason:
        "각 제품에서 저장소를 준비한 뒤 상태를 기록해 주세요. 진행 중이거나 확인이 필요한 제품은 먼저 완료하세요.",
    };
  }
  return {
    stage: "recover",
    next: null,
    blockedReason: "설치 단계를 확인할 수 없습니다. 기록을 새로 읽고 복구 상태를 확인해 주세요.",
  };
}
