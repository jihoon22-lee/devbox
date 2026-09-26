import { localDateKey } from "./dates";
import { useEffect, useRef, useState } from "react";
import { notesCall } from "@devbox/knowledge-features/notes/api";
import { useUndo } from "@devbox/product-shell/undo";
import { undoCreated, useNoteSession } from "@devbox/knowledge-features/notes-lifecycle";
import { nativeMode } from "@devbox/product-shell/api";

interface Props {
  date: string;
  onDateChange: (date: string) => void;
  onOpen: (path: string) => void;
  onActivity: () => void;
}
export default function Daily({ date, onDateChange, onOpen, onActivity }: Props) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const mounted = useRef(false);
  const busyRef = useRef(false);
  const session = useNoteSession();
  const { offer, toast } = useUndo();
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  const open = async () => {
    if (busyRef.current || !nativeMode) return;
    busyRef.current = true;
    setBusy(true);
    setError(null);
    try {
      const result = await notesCall("open_daily", { date });
      if (result.created) {
        offer(
          result.indexed ? "노트를 만들었습니다." : "노트를 만들었습니다. 검색 색인은 다시 확인해야 합니다.",
          async () => {
            await undoCreated(result, session, (path, revision) => notesCall("undo_created_note", { path, revision }));
          },
        );
      }
      if (mounted.current) onOpen(result.path);
    } catch (cause) {
      if (mounted.current) setError(cause instanceof Error ? cause.message : "일일 노트를 열지 못했습니다.");
    } finally {
      busyRef.current = false;
      if (mounted.current) setBusy(false);
    }
  };
  return (
    <section className="knowledge-daily" aria-labelledby="knowledge-daily-title">
      <h2 id="knowledge-daily-title">일일 기록</h2>
      <div className="knowledge-daily-actions">
        <label>
          날짜{" "}
          <input
            type="date"
            aria-label="일일 기록 날짜"
            value={date}
            min="0001-01-01"
            max="9999-12-31"
            disabled={busy}
            onChange={(event) => {
              if (event.currentTarget.validity.valid && event.currentTarget.value)
                onDateChange(event.currentTarget.value);
            }}
          />
        </label>
        <button type="button" disabled={busy} onClick={() => onDateChange(localDateKey())}>
          오늘
        </button>
        <button type="button" disabled={busy} onClick={onActivity}>
          이 날짜의 활동
        </button>
      </div>
      <p>시스템 현지 날짜 기준입니다. 날짜를 선택해도 노트를 자동으로 만들지 않습니다.</p>
      <button type="button" disabled={busy || !nativeMode} onClick={() => void open()}>
        일일 노트 열기
      </button>
      {busy && <p role="status">일일 노트를 확인하고 있습니다…</p>}
      {error && <p role="alert">{error}</p>}
      {!nativeMode && <p>브라우저 미리보기입니다. 실제 파일을 열거나 저장하려면 데스크톱 앱을 사용하세요.</p>}
      {toast}
    </section>
  );
}
