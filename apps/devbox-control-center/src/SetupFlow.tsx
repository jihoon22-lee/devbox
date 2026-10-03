import { useState } from "react";
import { makeRequest, nativeMode } from "@devbox/product-shell/api";
import type { ShellContentProps } from "@devbox/product-shell";
import type { RestoreInventory } from "@devbox/control-center-features/generated/RestoreInventory";
import type { RecoveryStatus } from "@devbox/control-center-features/generated/RecoveryStatus";
import { deliveryCall } from "./delivery";
import { deriveSetupView } from "./setupFlow";
import Health from "./Health";
import catalog from "../../../apps/products.json";
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
  const [opening, setOpening] = useState<string | null>(null),
    [error, setError] = useState("");
  const open = async (product: string) => {
    if (opening || busy || !nativeMode) return;
    setOpening(product);
    setError("");
    try {
      const header = makeRequest(description.handshake, route, Date.now(), description.context);
      const result = await deliveryCall(header, "open_setup_product", { product });
      if (!result.value.opened) throw new Error("not opened");
    } catch {
      setError("제품을 열지 못했습니다. 설치 상태를 확인한 뒤 다시 시도해 주세요.");
    } finally {
      setOpening(null);
    }
  };
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
      <p>준비 기록 {inventory.installation.recordedOwners}/4 · 이미 준비한 저장소는 다시 만들지 않습니다.</p>
      <div className="setup-products">
        {catalog.products
          .filter((p) => p.id !== "control-center")
          .map((product) => (
            <button key={product.id} disabled={!!opening || busy || !nativeMode} onClick={() => void open(product.id)}>
              {opening === product.id ? "여는 중…" : `${product.label} 열기`}
            </button>
          ))}
      </div>
      {error && <p role="alert">{error}</p>}
      <Health description={description} route={route} onRecorded={onRecorded} />
      {(view.next === "activateClean" || view.next === "commitClean") && (
        <button disabled={busy} onClick={() => onAction(view.next as "activateClean" | "commitClean")}>
          다음 단계
        </button>
      )}
    </section>
  );
}
