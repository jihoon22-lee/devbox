import { useEffect, useRef, useState } from "react";
import { nativeMode } from "@devbox/product-shell/api";
import { componentInvoke } from "@devbox/knowledge-features/transport";
const invoke = componentInvoke("knowledge.activity");
interface Policy { closeToTray: boolean; trayAvailable: boolean }
export default function LifecycleSettings() {
  const [policy, setPolicy] = useState<Policy | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const alive = useRef(false);
  const pending = useRef(false);
  useEffect(() => {
    alive.current = true;
    if (nativeMode) {
      void invoke<Policy>("get_close_policy").then(value => { if (alive.current) setPolicy(value); })
        .catch(error => { if (alive.current) setError(error instanceof Error ? error.message : "종료 설정을 확인하지 못했습니다."); });
    }
    return () => { alive.current = false; };
  }, []);
  const change = async (closeToTray: boolean) => {
    if (!policy || pending.current) return;
    pending.current = true; setBusy(true); setError(null);
    try {
      const value = await invoke<Policy>("set_close_policy", { closeToTray });
      if (alive.current) setPolicy(value);
    } catch (error) {
      if (alive.current) setError(error instanceof Error ? error.message : "종료 설정을 저장하지 못했습니다.");
    } finally { pending.current = false; if (alive.current) setBusy(false); }
  };
  return <section className="panel" aria-labelledby="knowledge-close-title">
    <h2 id="knowledge-close-title">창 닫기와 활동 수집</h2>
    <label className="row"><input type="checkbox" checked={policy?.closeToTray ?? false} disabled={!nativeMode || !policy || busy || (!policy.trayAvailable && !policy.closeToTray)} onChange={event => void change(event.currentTarget.checked)}/>창을 닫을 때 트레이로 숨기기</label>
    <p className="dim">기본값은 앱 종료입니다. 트레이로 숨기면 앱이 계속 실행되며, 이미 켠 활동 수집도 계속됩니다. 수집은 활동 화면에서 별도로 켜거나 중지할 수 있습니다.</p>
    <p className="dim">트레이 메뉴의 ‘Knowledge 종료 · 수집 중지’를 선택하면 앱과 수집을 함께 종료합니다. 다음 실행에는 저장된 수집 설정을 사용합니다.</p>
    {!nativeMode && <p>브라우저에서는 종료 설정을 변경할 수 없습니다.</p>}
    {policy && !policy.trayAvailable && <p role="status">트레이를 사용할 수 없어 창을 닫으면 앱이 종료됩니다.</p>}
    {busy && <p role="status">종료 설정을 저장하고 있습니다…</p>}
    {error && <p role="alert">{error}</p>}
  </section>;
}
