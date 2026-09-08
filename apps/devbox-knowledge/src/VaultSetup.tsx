import { useEffect, useRef, useState } from "react";
import { vaultInvoke, vaultJob, type VaultPreview, type VaultSchedule } from "./vaultApi";
export default function VaultSetup({ onActivated }: { onActivated: () => void }) {
  const [schedule, setSchedule] = useState<VaultSchedule | null>(null);
  const [preview, setPreview] = useState<VaultPreview | null>(null);
  const [busy, setBusy] = useState(true);
  const [committed, setCommitted] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const current = useRef<AbortController | null>(null);
  const alive = useRef(true);
  const previewRef = useRef<VaultPreview | null>(null);
  useEffect(() => {
    alive.current = true;
    void vaultInvoke<{ schedule: VaultSchedule | null }>("vault_change_status").then(value => { if (alive.current) setSchedule(value.schedule); })
      .catch(error => { if (alive.current) setError(error instanceof Error ? error.message : "폴더 변경 정보를 확인하지 못했습니다."); })
      .finally(() => { if (alive.current) setBusy(false); });
    return () => {
      alive.current = false; current.current?.abort();
      if (previewRef.current) void vaultInvoke("discard_vault_preview", { id: previewRef.current.previewId }).catch(() => undefined);
    };
  }, []);
  const run = async (apply = false) => {
    if (busy || (apply && !preview)) return;
    current.current?.abort(); const controller = new AbortController(); current.current = controller;
    setBusy(true); setError(null);
    try {
      if (apply) {
        const result = await vaultJob<{ active: boolean }>("apply_vault_change", { id: preview!.previewId }, controller.signal, () => { if (alive.current) setCommitted(true); });
        if (alive.current && !controller.signal.aborted && result.active) { previewRef.current = null; onActivated(); }
      } else {
        const result = await vaultJob<VaultPreview>("prepare_vault_change", {}, controller.signal, () => undefined);
        if (alive.current && !controller.signal.aborted) { previewRef.current = result; setPreview(result); }
      }
    } catch (error) { if (alive.current && !controller.signal.aborted) setError(error instanceof Error ? error.message : "폴더 연결을 완료하지 못했습니다."); }
    finally { if (alive.current && current.current === controller) setBusy(false); }
  };
  const keep = async () => {
    if (committed) return;
    current.current?.abort(); setBusy(true); setError(null);
    try {
      await vaultInvoke("cancel_vault_change"); previewRef.current = null;
      const result = await vaultInvoke<{ active: boolean }>("continue_existing");
      if (alive.current && result.active) onActivated();
    } catch (error) { if (alive.current) setError(error instanceof Error ? error.message : "현재 폴더로 계속하지 못했습니다."); }
    finally { if (alive.current) setBusy(false); }
  };
  return <section className="knowledge-migration" aria-labelledby="vault-setup-title">
    <h2 id="vault-setup-title">노트 폴더 변경 확인</h2>
    <p className="knowledge-migration-path">현재 폴더: {schedule?.previousRoot ?? "확인 중…"}</p>
    <p className="knowledge-migration-path">선택한 폴더: {schedule?.target ?? "확인 중…"}</p>
    <p>미리보기는 폴더를 읽기만 합니다. 적용하면 필요한 기본 폴더를 만들고 노트 연결을 바꾸며, 기존 파일과 템플릿은 유지합니다.</p>
    <p>이전 Knowledge 앱이 설치되어 있다면 완전히 종료한 뒤 확인해 주세요.</p>
    {busy && <p role="status">{committed ? "폴더 연결을 저장했습니다. 화면을 준비하고 있습니다…" : "노트 폴더를 확인하고 있습니다…"}</p>}
    {preview && <div aria-labelledby="vault-preview-title"><h3 id="vault-preview-title">연결 미리보기</h3><p>{preview.target}</p>
      <p>{preview.alreadyApplied ? "이 변경은 저장되어 있습니다. 다시 확인하여 시작을 마무리합니다." : "폴더와 소유권을 확인했습니다. 5분 안에 적용해 주세요."}</p>
      <button type="button" disabled={busy || committed} onClick={() => void run(true)}>이 폴더로 변경하고 시작</button></div>}
    {error && <p role="alert">{error}</p>}
    {committed && error && <p>폴더 연결은 저장되었습니다. 앱을 다시 시작해 마무리해 주세요.</p>}
    <div className="migration-actions"><button type="button" disabled={busy || committed || !schedule} onClick={() => void run()}>폴더 연결 미리보기</button>
      <button type="button" disabled={busy || committed} onClick={() => void keep()}>현재 폴더 유지하고 계속</button></div>
  </section>;
}
