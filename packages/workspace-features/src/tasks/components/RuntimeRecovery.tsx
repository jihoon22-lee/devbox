import { useCallback, useEffect, useState } from "react";
import { isProductHosted } from "../../transport";
import { listRuntimeControls, reviewRuntimeControl, type RuntimeControlReceipt } from "../api";
import { forgetReviewedControl } from "../runtimeControls";

export default function RuntimeRecovery({
  active,
  busy,
  onReviewed,
  onTarget,
}: {
  active: boolean;
  busy: boolean;
  onReviewed: () => void;
  onTarget?: (receipt: RuntimeControlReceipt) => void;
}) {
  const [items, setItems] = useState<RuntimeControlReceipt[]>([]);
  const [error, setError] = useState("");
  const [reviewing, setReviewing] = useState(false);
  const refresh = useCallback(async () => {
    setItems(await listRuntimeControls());
    setError("");
  }, []);
  useEffect(() => {
    if (!active || busy || !isProductHosted()) return;
    let disposed = false;
    void listRuntimeControls()
      .then((values) => {
        if (!disposed) {
          setItems(values);
          setError("");
        }
      })
      .catch(() => {
        if (!disposed) setError("완료되지 않은 실행 요청을 확인하지 못했습니다.");
      });
    return () => {
      disposed = true;
    };
  }, [active, busy]);
  const review = async (id: string) => {
    setReviewing(true);
    setError("");
    try {
      await reviewRuntimeControl(id);
      forgetReviewedControl(id);
      await refresh();
      onReviewed();
    } catch {
      setError("실행 소유권이 아직 확인되지 않았거나 검토를 저장하지 못했습니다. 현재 작업 상태를 다시 확인해 주세요.");
    } finally {
      setReviewing(false);
    }
  };
  if (!items.length && !error) return null;
  return (
    <section aria-label="실행 요청 복구">
      <p>대상 실행과 로그를 먼저 확인하세요. 요청 기록 정리는 실행 중인 프로세스를 중단하지 않습니다.</p>
      {error && <p role="alert">{error}</p>}
      <ul>
        {items.map((item) => (
          <li key={item.operationId}>
            <span>
              {(
                {
                  run_job: "작업 실행 요청",
                  stop_run: "작업 중단 요청",
                  start_service: "서비스 시작 요청",
                  stop_service: "서비스 중지 요청",
                  restart_service: "서비스 재시작 요청",
                } as Record<string, string>
              )[item.method] ?? "실행 제어 요청"}{" "}
              · {item.state === "pending" ? "처리 중" : "연결 중단 후 확인 필요"} ·{" "}
              {new Date(item.createdAt).toLocaleString("ko-KR")}
            </span>{" "}
            {onTarget && (
              <button type="button" disabled={busy || reviewing} onClick={() => onTarget(item)}>
                대상 실행 상태 보기
              </button>
            )}
            {item.state === "interrupted" && (
              <button type="button" disabled={reviewing || busy} onClick={() => void review(item.operationId)}>
                실행 상태 확인 후 요청 기록 정리
              </button>
            )}
          </li>
        ))}
      </ul>
      <button
        type="button"
        disabled={reviewing || busy}
        onClick={() => void refresh().catch(() => setError("요청 상태를 확인하지 못했습니다."))}
      >
        상태 새로고침
      </button>
    </section>
  );
}
