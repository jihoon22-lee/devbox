import { ContextMenu, useContextMenu, type ContextMenuEntry } from "@devbox/context-menu";
import { displayNameForPath, panelIdForDoc, tabIdForDoc, type Doc, type DocId, type ViewId } from "../types";
import { useCallback, useEffect, useMemo, useState, type KeyboardEvent } from "react";

export type TabContextAction =
  | "close"
  | "close-others"
  | "close-right"
  | "copy-path"
  | "reveal"
  | "rename"
  | "delete";

interface TabBarProps {
  view: ViewId;
  docs: Doc[];
  docIds: DocId[];
  activeDocId: DocId | null;
  onActivate: (docId: DocId) => void;
  onClose: (docId: DocId) => void;
  onMove: (docId: DocId) => void;
  onContextAction: (view: ViewId, docId: DocId, action: TabContextAction) => void;
  disabled?: boolean;
}

export default function TabBar({
  view,
  docs,
  docIds,
  activeDocId,
  onActivate,
  onClose,
  onMove,
  onContextAction,
  disabled = false,
}: TabBarProps) {
  const docsById = new Map(docs.map((doc) => [doc.id, doc]));
  const visibleDocIds = docIds.filter((docId) => docsById.has(docId));
  const rovingDocId = visibleDocIds.includes(activeDocId ?? "") ? activeDocId : visibleDocIds[0] ?? null;
  const [contextDocId, setContextDocId] = useState<DocId | null>(null);
  const prepareContext = useCallback((_reason: "pointer" | "keyboard", target: HTMLElement) => {
    const docId = target.dataset.docId;
    if (!docId || !visibleDocIds.includes(docId)) return;
    setContextDocId(docId);
    onActivate(docId);
  }, [onActivate, visibleDocIds]);
  const contextMenu = useContextMenu({ disabled, onBeforeOpen: prepareContext });

  useEffect(() => {
    if (disabled || (contextDocId && !visibleDocIds.includes(contextDocId))) {
      contextMenu.close();
      setContextDocId(null);
    }
  }, [contextDocId, contextMenu.close, disabled, visibleDocIds]);

  const contextItems = useMemo<readonly ContextMenuEntry[]>(() => {
    const index = contextDocId ? visibleDocIds.indexOf(contextDocId) : -1;
    return [
      { type: "item", id: "close", label: "닫기" },
      { type: "item", id: "close-others", label: "다른 탭 닫기", disabled: visibleDocIds.length <= 1 },
      { type: "item", id: "close-right", label: "오른쪽 탭 모두 닫기", disabled: index < 0 || index === visibleDocIds.length - 1 },
      { type: "separator", id: "path-separator" },
      { type: "item", id: "copy-path", label: "경로 복사" },
      { type: "item", id: "reveal", label: "탐색기에서 열기" },
      { type: "separator", id: "mutation-separator" },
      { type: "item", id: "rename", label: "이름 변경" },
      { type: "item", id: "delete", label: "삭제", danger: true },
    ];
  }, [contextDocId, visibleDocIds]);

  const focusTab = (docId: DocId) => {
    document.getElementById(tabIdForDoc(docId))?.focus();
  };

  const handleTabKeyDown = (event: KeyboardEvent<HTMLButtonElement>, docId: DocId) => {
    contextMenu.triggerProps.onKeyDown?.(event);
    if (event.defaultPrevented) return;
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
    <>
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
              data-doc-id={doc.id}
              onClick={() => onActivate(doc.id)}
              onKeyDown={(event) => handleTabKeyDown(event, doc.id)}
              onContextMenu={contextMenu.triggerProps.onContextMenu}
              aria-haspopup="menu"
              aria-expanded={contextMenu.open && contextDocId === doc.id}
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
      <ContextMenu
        open={contextMenu.open}
        anchor={contextMenu.anchor}
        items={contextItems}
        onSelect={(id) => {
          if (contextDocId) onContextAction(view, contextDocId, id as TabContextAction);
        }}
        onClose={contextMenu.close}
        restoreFocusTo={contextMenu.restoreFocusTo}
        ariaLabel="문서 탭 작업"
      />
    </>
  );
}
