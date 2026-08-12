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
  onFocus?: () => void;
  onReplaceCommandReady?: (docId: string, command: (() => boolean) | null) => void;
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
  onFocus,
  onReplaceCommandReady,
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

  useEffect(() => {
    if (!mountRef.current) return;
    const compartments = compartmentsRef.current!;
    const view = new EditorView({
      state: EditorState.create({
        doc: value,
        extensions: editorExtensions({
          language: languageForPath(path),
          syntaxHighlightingEnabled,
          readOnly,
          onChange: (text) => onChangeRef.current(text),
          compartments,
        }),
      }),
      parent: mountRef.current,
    });
    viewRef.current = view;
    onReplaceCommandReadyRef.current?.(docId, () => openSearchPanel(view));
    return () => {
      onReplaceCommandReadyRef.current?.(docId, null);
      view.destroy();
      viewRef.current = null;
    };
    // The document ID is the lifetime boundary. Parent rerenders and changes to
    // the active view only alter the wrapper style, never this EditorView.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [docId]);

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
