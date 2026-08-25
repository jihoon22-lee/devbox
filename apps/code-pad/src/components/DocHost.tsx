import type { CSSProperties } from "react";
import type { CompletionSource } from "@codemirror/autocomplete";
import type { Diagnostic } from "@codemirror/lint";
import type { HoverTooltipSource } from "@codemirror/view";
import { tabIdForDoc, type Doc, type DocId, type ViewId } from "../types";
import CodeEditor from "../editor/CodeEditor";
import type { BookmarkCommands } from "../editor/bookmarks";

interface DocHostProps {
  docs: Doc[];
  views: [DocId[], DocId[]];
  activeDocByView: [DocId | null, DocId | null];
  split: boolean;
  fontSize: number;
  onChange: (docId: DocId, text: string) => void;
  onCursorChange?: (docId: DocId, cursor: number) => void;
  onBookmarksChange?: (docId: DocId, bookmarks: number[]) => void;
  onFocusDoc: (view: ViewId, docId: DocId) => void;
  onReplaceCommandReady?: (docId: DocId, command: (() => boolean) | null) => void;
  onBookmarkCommandsReady?: (docId: DocId, commands: BookmarkCommands | null) => void;
  diagnostics?: (docId: string) => Diagnostic[];
  completionSource?: (docId: string) => CompletionSource | undefined;
  hoverSource?: (docId: string) => HoverTooltipSource | undefined;
  canNavigate?: (docId: DocId, kind: "definition" | "references") => boolean;
  navigationBusy?: boolean;
  onNavigate?: (docId: DocId, kind: "definition" | "references", cursor: number) => void;
  onError?: (message: string | null) => void;
}

function placementForDoc(
  docId: DocId,
  views: [DocId[], DocId[]],
  activeDocByView: [DocId | null, DocId | null],
  split: boolean,
): { view: ViewId; style: CSSProperties } | null {
  for (const view of [0, 1] as const) {
    const tabIndex = views[view].indexOf(docId);
    if (tabIndex === -1) continue;
    const visible = (split || view === 0) && activeDocByView[view] === docId;
    return {
      view,
      style: visible
        ? {
            gridColumn: view + 1,
            gridRow: 1,
            order: tabIndex,
            minWidth: 0,
            minHeight: 0,
          }
        : { display: "none" },
    };
  }
  return null;
}

/**
 * Stable parent for every CodeEditor. This component must map the append-only
 * `docs` registry directly; views only affect CSS placement/display.
 */
export default function DocHost({
  docs,
  views,
  activeDocByView,
  split,
  fontSize,
  onChange,
  onCursorChange,
  onBookmarksChange,
  onFocusDoc,
  onReplaceCommandReady,
  onBookmarkCommandsReady,
  diagnostics,
  completionSource,
  hoverSource,
  canNavigate,
  navigationBusy,
  onNavigate,
  onError,
}: DocHostProps) {
  return (
    <div
      className={`doc-host ${split ? "split" : "single"}`}
      style={{ gridTemplateColumns: split ? "repeat(2, minmax(0, 1fr))" : "minmax(0, 1fr)" }}
      data-doc-order={docs.map((doc) => doc.id).join(",")}
    >
      {docs.map((doc) => {
        const placement = placementForDoc(doc.id, views, activeDocByView, split);
        const view = placement?.view ?? 0;
        return (
          <CodeEditor
            key={doc.id}
            docId={doc.id}
            path={doc.path}
            value={doc.text}
            readOnly={doc.readOnly}
            syntaxHighlightingEnabled={!doc.readOnly || doc.size <= 5 * 1024 * 1024}
            fontSize={fontSize}
            style={placement?.style ?? { display: "none" }}
            visible={placement?.style.display !== "none"}
            tabId={tabIdForDoc(doc.id)}
            onChange={(text) => onChange(doc.id, text)}
            cursor={doc.cursor}
            bookmarks={doc.bookmarks}
            onCursorChange={(cursor) => onCursorChange?.(doc.id, cursor)}
            onBookmarksChange={(bookmarks) => onBookmarksChange?.(doc.id, bookmarks)}
            onFocus={() => onFocusDoc(view, doc.id)}
            onReplaceCommandReady={onReplaceCommandReady}
            onBookmarkCommandsReady={onBookmarkCommandsReady}
            diagnostics={diagnostics?.(doc.id)}
            completionSource={completionSource?.(doc.id)}
            hoverSource={hoverSource?.(doc.id)}
            canGoToDefinition={canNavigate?.(doc.id, "definition")}
            canFindReferences={canNavigate?.(doc.id, "references")}
            navigationBusy={navigationBusy}
            onNavigate={onNavigate}
            onError={onError}
          />
        );
      })}
      {docs.length === 0 && <div className="doc-host-empty">파일을 열어 시작하세요</div>}
    </div>
  );
}

export { placementForDoc };
