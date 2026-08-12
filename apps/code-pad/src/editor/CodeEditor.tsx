import { useEffect, useRef, type CSSProperties } from "react";
import { Compartment, EditorState, Transaction } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { openSearchPanel } from "@codemirror/search";
import {
  editorExtensions,
  externalValueSync,
  languageExtensionFor,
  languageForPath,
  readOnlyExtension,
  syntaxHighlightingExtension,
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

  useEffect(() => {
    if (!mountRef.current) return;
    const compartments = compartmentsRef.current!;
    const view = new EditorView({
      state: EditorState.create({
        doc: value,
        selection: { anchor: Math.min(Math.max(0, cursor), value.length) },
        extensions: editorExtensions({
          language: languageForPath(path),
          syntaxHighlightingEnabled,
          readOnly,
          bookmarks,
          onChange: (text) => onChangeRef.current(text),
          onCursorChange: (position) => onCursorChangeRef.current?.(position),
          onBookmarksChange: (next) => onBookmarksChangeRef.current?.(next),
          compartments,
        }),
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

  return (
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
  );
}
