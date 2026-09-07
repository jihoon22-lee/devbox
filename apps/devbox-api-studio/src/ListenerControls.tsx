import { useEffect, useState } from "react";
import { nativeMode } from "@devbox/product-shell/api";
import { componentInvoke } from "@devbox/api-studio-features/transport";
const invoke = componentInvoke("api-studio.webhooks");
interface Status { policy: "stop-on-close" | "keep-listening"; running: boolean; trayAvailable: boolean; closing: boolean; stopFailed: boolean; settingsWritable: boolean }
export function ListenerControls() {
  const [status, setStatus] = useState<Status | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (!nativeMode) return;
    let alive = true, pending = false;
    const refresh = async () => {
      if (pending) return; pending = true;
      try { const value = await invoke<Status>("lifecycle_status"); if (alive) setStatus(value); }
      catch { if (alive) setError("서버 종료 설정을 확인하지 못했습니다."); }
      finally { pending = false; }
    };
    void refresh(); const timer = setInterval(() => { void refresh(); }, 2000);
    return () => { alive = false; clearInterval(timer); };
  }, []);
  async function act(method: string, args?: Record<string, unknown>) {
    setBusy(true); setError(null);
    try { await invoke(method, args); if (method !== "quit_product") setStatus(await invoke<Status>("lifecycle_status")); }
    catch (cause) {
      const code = cause instanceof Error ? cause.message : "";
      setError(code === "lifecycle_settings_changed" ? "종료 설정이 변경됐거나 읽을 수 없습니다. 앱을 다시 열어 확인해 주세요."
        : code === "lifecycle_tray_unavailable" ? "알림 영역 아이콘을 사용할 수 없어 창을 숨길 수 없습니다."
        : "서버 종료 설정을 적용하지 못했습니다.");
    } finally { setBusy(false); }
  }
  return <section className="listener-controls" aria-label="Webhook 서버 종료 설정">
    <label>창을 닫을 때 <select disabled={!status || busy || !status.settingsWritable} value={status?.policy ?? "stop-on-close"}
      onChange={event => void act("set_close_policy", { policy: event.target.value })}>
      <option value="stop-on-close">임시 서버를 중지하고 앱 종료</option>
      <option value="keep-listening" disabled={!status?.trayAvailable}>서버 유지 · 알림 영역으로 숨기기</option>
    </select></label>
    <p>다른 화면으로 이동하거나 최소화해도 서버는 유지됩니다. 완전히 종료하면 이 창의 임시 서버가 중지됩니다.</p>
    <p>서비스로 내보낸 설정은 비활성 상태이며, 별도로 실행한 서비스 작업은 해당 실행 소유자가 관리합니다.</p>
    {status?.policy === "keep-listening" && <p>알림 영역 아이콘에서 창 열기·서버 중지·완전 종료를 선택할 수 있습니다.{!status.trayAvailable ? " 현재 아이콘을 사용할 수 없어 창을 닫으면 종료합니다." : ""}</p>}
    {status && !status.settingsWritable && <p role="alert">저장된 종료 설정은 보존됩니다. 확인할 수 없는 설정에는 기본 종료 정책을 적용합니다.</p>}
    {status?.stopFailed && <p role="alert">임시 서버를 중지하지 못했습니다. 상태를 확인한 후 다시 시도해 주세요.</p>}
    {!nativeMode && <p>종료 설정은 Windows 앱에서 사용할 수 있습니다.</p>}
    <div><button disabled={busy || !status?.running || status.policy !== "keep-listening" || !status.trayAvailable} onClick={() => void act("hide_main_window")}>서버를 유지하고 창 숨기기</button>
      <button disabled={busy || !status || status.closing} onClick={() => void act("quit_product")}>API Studio 완전히 종료</button></div>
    {error && <p role="alert">{error}</p>}
  </section>;
}
