import { useEffect, useMemo, useRef, useState } from "react";
import { ContextMenu, useContextMenu, type ContextMenuEntry } from "@devbox/context-menu";
import { baseEditorExtensions, markdownEditorExtensions } from "@devbox/editor";
import { defaultKeymap } from "@codemirror/commands";
import { EditorSelection, EditorState } from "@codemirror/state";
import { EditorView, keymap } from "@codemirror/view";
import { readClipboardText } from "../api";
import type {
  EditorCursorRequest,
  WikilinkCandidate,
  WikilinkOccurrence,
} from "../types";
import {
  hasSelectedText,
  insertMarkdownLink,
  removeSelectedText,
  selectedText,
} from "./editorActions";
import { setWikilinkOccurrences, wikilinkEditorExtensions } from "./wikilinkEditor";

// 공용 CodeMirror 설정은 packages/editor(@devbox/editor)에서 가져온다 (PR 24).
// LSP는 넣지 않는다 (knowledge-base는 노트 앱이다).
interface Props {
  value: string;
  onChange: (text: string) => void;
  onSave: () => void;
  onError: (message: string | null) => void;
  wikilinks?: readonly WikilinkOccurrence[];
  loadWikilinkCandidates?: (query: string) => Promise<WikilinkCandidate[]>;
  onNavigateWikilink?: (path: string) => void;
  cursorRequest?: EditorCursorRequest | null;
}

export default function MarkdownEditor({
  value,
  onChange,
  onSave,
  onError,
  wikilinks = [],
  loadWikilinkCandidates,
  onNavigateWikilink,
  cursorRequest = null,
}: Props) {
  const mountRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);
  const syncingValueRef = useRef(false);
  const [hasSelection, setHasSelection] = useState(false);
  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;
  const onSaveRef = useRef(onSave);
  onSaveRef.current = onSave;
  const onErrorRef = useRef(onError);
  onErrorRef.current = onError;
  const loadCandidatesRef = useRef(loadWikilinkCandidates);
  loadCandidatesRef.current = loadWikilinkCandidates;
  const navigateWikilinkRef = useRef(onNavigateWikilink);
  navigateWikilinkRef.current = onNavigateWikilink;
  const editorMenu = useContextMenu();
  const openMenuRef = useRef(editorMenu.openAt);
  openMenuRef.current = editorMenu.openAt;

  const menuItems = useMemo<readonly ContextMenuEntry[]>(
    () => [
      { type: "item", id: "cut", label: "잘라내기", shortcut: "Ctrl+X", disabled: !hasSelection },
      { type: "item", id: "copy", label: "복사", shortcut: "Ctrl+C", disabled: !hasSelection },
      { type: "item", id: "paste", label: "붙여넣기", shortcut: "Ctrl+V" },
      { type: "separator", id: "link-separator" },
      { type: "item", id: "insert-link", label: "링크 삽입" },
    ],
    [hasSelection],
  );

  useEffect(() => {
    if (!mountRef.current) return;
    const view = new EditorView({
      state: EditorState.create({
        doc: value,
        extensions: [
          baseEditorExtensions(),
          markdownEditorExtensions(),
          wikilinkEditorExtensions(
            (query) => loadCandidatesRef.current?.(query) ?? Promise.resolve([]),
            (path) => navigateWikilinkRef.current?.(path),
          ),
          keymap.of([
            ...defaultKeymap,
            { key: "Mod-s", run: () => { onSaveRef.current(); return true; }, preventDefault: true },
          ]),
          EditorView.domEventHandlers({
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
                // jsdom이나 초기 layout처럼 좌표 해석이 불가능한 경우 현재 selection을 유지한다.
              }
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
          }),
          EditorView.updateListener.of((update) => {
            if (update.docChanged && !syncingValueRef.current) {
              onChangeRef.current(update.state.doc.toString());
            }
          }),
        ],
      }),
      parent: mountRef.current,
    });
    viewRef.current = view;
    return () => {
      view.destroy();
      viewRef.current = null;
    };
    // 마운트 수명은 컴포넌트 수명과 같다 (파일 전환은 value 동기화로 처리)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // 외부 값 동기화 (파일 전환/저장 취소)
  useEffect(() => {
    const view = viewRef.current;
    if (!view || view.state.doc.toString() === value) return;
    syncingValueRef.current = true;
    try {
      view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: value } });
    } finally {
      syncingValueRef.current = false;
    }
  }, [value]);

  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;
    view.dispatch({ effects: setWikilinkOccurrences.of(wikilinks) });
  }, [wikilinks]);

  useEffect(() => {
    const view = viewRef.current;
    if (!view || !cursorRequest) return;
    const lineNumber = Math.min(Math.max(cursorRequest.line, 1), view.state.doc.lines);
    const line = view.state.doc.line(lineNumber);
    const columnOffset = Math.min(Math.max(cursorRequest.column - 1, 0), line.length);
    const position = line.from + columnOffset;
    view.dispatch({
      selection: EditorSelection.cursor(position),
      effects: EditorView.scrollIntoView(position, { y: "center" }),
    });
    view.focus();
  }, [cursorRequest]);

  const runAction = async (id: string) => {
    const view = viewRef.current;
    if (!view) return;
    onErrorRef.current(null);
    if (id === "copy" || id === "cut") {
      const text = selectedText(view.state);
      if (!text) return;
      await navigator.clipboard.writeText(text);
      if (id === "cut") view.dispatch(removeSelectedText(view.state));
      return;
    }
    if (id === "paste") {
      const text = await readClipboardText();
      view.dispatch(view.state.replaceSelection(text));
      return;
    }
    if (id === "insert-link") {
      const requested = prompt("링크 URL");
      const url = requested?.trim();
      if (url) view.dispatch(insertMarkdownLink(view.state, url));
    }
  };

  const onSelect = (id: string) => {
    void runAction(id).catch((cause: unknown) => {
      onErrorRef.current(cause instanceof Error ? cause.message : String(cause));
    });
  };

  return (
    <>
      <div ref={mountRef} className="editor codemirror-editor" />
      <ContextMenu
        open={editorMenu.open}
        anchor={editorMenu.anchor}
        items={menuItems}
        onSelect={onSelect}
        onClose={editorMenu.close}
        restoreFocusTo={editorMenu.restoreFocusTo}
        ariaLabel="Markdown 편집기 작업"
      />
    </>
  );
}
