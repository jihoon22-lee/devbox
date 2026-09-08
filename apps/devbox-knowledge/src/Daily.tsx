import { localDateKey } from "./dates";
import { useEffect, useRef, useState } from "react";
import { componentInvoke } from "@devbox/knowledge-features/transport";
import { nativeMode } from "@devbox/product-shell/api";
const invoke = componentInvoke("knowledge.notes");
interface Preview { path: string; content: string; previewId: string | null; exists: boolean }
interface Props { date: string; onDateChange: (date: string) => void; onOpen: (path: string) => void; onActivity: () => void }
export default function Daily({ date, onDateChange, onOpen, onActivity }: Props) {
  const [preview, setPreview] = useState<Preview | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const request = useRef(0);
  const mounted = useRef(false);
  const pending = useRef<string | null>(null);
  const busyRef = useRef(false);
  const discard = () => {
    const id = pending.current;
    pending.current = null;
    if (id && nativeMode) void invoke("discard_daily", { previewId: id }).catch(() => undefined);
  };
  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; request.current++; discard(); };
  }, []);
  useEffect(() => {
    request.current++; discard(); setPreview(null); setError(null); setNotice(null);
  }, [date]);
  const prepare = async () => {
    if (busyRef.current) return;
    discard(); setPreview(null); setError(null); setNotice(null);
    busyRef.current = true; setBusy(true);
    const generation = ++request.current;
    try {
      const value = nativeMode ? await invoke<Preview>("preview_daily", { date }) : {
        path: `Journal/${date}.md`, content: `---\ntags: [daily]\n---\n\n# ${date}\n\n`, previewId: null, exists: false,
      };
      if (!mounted.current || generation !== request.current) {
        if (value.previewId && nativeMode) void invoke("discard_daily", { previewId: value.previewId }).catch(() => undefined);
        return;
      }
      pending.current = value.previewId;
      setPreview(value);
    } catch (error) {
      if (mounted.current && generation === request.current) setError(error instanceof Error ? error.message : "일일 노트를 확인하지 못했습니다.");
    } finally {
      busyRef.current = false;
      if (mounted.current) setBusy(false);
    }
  };
  const save = async () => {
    const id = pending.current;
    if (!id || !nativeMode || busyRef.current) return;
    pending.current = null; busyRef.current = true; setBusy(true); setError(null);
    const generation = ++request.current;
    try {
      const saved = await invoke<{ path: string; indexed: boolean }>("save_daily", { previewId: id });
      if (!mounted.current || generation !== request.current) return;
      setPreview({ path: saved.path, content: "", previewId: null, exists: true });
      setNotice(saved.indexed ? "일일 노트를 만들었습니다." : "일일 노트를 저장했습니다. 검색 색인은 다시 확인해야 합니다.");
    } catch (error) {
      if (mounted.current && generation === request.current) {
        setPreview(null);
        setError(error instanceof Error ? error.message : "노트를 만들지 못했습니다. 다시 확인해 주세요.");
      }
    } finally {
      busyRef.current = false;
      if (mounted.current) setBusy(false);
    }
  };
  return <section className="knowledge-daily" aria-labelledby="knowledge-daily-title">
    <h2 id="knowledge-daily-title">일일 기록</h2>
    <div className="knowledge-daily-actions">
      <label>날짜 <input type="date" aria-label="일일 기록 날짜" value={date} min="0001-01-01" max="9999-12-31" disabled={busy} onChange={event => { if (event.currentTarget.validity.valid && event.currentTarget.value) onDateChange(event.currentTarget.value); }}/></label>
      <button type="button" disabled={busy} onClick={() => onDateChange(localDateKey())}>오늘</button>
      <button type="button" disabled={busy} onClick={onActivity}>이 날짜의 활동</button>
    </div>
    <p>시스템 현지 날짜 기준입니다. 날짜를 선택해도 노트를 자동으로 만들지 않습니다.</p>
    <button type="button" disabled={busy} onClick={() => void prepare()}>일일 노트 확인</button>
    {busy && <p role="status">일일 노트를 확인하고 있습니다…</p>}
    {error && <p role="alert">{error}</p>}
    {notice && <p role="status">{notice}</p>}
    {preview && <div className="knowledge-daily-preview">
      <p>{preview.path}</p>
      {preview.exists ? <>
        <p>저장된 노트가 있습니다. 기존 내용을 열어 확인할 수 있습니다.</p>
        <button type="button" disabled={busy} onClick={() => onOpen(preview.path)}>노트 열기</button>
      </> : <>
        <h3>새 노트 미리보기</h3><pre>{preview.content}</pre>
        {!nativeMode && <p>브라우저 미리보기입니다. 실제 파일을 저장하려면 데스크톱 앱을 사용하세요.</p>}
        <button type="button" disabled={busy || !nativeMode || !preview.previewId} onClick={() => void save()}>확인 후 새 노트 만들기</button>
        <button type="button" disabled={busy} onClick={() => { request.current++; discard(); setPreview(null); }}>취소</button>
      </>}
    </div>}
  </section>;
}
