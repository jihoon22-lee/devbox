import { useEffect, useRef, useState } from "react";
import { restoreFocus, trapDialogKeyDown } from "@devbox/a11y";
import { draftError, exportDraft, saveDraft, type DraftOwner, type KnowledgeDraft } from "./api";
import type { OutputSource } from "../transforms/tools/outputPolicy";
import "./knowledge.css";

export function KnowledgeDraftAction({ value, owner, source, disabled = false }: {
  value: string; owner: DraftOwner; source?: OutputSource; disabled?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [saved, setSaved] = useState<KnowledgeDraft | null>(null);
  const [error, setError] = useState("");
  const mounted = useRef(false); const revision = useRef(0); const running = useRef(false);
  const dialog = useRef<HTMLDivElement>(null); const cancel = useRef<HTMLButtonElement>(null);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; revision.current += 1; }; }, []);
  useEffect(() => { revision.current += 1; setOpen(false); setSaved(null); setError(""); }, [value]);
  useEffect(() => {
    if (!open) return;
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    cancel.current?.focus();
    return () => { restoreFocus(previous); };
  }, [open]);
  const save = async () => {
    if (running.current) return;
    running.current = true; setBusy(true); setError(""); const version = ++revision.current;
    try {
      const draft = await saveDraft(owner, value, source);
      if (mounted.current && revision.current === version) { setSaved(draft); setOpen(false); }
    } catch (error) {
      if (mounted.current && revision.current === version) setError(draftError(error));
    } finally { running.current = false; if (mounted.current) setBusy(false); }
  };
  if (!value) return null;
  return <div className="studio-knowledge-action">
    <button className="copy-btn" disabled={disabled || busy} onClick={() => { setOpen(true); setError(""); }}>Knowledge 초안 보관</button>
    {saved && <div role="status"><p>Knowledge 수신 연결을 사용할 수 없어 API Studio에 초안을 보관했습니다. History & Console에서 다시 열 수 있습니다.{saved.redacted ? " 민감한 값은 마스킹되었습니다." : ""}</p>
      <button className="copy-btn" onClick={() => { try { exportDraft(saved); } catch { setError("보관한 초안을 내보내지 못했습니다."); } }}>보관한 초안 내보내기</button></div>}
    {error && !open && <p role="alert">{error}</p>}
    {open && <div className="studio-knowledge-backdrop"><div ref={dialog} className="studio-knowledge-dialog" role="dialog" aria-modal="true" aria-label="Knowledge 초안 보관 확인" aria-busy={busy}
      onKeyDown={event => { if (dialog.current) trapDialogKeyDown(event, dialog.current, () => { if (!running.current) setOpen(false); }); }}>
      <h2>Knowledge 초안 보관</h2><p>현재 결과를 확인하세요. 마스킹한 사본을 API Studio에 보관합니다. Knowledge 수신 연결이 준비되기 전에는 이곳에서 결과를 다시 열고 내보낼 수 있습니다.</p>
      <pre>{value}</pre>{error && <p role="alert">{error}</p>}
      <div><button ref={cancel} className="btn" disabled={busy} onClick={() => setOpen(false)}>취소</button><button className="btn" disabled={busy || new TextEncoder().encode(value).length > 512 * 1024} onClick={() => void save()}>{busy ? "보관 중…" : "마스킹 사본 보관"}</button></div>
    </div></div>}
  </div>;
}
