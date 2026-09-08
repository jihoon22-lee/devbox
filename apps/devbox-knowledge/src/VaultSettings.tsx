import { useEffect, useState } from "react";
import { nativeMode } from "@devbox/product-shell/api";
import { vaultInvoke, type VaultSchedule } from "./vaultApi";
export default function VaultSettings({ onScheduled }: { onScheduled?: () => void } = {}) {
  const [path, setPath] = useState("");
  const [schedule, setSchedule] = useState<VaultSchedule | null>(null);
  const [busy, setBusy] = useState(nativeMode);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    if (!nativeMode) return;
    let alive = true;
    void vaultInvoke<{ schedule: VaultSchedule | null }>("vault_change_status").then(value => { if (alive) { setSchedule(value.schedule); setPath(value.schedule?.target ?? ""); } })
      .catch(error => { if (alive) setError(error instanceof Error ? error.message : "노트 폴더 설정을 확인하지 못했습니다."); })
      .finally(() => { if (alive) setBusy(false); });
    return () => { alive = false; };
  }, []);
  const change = async (cancel = false) => {
    setBusy(true); setError(null);
    try { const result = await vaultInvoke<{ schedule: VaultSchedule | null }>(cancel ? "cancel_vault_change" : "schedule_vault_change", cancel ? {} : { path }); setSchedule(result.schedule); if (result.schedule) onScheduled?.(); }
    catch (error) { setError(error instanceof Error ? error.message : "노트 폴더 변경을 예약하지 못했습니다."); }
    finally { setBusy(false); }
  };
  return <section className="knowledge-migration" aria-labelledby="vault-settings-title">
    <h2 id="vault-settings-title">노트 폴더 연결</h2>
    <p>{onScheduled ? "기존 폴더를 확인한 뒤 연결을 변경합니다." : "다음 시작에서 기존 폴더를 확인하고 연결을 변경합니다. 현재 노트를 저장한 뒤 앱을 다시 시작해 주세요."}</p>
    <p>기존 파일을 옮기거나 삭제하지 않습니다. 템플릿과 설정은 유지하며, 새 폴더의 노트를 다시 색인합니다.</p>
    <label className="knowledge-vault-choice">연결할 폴더 <input aria-label="연결할 노트 폴더" value={path} maxLength={4096} disabled={busy || !nativeMode} onChange={event => setPath(event.currentTarget.value)} placeholder={"C:\\Notes 또는 WSL 폴더의 UNC 경로"}/></label>
    {schedule && <p role="status">다음 시작에서 확인할 폴더: {schedule.target}</p>}
    {!nativeMode && <p>데스크톱 앱에서 사용할 수 있습니다.</p>}
    {error && <p role="alert">{error}</p>}
    <div className="migration-actions"><button type="button" disabled={!nativeMode || busy || !path.trim()} onClick={() => void change()}>{onScheduled ? "선택한 폴더 확인" : "다음 시작에서 폴더 확인"}</button>
      {schedule && <button type="button" disabled={busy} onClick={() => void change(true)}>폴더 변경 예약 취소</button>}</div>
  </section>;
}
