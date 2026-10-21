import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { useLayoutEffect, useRef } from "react";
import type { DocKey } from "../../rpc/gen/rpc";
import {
  documentExtensions,
  editing,
  editingSlot,
  languageFor,
  languageSlot,
  placeholderState,
  replaceText,
} from "./editorExtensions";
import type { DocState } from "./machine";
import { keyId } from "./offlineJournal";
import { useDocuments } from "./store";

export interface EditorHostProps {
  docKey: DocKey;
  /** A language name or file name; notes default to Markdown. */
  language?: string;
  readOnly?: boolean;
  /** Ctrl+S. Defaults to saving the document through the store. */
  onSave?: () => void;
  ariaLabel?: string;
}

const EDITABLE: ReadonlySet<DocState> = new Set<DocState>(["Clean", "Dirty", "Saving", "Conflict", "Orphaned"]);

/**
 * One CodeMirror view for every open document. Each document keeps its own `EditorState`
 * (and so its own undo history) and switching documents swaps it in with `setState` (AU-K1).
 */
export function EditorHost({ docKey, language, readOnly = false, onSave, ariaLabel = "문서 편집기" }: EditorHostProps) {
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const states = useRef(new Map<string, EditorState>());
  const active = useRef<string | undefined>(undefined);
  const save = useRef(onSave);
  const initialLabel = useRef(ariaLabel);
  const id = useDocuments((s) => {
    const wanted = keyId(docKey);
    return Object.values(s.docs).find((d) => keyId(d.key) === wanted)?.id;
  });
  const state = useDocuments((s) => (id ? s.docs[id]?.state : undefined));
  const loaded = state !== undefined && state !== "Loading";
  const locked = readOnly || !state || !EDITABLE.has(state);
  const kind = useDocuments((s) => (id ? s.docs[id]?.policy.kind : undefined)) ?? "note";

  useLayoutEffect(() => {
    save.current = onSave;
  }, [onSave]);

  useLayoutEffect(() => {
    if (!host.current) return;
    const created = new EditorView({ parent: host.current, state: placeholderState(initialLabel.current) });
    view.current = created;
    const off = useDocuments.getState().registerEditor((target, text) => {
      if (target === active.current) {
        const spec = replaceText(created.state, text);
        if (spec) created.dispatch(spec);
        return;
      }
      const stored = states.current.get(target);
      const spec = stored && replaceText(stored, text);
      if (stored && spec) states.current.set(target, stored.update(spec).state);
    });
    return () => {
      off();
      // Keep the active document's state (and history) for a remount (StrictMode runs effects twice).
      if (active.current) states.current.set(active.current, created.state);
      active.current = undefined;
      view.current = null;
      created.destroy();
    };
  }, []);

  // Swap the document's own state in (never just its text), creating it on first show.
  useLayoutEffect(() => {
    const current = view.current;
    if (!current) return;
    if (active.current) states.current.set(active.current, current.state);
    const docs = useDocuments.getState().docs;
    for (const known of [...states.current.keys()]) if (!docs[known]) states.current.delete(known);
    const doc = id ? docs[id] : undefined;
    if (!doc || !loaded) {
      active.current = undefined;
      current.setState(placeholderState(ariaLabel));
      return;
    }
    let next = states.current.get(doc.id);
    if (!next) {
      next = createState(doc.id, doc.text, ariaLabel, save);
      states.current.set(doc.id, next);
    }
    active.current = doc.id;
    current.setState(next);
  }, [id, loaded, ariaLabel]);

  useLayoutEffect(() => {
    const current = view.current;
    if (!current || !loaded || !id || active.current !== id) return;
    current.dispatch({ effects: editingSlot.reconfigure(editing(locked, ariaLabel)) });
  }, [id, loaded, locked, ariaLabel]);

  useLayoutEffect(() => {
    const target = id;
    if (!target || !loaded || active.current !== target) return;
    let cancelled = false;
    void languageFor(language, kind).then(
      (support) => {
        if (cancelled) return;
        const spec = { effects: languageSlot.reconfigure(support ?? []) };
        if (active.current === target) view.current?.dispatch(spec);
        else {
          const stored = states.current.get(target);
          if (stored) states.current.set(target, stored.update(spec).state);
        }
      },
      () => {
        // A missing parser chunk must not prevent viewing or editing the text.
      },
    );
    return () => {
      cancelled = true;
    };
  }, [id, loaded, language, kind]);

  return <div ref={host} data-keyscope="editor" style={{ height: "100%" }} />;
}

function createState(id: string, text: string, label: string, save: { current?: () => void }): EditorState {
  const dispatch = useDocuments.getState().dispatch;
  const handlers = {
    edit: (next: string) => dispatch(id, { type: "edit", text: next }),
    save: () => (save.current ? save.current() : dispatch(id, { type: "save" })),
  };
  // Starts locked; the editing effect unlocks it for editable states.
  return EditorState.create({ doc: text, extensions: documentExtensions(handlers, true, label) });
}
