import { useEffect, useState, type ReactNode } from "react";
import { nativeMode } from "@devbox/product-shell/api";
import { componentInvoke } from "@devbox/knowledge-features/transport";
const invoke = componentInvoke("knowledge.migration");
export function Startup({ children }: { children: ReactNode }) {
  const [active, setActive] = useState(!nativeMode);
  const [loading, setLoading] = useState(nativeMode);
  const [blocked, setBlocked] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    if (!nativeMode) return;
    let alive = true;
    void invoke<{ active: boolean }>("status").then(value => { if (alive) setActive(value.active === true); })
      .catch(error => { if (alive) { setError(error instanceof Error ? error.message : "저장소를 확인하지 못했습니다."); setBlocked(error instanceof Error && ["future_schema", "store_invalid", "restart_required"].includes(error.name)); } })
      .finally(() => { if (alive) setLoading(false); });
    return () => { alive = false; };
  }, []);
  if (active) return children;
  const start = async () => {
    setLoading(true); setError(null);
    try { const value = await invoke<{ active: boolean }>("start_empty"); setActive(value.active === true); }
    catch (error) { setError(error instanceof Error ? error.message : "저장소를 준비하지 못했습니다."); setBlocked(error instanceof Error && ["future_schema", "store_invalid", "restart_required"].includes(error.name)); }
    finally { setLoading(false); }
  };
  return <section className="knowledge-startup" aria-labelledby="knowledge-start-title">
    <h2 id="knowledge-start-title">Knowledge 시작</h2>
    <p>노트와 활동, 파일 검색을 위한 저장소를 준비합니다. 활동 수집은 직접 켜기 전까지 시작하지 않습니다.</p>
    {loading && <p role="status">저장소를 확인하고 있습니다…</p>}
    {error && <p role="alert">{error}</p>}
    <button type="button" disabled={loading || blocked} onClick={() => void start()}>새 저장소로 시작</button>
  </section>;
}
