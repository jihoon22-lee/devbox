import SetupHealth from "./SetupHealth";
import type { ShellContentProps } from "@devbox/product-shell";
import type { RestoreInventory } from "@devbox/control-center-features/generated/RestoreInventory";
import type { RecoveryStatus } from "@devbox/control-center-features/generated/RecoveryStatus";
import { deriveSetupView } from "./setupFlowState";
export default function SetupFlow({
  description,
  route,
  inventory,
  recovery,
  onRecorded,
  onAction,
  busy,
}: {
  description: ShellContentProps["description"];
  route: string;
  inventory: RestoreInventory;
  recovery: RecoveryStatus;
  onRecorded: () => Promise<void>;
  onAction: (action: "activateClean" | "commitClean") => void;
  busy: boolean;
}) {
  const view = deriveSetupView(inventory, recovery);
  if (view.stage === "complete") return <p role="status">이 설치는 사용할 준비가 되었습니다.</p>;
  if (inventory.update || inventory.installation.reinstall || inventory.activeOperation) return null;
  return (
    <section aria-label="설치 준비 안내">
      <h2>Devbox 사용 준비</h2>
      <ol className="setup-steps">
        {[
          ["prepareStores", "저장소 준비"],
          ["review", "준비 검토"],
          ["health", "실행 확인"],
          ["committing", "활성화 확정"],
        ].map(([stage, label]) => (
          <li key={stage} aria-current={view.stage === stage ? "step" : undefined}>
            {label}
          </li>
        ))}
      </ol>
      {view.blockedReason && <p role="status">{view.blockedReason}</p>}
      {(view.next === "activateClean" || view.next === "commitClean") && (
        <button disabled={busy} onClick={() => onAction(view.next as "activateClean" | "commitClean")}>
          다음 단계
        </button>
      )}
      <SetupHealth description={description} route={route} onRecorded={onRecorded} busy={busy} />
      <p>준비 기록 {inventory.installation.recordedOwners}/4 · 이미 준비한 저장소는 다시 만들지 않습니다.</p>
    </section>
  );
}
