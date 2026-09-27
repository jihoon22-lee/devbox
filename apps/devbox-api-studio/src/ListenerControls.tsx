import { usePolling } from "@devbox/hooks";
import type { LifecycleCall } from "@devbox/api-studio-features/generated/LifecycleCall";
import { webhookCall } from "@devbox/api-studio-features/calls";
import { useRef, useEffect, useState } from "react";
import { nativeMode } from "@devbox/product-shell/api";

interface Status {
  policy: "stop-on-close" | "keep-listening";
  running: boolean;
  backgroundAvailable: boolean;
  closing: boolean;
  stopFailed: boolean;
  settingsWritable: boolean;
}
export function ListenerControls() {
  const [status, setStatus] = useState<Status | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const pollListenerCallback = useRef<() => Promise<void> | void>(() => {});
  const { refresh: pollListener } = usePolling(() => pollListenerCallback.current(), {
    intervalMs: 2000,
    active: nativeMode,
    immediate: false,
  });
  useEffect(() => {
    if (!nativeMode) return;
    let alive = true,
      pending = false;
    const refresh = async () => {
      if (pending) return;
      pending = true;
      try {
        const value = await webhookCall("lifecycle_status", {});
        if (alive) setStatus(value);
      } catch {
        if (alive) setError("서버 종료 설정을 확인하지 못했습니다.");
      } finally {
        pending = false;
      }
    };
    pollListenerCallback.current = refresh;
    pollListener();
    return () => {
      alive = false;
      pollListenerCallback.current = () => {};
    };
  }, [pollListener]);
  async function act<M extends LifecycleCall["method"]>(
    method: M,
    args: Extract<LifecycleCall, { method: M }> extends { args: infer A } ? A : never,
  ) {
    setBusy(true);
    setError(null);
    try {
      await webhookCall(method, args);
      if (method !== "quit_product") setStatus(await webhookCall("lifecycle_status", {}));
    } catch (cause) {
      const code = cause instanceof Error ? cause.message : "";
      setError(
        code === "lifecycle_settings_changed"
          ? "종료 설정이 변경됐거나 읽을 수 없습니다. 앱을 다시 열어 확인해 주세요."
          : "서버 종료 설정을 적용하지 못했습니다.",
      );
    } finally {
      setBusy(false);
    }
  }
  return (
    <section className="listener-controls" aria-label="Webhook 서버 종료 설정">
      <label>
        창을 닫을 때{" "}
        <select
          disabled={!status || busy || !status.settingsWritable}
          value={status?.policy ?? "keep-listening"}
          onChange={(event) =>
            void act("set_close_policy", {
              policy: event.target.value === "keep-listening" ? "keep-listening" : "stop-on-close",
            })
          }
        >
          <option value="keep-listening" disabled={!status?.backgroundAvailable}>
            닫아도 계속 듣기 (기본)
          </option>
          <option value="stop-on-close">닫을 때 리스너 멈추기</option>
        </select>
      </label>
      {status?.backgroundAvailable ? (
        <p>창을 닫으면 API Studio는 종료되고 백그라운드 서비스가 계속 요청을 받습니다.</p>
      ) : (
        <p>portable에서는 창을 닫으면 리스너도 중지됩니다.</p>
      )}
      <p>
        백그라운드 서비스가 재시작되면 수신 기록과 일반 응답 규칙은 사라집니다. 저장한 fixture와 내보낸 서비스 프로필은
        유지됩니다.
      </p>
      <p>별도로 실행한 서비스 작업은 해당 실행 소유자가 관리합니다.</p>
      {status && !status.settingsWritable && (
        <p role="alert">저장된 종료 설정은 보존됩니다. 확인할 수 없는 설정에는 기본 종료 정책을 적용합니다.</p>
      )}
      {status?.stopFailed && <p role="alert">임시 서버를 중지하지 못했습니다. 상태를 확인한 후 다시 시도해 주세요.</p>}
      {!nativeMode && <p>종료 설정은 Windows 앱에서 사용할 수 있습니다.</p>}
      <div>
        <button disabled={busy || !status || status.closing} onClick={() => void act("quit_product", {})}>
          API Studio 닫기
        </button>
      </div>
      {error && <p role="alert">{error}</p>}
    </section>
  );
}
