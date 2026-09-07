import { useEffect, useRef } from "react";
import { focusFirst, restoreFocus, trapDialogKeyDown } from "@devbox/a11y";
import type { PersistedHistoryRequest } from "./types";
import { sanitizeRequestForPersistence } from "./lib/persistence";
interface Props { title: string; request: PersistedHistoryRequest; canApply: boolean; onClose: () => void; onApply: () => void }
export function SavedRequestPreview({ title, request, canApply, onClose, onApply }: Props) {
  const dialog = useRef<HTMLElement>(null);
  const close = useRef(onClose); close.current = onClose;
  useEffect(() => {
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    if (dialog.current) focusFirst(dialog.current);
    return () => { restoreFocus(previous); };
  }, []);
  const preview = JSON.stringify(sanitizeRequestForPersistence(request), null, 2);
  return <div className="handoff-backdrop"><section ref={dialog} className="handoff-dialog" role="dialog" aria-modal="true" aria-labelledby="saved-request-preview-title"
    onKeyDown={event => { if (dialog.current) trapDialogKeyDown(event, dialog.current, () => close.current()); }}>
    <h2 id="saved-request-preview-title">저장된 요청 미리보기</h2><p>{title}</p>
    <p>열린 초안은 유지됩니다. 아래 버튼을 누르면 이 요청으로 바뀌며 자동 전송하지 않습니다.</p>
    <pre className="handoff-body-preview">{preview.slice(0, 32768)}</pre>
    {preview.length > 32768 && <p>긴 요청은 앞부분만 표시합니다.</p>}
    <div className="handoff-dialog-actions"><button className="btn" onClick={onClose}>닫기</button><button className="btn" disabled={!canApply} onClick={onApply}>현재 초안 대신 열기</button></div>
  </section></div>;
}
