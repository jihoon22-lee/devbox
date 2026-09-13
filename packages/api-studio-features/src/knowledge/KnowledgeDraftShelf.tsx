import { useEffect, useRef, useState } from "react";
import { deleteDraft, draftError, exportDraft, getDraft, listDrafts, type DraftOwner, type DraftSummary, type KnowledgeDraft } from "./api";
import "./knowledge.css";
import {SendDraft} from "./SendDraft";
const OWNERS: DraftOwner[] = ["api-studio.api", "api-studio.transforms"];
export function KnowledgeDraftShelf() {
  const [items, setItems] = useState<DraftSummary[]>([]);
  const [selected, setSelected] = useState<KnowledgeDraft | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [busy, setBusy] = useState(false); const running = useRef(false);
  const [error, setError] = useState(""); const mounted = useRef(false);
  const refreshRevision = useRef(0);
  const native = "__TAURI_INTERNALS__" in window;
  const refresh = async () => {
    const revision = ++refreshRevision.current;
    const groups = await Promise.all(OWNERS.map(listDrafts));
    if (mounted.current && refreshRevision.current === revision) setItems(groups.flat().sort((a, b) => b.createdAtMs - a.createdAtMs));
  };
  useEffect(() => {
    let current = true;
    mounted.current = true;
    if (native) void refresh().catch(error => { if (current && mounted.current) setError(draftError(error)); });
    return () => { current = false; mounted.current = false; refreshRevision.current += 1; };
  }, [native]);
  const run = async (action: () => Promise<void>) => {
    if (running.current) return;
    running.current = true; setBusy(true); setError("");
    try { await action(); } catch (error) { if (mounted.current) setError(draftError(error)); }
    finally { running.current = false; if (mounted.current) setBusy(false); }
  };
  return <section className="studio-knowledge-shelf" aria-label="보관한 Knowledge 초안">
    <h2>보관한 Knowledge 초안</h2>
    <p>명시적으로 보관한 마스킹 사본입니다. Requests와 Transforms가 각각 소유하며, 직접 삭제할 때까지 유지됩니다.</p>
    {!native ? <p>초안 보관함은 데스크톱 앱에서 사용할 수 있습니다.</p> : <>
      <button className="btn" disabled={busy} onClick={() => void run(refresh)}>보관함 새로고침</button>
      {error && <p role="alert">{error}</p>}
      <div className="studio-knowledge-items">{items.map(item => <button className="btn" key={item.artifact.id} disabled={busy}
        aria-pressed={selected?.artifact.id === item.artifact.id} onClick={() => void run(async () => {
          const draft = await getDraft(item.artifact.provenance.component, item.artifact.id);
          if (mounted.current) { setSelected(draft); setConfirmDelete(false); }
        })}>{item.title} · {new Date(item.createdAtMs).toLocaleString()}</button>)}</div>
      {!items.length && !error && <p>보관한 초안이 없습니다.</p>}
      {selected && <div aria-label="보관한 초안 미리보기"><h3>{selected.title}</h3><pre>{selected.body}</pre>
        <SendDraft key={selected.artifact.id} draft={selected} disabled={busy}/><button className="btn" disabled={busy} onClick={() => void run(async () => { exportDraft(selected); })}>초안 내보내기</button>
        {confirmDelete ? <><p>이 보관 사본을 삭제합니다. 내보낸 파일에는 영향을 주지 않습니다.</p>
          <button className="btn" disabled={busy} onClick={() => setConfirmDelete(false)}>유지</button>
          <button className="btn" disabled={busy} onClick={() => void run(async () => {
            await deleteDraft(selected.artifact.provenance.component, selected.artifact.id);
            if (mounted.current) { setSelected(null); setConfirmDelete(false); }
            await refresh();
          })}>보관 사본 삭제 확인</button></> : <button className="btn" disabled={busy} onClick={() => setConfirmDelete(true)}>초안 삭제</button>}
      </div>}
    </>}
  </section>;
}
