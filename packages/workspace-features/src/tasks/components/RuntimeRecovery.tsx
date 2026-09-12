import { useCallback, useEffect, useState } from "react";
import { isProductHosted } from "../../transport";
import { listRuntimeControls, reviewRuntimeControl, type RuntimeControlReceipt } from "../api";
import { forgetReviewedControl } from "../runtimeControls";

export default function RuntimeRecovery({active, busy, onReviewed}: {active: boolean; busy: boolean; onReviewed: () => void}) {
  const [items,setItems] = useState<RuntimeControlReceipt[]>([]);
  const [error,setError] = useState("");
  const [reviewing,setReviewing] = useState(false);
  const refresh = useCallback(async () => { setItems(await listRuntimeControls()); }, []);
  useEffect(() => {
    if (!active || busy || !isProductHosted()) return;
    let disposed = false;
    void listRuntimeControls().then(values => { if (!disposed) setItems(values); }).catch(() => { if (!disposed) setError("완료되지 않은 실행 요청을 확인하지 못했습니다."); });
    return () => { disposed = true; };
  }, [active,busy]);
  const review = async (id: string) => {
    setReviewing(true); setError("");
    try {
      await reviewRuntimeControl(id);
      forgetReviewedControl(id);
      await refresh();
      onReviewed();
    } catch { setError("실행 소유권이 아직 확인되지 않았거나 검토를 저장하지 못했습니다. 현재 작업 상태를 다시 확인해 주세요."); }
    finally { setReviewing(false); }
  };
  if (!items.length && !error) return null;
  return <section aria-label="실행 요청 복구">
    <p>연결이 끊긴 요청은 자동으로 다시 실행하지 않습니다. 현재 실행 상태를 확인해 주세요.</p>
    {error && <p role="alert">{error}</p>}
    <ul>{items.map(item => <li key={item.operationId}>
      <span>{item.state === "pending" ? "처리 중인 요청" : "중단된 요청"} · {item.targetId}</span>{" "}
      {item.state === "interrupted" && <button type="button" disabled={reviewing || busy} onClick={() => void review(item.operationId)}>현재 상태 확인 완료</button>}
    </li>)}</ul>
    <button type="button" disabled={reviewing || busy} onClick={() => void refresh().catch(() => setError("요청 상태를 확인하지 못했습니다."))}>상태 새로고침</button>
  </section>;
}
