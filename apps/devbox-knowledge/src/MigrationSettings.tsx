import { useEffect, useState } from "react";
import { nativeMode } from "@devbox/product-shell/api";
import { componentInvoke } from "@devbox/knowledge-features/transport";
const invoke = componentInvoke("knowledge.migration");
export default function MigrationSettings() {
  const [scheduled, setScheduled] = useState(false);
  const [busy, setBusy] = useState(nativeMode);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    if (!nativeMode) return;
    let alive = true;
    void invoke<{ scheduled: boolean }>("status").then(value => { if (alive) setScheduled(value.scheduled === true); })
      .catch(error => { if (alive) setError(error instanceof Error ? error.message : "가져오기 설정을 확인하지 못했습니다."); })
      .finally(() => { if (alive) setBusy(false); });
    return () => { alive = false; };
  }, []);
  const change = async () => {
    setBusy(true); setError(null);
    try { const value = await invoke<{ scheduled: boolean }>(scheduled ? "cancel_scheduled_import" : "schedule_import"); setScheduled(value.scheduled === true); }
    catch (error) { setError(error instanceof Error ? error.message : "가져오기 설정을 저장하지 못했습니다."); }
    finally { setBusy(false); }
  };
  return <section aria-labelledby="migration-settings-title" className="knowledge-migration">
    <h2 id="migration-settings-title">기존 앱 데이터 가져오기</h2>
    <p>이전 Knowledge의 템플릿·설정, Life Log 활동 이력, Everything+의 검색 위치·저장 검색을 선택해 가져올 수 있습니다.</p>
    <p>열어 둔 노트를 저장하고 앱을 다시 시작하면 가져오기 미리보기를 확인합니다. 원본 파일은 이동하지 않으며, 현재 노트 저장소와 충돌하는 설정은 유지합니다.</p>
    {scheduled && <p role="status">다음 시작에서 가져오기를 검토합니다. 지금은 계속 작업할 수 있습니다.</p>}
    {!nativeMode && <p>데스크톱 앱에서 사용할 수 있습니다.</p>}
    {error && <p role="alert">{error}</p>}
    <button type="button" disabled={!nativeMode || busy} onClick={() => void change()}>{scheduled ? "다음 시작의 가져오기 취소" : "다음 시작에서 가져오기 검토"}</button>
  </section>;
}
