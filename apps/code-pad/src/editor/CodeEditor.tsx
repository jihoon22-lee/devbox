import { useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import { ContextMenu, useContextMenu, type ContextMenuEntry } from "@devbox/context-menu";
import { Compartment, EditorSelection, EditorState, Transaction } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import type { HoverTooltipSource } from "@codemirror/view";
import { setDiagnostics } from "@codemirror/lint";
import type { Diagnostic } from "@codemirror/lint";
import type { CompletionSource } from "@codemirror/autocomplete";
import { openSearchPanel } from "@codemirror/search";
import {
  editorExtensions,
  externalValueSync,
  languageExtensionFor,
  languageForPath,
  readOnlyExtension,
  syntaxHighlightingExtension,
  currentDocumentWordCompletion,
  type EditorCompartments,
} from "./extensions";
import { panelIdForDoc } from "../types";
import {
  bookmarkField,
  nextBookmark,
  previousBookmark,
  setBookmarkLines,
  toggleBookmark,
  type BookmarkCommands,
} from "./bookmarks";
import { readClipboardText } from "../api";
import { hasSelectedText, removeSelectedText, selectedText } from "./contextActions";

function sameClipboardTarget(before: EditorState, after: EditorState): boolean {
  return before.doc.eq(after.doc) && before.selection.eq(after.selection);
}

interface CodeEditorProps {
  docId: string;
  path: string;
  value: string;
  readOnly: boolean;
  syntaxHighlightingEnabled?: boolean;
  fontSize: number;
  style?: CSSProperties;
  visible?: boolean;
  tabId?: string;
  onChange: (text: string) => void;
  cursor?: number;
  bookmarks?: number[];
  onCursorChange?: (cursor: number) => void;
  onBookmarksChange?: (bookmarks: number[]) => void;
  onFocus?: () => void;
  onReplaceCommandReady?: (docId: string, command: (() => boolean) | null) => void;
  onBookmarkCommandsReady?: (docId: string, commands: BookmarkCommands | null) => void;
  diagnostics?: Diagnostic[];
  completionSource?: CompletionSource;
  hoverSource?: HoverTooltipSource;
  canGoToDefinition?: boolean;
  canFindReferences?: boolean;
  navigationBusy?: boolean;
  onNavigate?: (docId: string, kind: "definition" | "references", cursor: number) => void;
  onError?: (message: string | null) => void;
}

/** One long-lived CodeMirror 6 instance for one document ID. */
export default function CodeEditor({
  docId,
  path,
  value,
  readOnly,
  syntaxHighlightingEnabled = true,
  fontSize,
  style,
  visible = true,
  tabId,
  onChange,
  cursor = 0,
  bookmarks = [],
  onCursorChange,
  onBookmarksChange,
  onFocus,
  onReplaceCommandReady,
  onBookmarkCommandsReady,
  diagnostics = [],
  completionSource,
  hoverSource,
  canGoToDefinition = false,
  canFindReferences = false,
  navigationBusy = false,
  onNavigate,
  onError,
}: CodeEditorProps) {
  const mountRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);
  const compartmentsRef = useRef<EditorCompartments | null>(null);
  if (!compartmentsRef.current) {
    compartmentsRef.current = {
      language: new Compartment(),
      readOnly: new Compartment(),
      syntax: new Compartment(),
    };
  }
  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;
  const onReplaceCommandReadyRef = useRef(onReplaceCommandReady);
  onReplaceCommandReadyRef.current = onReplaceCommandReady;
  const onCursorChangeRef = useRef(onCursorChange);
  onCursorChangeRef.current = onCursorChange;
  const onBookmarksChangeRef = useRef(onBookmarksChange);
  onBookmarksChangeRef.current = onBookmarksChange;
  const onBookmarkCommandsReadyRef = useRef(onBookmarkCommandsReady);
  onBookmarkCommandsReadyRef.current = onBookmarkCommandsReady;
  const completionSourceRef = useRef(completionSource);
  completionSourceRef.current = completionSource;
  const hoverSourceRef = useRef(hoverSource);
  hoverSourceRef.current = hoverSource;
  const onFocusRef = useRef(onFocus);
  onFocusRef.current = onFocus;
  const onNavigateRef = useRef(onNavigate);
  onNavigateRef.current = onNavigate;
  const onErrorRef = useRef(onError);
  onErrorRef.current = onError;
  const readOnlyRef = useRef(readOnly);
  readOnlyRef.current = readOnly;
  const [hasSelection, setHasSelection] = useState(false);
  const editorMenu = useContextMenu();
  const openMenuRef = useRef(editorMenu.openAt);
  openMenuRef.current = editorMenu.openAt;

  const contextItems = useMemo<readonly ContextMenuEntry[]>(() => [
    { type: "item", id: "cut", label: "잘라내기", shortcut: "Ctrl+X", disabled: readOnly || !hasSelection },
    { type: "item", id: "copy", label: "복사", shortcut: "Ctrl+C", disabled: !hasSelection },
    { type: "item", id: "paste", label: "붙여넣기", shortcut: "Ctrl+V", disabled: readOnly },
    { type: "separator", id: "navigation-separator" },
    { type: "item", id: "definition", label: "정의로 이동", disabled: navigationBusy || !canGoToDefinition },
    { type: "item", id: "references", label: "참조 찾기", disabled: navigationBusy || !canFindReferences },
  ], [canFindReferences, canGoToDefinition, hasSelection, navigationBusy, readOnly]);

  useEffect(() => {
    if (!mountRef.current) return;
    const compartments = compartmentsRef.current!;
    const view = new EditorView({
      state: EditorState.create({
        doc: value,
        selection: { anchor: Math.min(Math.max(0, cursor), value.length) },
        extensions: [...editorExtensions({
          language: languageForPath(path),
          syntaxHighlightingEnabled,
          readOnly,
          bookmarks,
          onChange: (text) => onChangeRef.current(text),
          onCursorChange: (position) => onCursorChangeRef.current?.(position),
          onBookmarksChange: (next) => onBookmarksChangeRef.current?.(next),
          completionSource: async (context) => {
            try {
              const result = await completionSourceRef.current?.(context);
              return result ?? currentDocumentWordCompletion(context);
            } catch {
              return currentDocumentWordCompletion(context);
            }
          },
          hoverSource: (view, pos, side) => hoverSourceRef.current?.(view, pos, side) ?? null,
          compartments,
        }), EditorView.domEventHandlers({
          contextmenu(event, currentView) {
            if (currentView.compositionStarted) return false;
            event.preventDefault();
            const point = { x: event.clientX, y: event.clientY };
            try {
              const position = currentView.posAtCoords(point);
              const insideSelection = position !== null && currentView.state.selection.ranges.some(
                (range) => !range.empty && position >= range.from && position <= range.to,
              );
              if (position !== null && !insideSelection) {
                currentView.dispatch({ selection: EditorSelection.cursor(position) });
              }
            } catch {
              // Layout-less environments keep the current selection.
            }
            currentView.focus();
            onFocusRef.current?.();
            setHasSelection(hasSelectedText(currentView.state));
            openMenuRef.current(point, currentView.contentDOM);
            return true;
          },
          keydown(event, currentView) {
            if (
              event.isComposing
              || event.keyCode === 229
              || !(
                event.key === "ContextMenu"
                || event.code === "ContextMenu"
                || (event.shiftKey && event.key === "F10")
              )
            ) {
              return false;
            }
            event.preventDefault();
            onFocusRef.current?.();
            const rect = currentView.contentDOM.getBoundingClientRect();
            setHasSelection(hasSelectedText(currentView.state));
            openMenuRef.current(
              {
                x: rect.left + Math.min(24, Math.max(0, rect.width / 2)),
                y: rect.bottom,
              },
              currentView.contentDOM,
            );
            return true;
          },
        })],
      }),
      parent: mountRef.current,
    });
    viewRef.current = view;
    onReplaceCommandReadyRef.current?.(docId, () => openSearchPanel(view));
    const bookmarkCommands: BookmarkCommands = {
      toggle: () => toggleBookmark(view),
      next: () => nextBookmark(view),
      previous: () => previousBookmark(view),
    };
    onBookmarkCommandsReadyRef.current?.(docId, bookmarkCommands);
    return () => {
      onReplaceCommandReadyRef.current?.(docId, null);
      onBookmarkCommandsReadyRef.current?.(docId, null);
      view.destroy();
      viewRef.current = null;
    };
    // The document ID is the lifetime boundary. Parent rerenders and changes to
    // the active view only alter the wrapper style, never this EditorView.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [docId]);

  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;
    view.dispatch(setDiagnostics(view.state, diagnostics));
  }, [diagnostics]);

  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;
    const current = [...view.state.field(bookmarkField)];
    const next = [...new Set(bookmarks)].sort((a, b) => a - b);
    if (current.length === next.length && current.every((line, index) => line === next[index])) return;
    view.dispatch({ effects: setBookmarkLines.of(next), annotations: externalValueSync.of(true) });
  }, [bookmarks]);

  useEffect(() => {
    const view = viewRef.current;
    const compartments = compartmentsRef.current;
    if (!view || !compartments) return;
    view.dispatch({
      effects: [
        compartments.language.reconfigure(
          syntaxHighlightingEnabled ? languageExtensionFor(languageForPath(path)) : [],
        ),
        compartments.readOnly.reconfigure(readOnlyExtension(readOnly)),
        compartments.syntax.reconfigure(syntaxHighlightingExtension(syntaxHighlightingEnabled)),
      ],
    });
  }, [path, readOnly, syntaxHighlightingEnabled]);

  useEffect(() => {
    const view = viewRef.current;
    if (!view || view.state.doc.toString() === value) return;
    view.dispatch({
      changes: { from: 0, to: view.state.doc.length, insert: value },
      annotations: [externalValueSync.of(true), Transaction.addToHistory.of(false)],
    });
  }, [value]);

  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;
    const next = Math.min(Math.max(0, cursor), view.state.doc.length);
    if (view.state.selection.main.head === next) return;
    view.dispatch({
      selection: { anchor: next },
      annotations: [externalValueSync.of(true), Transaction.addToHistory.of(false)],
    });
  }, [cursor]);

  useEffect(() => {
    if (!visible) editorMenu.close();
  }, [editorMenu.close, visible]);

  const runContextAction = async (id: string) => {
    const view = viewRef.current;
    if (!view) return;
    onErrorRef.current?.(null);
    if (id === "copy" || id === "cut") {
      const before = view.state;
      const text = selectedText(before);
      if (!text || (id === "cut" && readOnlyRef.current)) return;
      await navigator.clipboard.writeText(text);
      if (id === "cut") {
        if (!sameClipboardTarget(before, view.state)) {
          onErrorRef.current?.("클립보드 처리 중 선택 영역이 변경되어 잘라내기를 취소했습니다.");
          return;
        }
        view.dispatch(removeSelectedText(before));
      }
      return;
    }
    if (id === "paste") {
      if (readOnlyRef.current) return;
      const before = view.state;
      const text = await readClipboardText();
      if (!sameClipboardTarget(before, view.state)) {
        onErrorRef.current?.("클립보드 처리 중 편집 위치가 변경되어 붙여넣기를 취소했습니다.");
        return;
      }
      view.dispatch(before.replaceSelection(text));
      return;
    }
    if (id === "definition" || id === "references") {
      onNavigateRef.current?.(docId, id, view.state.selection.main.head);
    }
  };

  const selectContextAction = (id: string) => {
    void runContextAction(id).catch((cause: unknown) => {
      onErrorRef.current?.(cause instanceof Error ? cause.message : String(cause));
    });
  };

  return (
    <>
      <div
        className="code-editor"
        data-doc-id={docId}
        data-read-only={String(readOnly)}
        id={panelIdForDoc(docId)}
        role="tabpanel"
        aria-hidden={!visible}
        aria-labelledby={tabId}
        style={{ ...style, fontSize }}
        onFocus={onFocus}
      >
        <div ref={mountRef} className="code-editor-mount" />
      </div>
      <ContextMenu
        open={editorMenu.open}
        anchor={editorMenu.anchor}
        items={contextItems}
        onSelect={selectContextAction}
        onClose={editorMenu.close}
        restoreFocusTo={editorMenu.restoreFocusTo}
        ariaLabel="코드 편집기 작업"
      />
    </>
  );
}
