import { useEffect, useState } from "react";
import { quitCall } from "@devbox/knowledge-features/commands/api";
import { nativeMode } from "@devbox/product-shell/api";
import type { CollectorStatus } from "@devbox/knowledge-features/generated/CollectorStatus";
export function collectorMessage(status: CollectorStatus | null): string {
  if (!status || status.owner === "unknown" || status.tracking === null)
    return "활동 수집 상태를 확인하지 못했습니다. Devbox 알림 영역에서 상태를 확인하세요.";
  if (status.owner === "portableLocal") return "portable 창을 닫으면 이 창의 활동 수집도 중지됩니다.";
  return status.tracking
    ? "설치본은 창을 닫아도 동의한 활동 수집이 계속됩니다. 중지하려면 활동 화면이나 Devbox 알림 영역을 사용하세요."
    : "활동 수집이 일시중지되어 있습니다. 창을 닫아도 자동으로 시작하지 않습니다.";
}
export function useCollectorStatus(refresh: unknown): CollectorStatus | null {
  const [status, setStatus] = useState<{ refresh: unknown; value: CollectorStatus } | null>(null);
  useEffect(() => {
    if (!nativeMode) return;
    let active = true;
    setStatus(null);
    void quitCall("lifecycle_status", {})
      .then((value) => {
        if (active) setStatus({ refresh, value });
      })
      .catch(() => {});
    return () => {
      active = false;
    };
  }, [refresh]);
  return status && status.refresh === refresh ? status.value : null;
}
