import { displayNameForPath, panelIdForDoc, tabIdForDoc, type Doc, type DocId, type ViewId } from "../types";
import type { KeyboardEvent } from "react";

interface TabBarProps {
  view: ViewId;
  docs: Doc[];
  docIds: DocId[];
  activeDocId: DocId | null;
  onActivate: (docId: DocId) => void;
  onClose: (docId: DocId) => void;
  onMove: (docId: DocId) => void;
}

export default function TabBar({
  view,
  docs,
  docIds,
  activeDocId,
  onActivate,
  onClose,
  onMove,
}: TabBarProps) {
  const docsById = new Map(docs.map((doc) => [doc.id, doc]));
  const visibleDocIds = docIds.filter((docId) => docsById.has(docId));
  const rovingDocId = visibleDocIds.includes(activeDocId ?? "") ? activeDocId : visibleDocIds[0] ?? null;

  const focusTab = (docId: DocId) => {
    document.getElementById(tabIdForDoc(docId))?.focus();
  };

  const handleTabKeyDown = (event: KeyboardEvent<HTMLButtonElement>, docId: DocId) => {
    const index = visibleDocIds.indexOf(docId);
    if (index < 0) return;
    let nextIndex: number | null = null;
    if (event.key === "ArrowRight" || event.key === "ArrowDown") {
      nextIndex = (index + 1) % visibleDocIds.length;
    } else if (event.key === "ArrowLeft" || event.key === "ArrowUp") {
      nextIndex = (index - 1 + visibleDocIds.length) % visibleDocIds.length;
    } else if (event.key === "Home") {
      nextIndex = 0;
    } else if (event.key === "End") {
      nextIndex = visibleDocIds.length - 1;
    } else if (event.key === "Delete" || event.key === "Backspace") {
      event.preventDefault();
      onClose(docId);
      return;
    }
    if (nextIndex === null || nextIndex < 0) return;
    event.preventDefault();
    const nextDocId = visibleDocIds[nextIndex];
    onActivate(nextDocId);
    focusTab(nextDocId);
  };

  return (
    <div
      className="tab-bar"
      role="tablist"
      aria-label={`${view + 1}번 뷰 문서 탭`}
      aria-orientation="horizontal"
    >
      {visibleDocIds.map((docId, index) => {
        const doc = docsById.get(docId);
        if (!doc) return null;
        const active = doc.id === activeDocId;
        return (
          <div className={`document-tab ${active ? "active" : ""}`} key={doc.id} role="presentation">
            <button
              type="button"
              role="tab"
              id={tabIdForDoc(doc.id)}
              aria-selected={active}
              aria-controls={panelIdForDoc(doc.id)}
              aria-posinset={index + 1}
              aria-setsize={visibleDocIds.length}
              tabIndex={doc.id === rovingDocId ? 0 : -1}
              className="document-tab-select"
              onClick={() => onActivate(doc.id)}
              onKeyDown={(event) => handleTabKeyDown(event, doc.id)}
              title={doc.path}
            >
              <span className="document-tab-name">
                {doc.dirty ? "● " : ""}
                {doc.readOnly ? "🔒 " : ""}
                {displayNameForPath(doc.path)}
              </span>
            </button>
            <button
              type="button"
              className="tab-action"
              aria-label={`${doc.path} 닫기`}
              onClick={() => onClose(doc.id)}
            >
              ×
            </button>
            <button
              type="button"
              className="tab-action tab-move"
              aria-label={`${doc.path} ${view === 0 ? "2번" : "1번"} 뷰로 이동`}
              title={`${view === 0 ? "2번" : "1번"} 뷰로 이동`}
              onClick={() => onMove(doc.id)}
            >
              {view === 0 ? "⇥" : "⇤"}
            </button>
          </div>
        );
      })}
      {visibleDocIds.length === 0 && <span className="tab-empty">열린 파일 없음</span>}
    </div>
  );
}
