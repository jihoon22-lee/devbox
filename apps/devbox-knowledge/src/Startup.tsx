import { lazy, Suspense, useEffect, useState, type ReactNode } from "react";
import { nativeMode } from "@devbox/product-shell/api";
import { componentInvoke } from "@devbox/knowledge-features/transport";
const invoke = componentInvoke("knowledge.migration");
const VaultSettings = lazy(() => import("./VaultSettings"));
const VaultSetup = lazy(() => import("./VaultSetup"));
const MigrationSetup = lazy(() => import("./MigrationSetup"));
export function Startup({ children }: { children: ReactNode }) {
  const [active, setActive] = useState(!nativeMode);
  const [loading, setLoading] = useState(nativeMode);
  const [blocked, setBlocked] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [hasExisting, setHasExisting] = useState(false);
  const [showImport, setShowImport] = useState(false);
  const [showVault, setShowVault] = useState(false);
  const [canReconnect, setCanReconnect] = useState(false);
  const [reconnect, setReconnect] = useState(false);
  useEffect(() => {
    if (!nativeMode) return;
    let alive = true;
    void invoke<{ active: boolean; scheduled?: boolean; hasExisting?: boolean; vaultChange?: boolean; bindingUnavailable?: boolean }>("status").then(value => { if (alive) { setActive(value.active === true); setCanReconnect(value.bindingUnavailable === true); if (value.bindingUnavailable) setError("현재 노트 폴더에 연결할 수 없습니다. 폴더를 복구하거나 다른 폴더를 선택해 주세요."); setShowVault(value.vaultChange === true && !value.active); setHasExisting(value.hasExisting === true); setShowImport(value.scheduled === true && !value.active); } })
      .catch(error => { if (alive) { setError(error instanceof Error ? error.message : "저장소를 확인하지 못했습니다."); setBlocked(error instanceof Error && ["future_schema", "store_invalid", "restart_required", "import_schema_unsupported", "vault_change_invalid"].includes(error.name)); } })
      .finally(() => { if (alive) setLoading(false); });
    return () => { alive = false; };
  }, []);
  if (active) return children;
  if (reconnect) return <Suspense fallback={<p role="status">노트 폴더 설정을 불러오고 있습니다…</p>}><VaultSettings onScheduled={() => { setReconnect(false); setShowVault(true); }}/></Suspense>;
  if (showVault) return <Suspense fallback={<p role="status">노트 폴더 설정을 불러오고 있습니다…</p>}><VaultSetup onActivated={() => setActive(true)}/></Suspense>;
  if (showImport) return <Suspense fallback={<p role="status">가져오기 화면을 불러오고 있습니다…</p>}><MigrationSetup onActivated={() => setActive(true)} onBack={() => setShowImport(false)}/></Suspense>;
  const start = async () => {
    setLoading(true); setError(null);
    try { const value = await invoke<{ active: boolean }>(hasExisting ? "continue_existing" : "start_empty"); setActive(value.active === true); }
    catch (error) { setError(error instanceof Error ? error.message : "저장소를 준비하지 못했습니다."); setBlocked(error instanceof Error && ["future_schema", "store_invalid", "restart_required", "import_schema_unsupported", "vault_change_invalid"].includes(error.name)); }
    finally { setLoading(false); }
  };
  return <section className="knowledge-startup" aria-labelledby="knowledge-start-title">
    <h2 id="knowledge-start-title">Knowledge 시작</h2>
    <p>노트와 활동, 파일 검색을 위한 저장소를 준비합니다. 활동 수집은 직접 켜기 전까지 시작하지 않습니다.</p>
    {loading && <p role="status">저장소를 확인하고 있습니다…</p>}
    {error && <p role="alert">{error}</p>}
    <button type="button" disabled={loading || blocked} onClick={() => void start()}>{hasExisting ? "현재 저장소로 계속" : error ? "다시 시도" : "새 저장소로 시작"}</button>
    <button type="button" disabled={loading || blocked} onClick={() => setShowImport(true)}>기존 앱 데이터 가져오기</button>
    {canReconnect && <button type="button" disabled={loading || blocked} onClick={() => setReconnect(true)}>다른 노트 폴더 선택</button>}
  </section>;
}
