import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import { bracketMatching, indentOnInput } from "@codemirror/language";
import { highlightSelectionMatches, searchKeymap } from "@codemirror/search";
import { Annotation, Compartment, EditorState, Transaction } from "@codemirror/state";
import { drawSelection, EditorView, keymap, lineNumbers } from "@codemirror/view";
import { useLayoutEffect, useRef } from "react";
import { loadLanguage } from "./languages";
import { codePhrases } from "./phrases";
import { codeTheme } from "./theme";

export interface CodeViewProps {
  value: string;
  language?: string;
  readOnly?: boolean;
  onChange?: (text: string) => void;
  ariaLabel: string;
}

const externalValue = Annotation.define<boolean>();

function editability(readOnly: boolean) {
  return [EditorState.readOnly.of(readOnly), EditorView.editable.of(!readOnly)];
}

function attributes(ariaLabel: string, readOnly: boolean) {
  return EditorView.contentAttributes.of({
    role: "textbox",
    "aria-label": ariaLabel,
    "aria-multiline": "true",
    "aria-readonly": String(readOnly),
    tabindex: "0",
  });
}

export function CodeView({ value, language, readOnly = false, onChange, ariaLabel }: CodeViewProps) {
  const host = useRef<HTMLDivElement>(null);
  const initial = useRef({ value, readOnly, ariaLabel });
  const change = useRef(onChange);
  const editor = useRef<{
    view: EditorView;
    syntax: Compartment;
    editing: Compartment;
    accessibility: Compartment;
  } | null>(null);

  useLayoutEffect(() => {
    change.current = onChange;
  }, [onChange]);

  useLayoutEffect(() => {
    if (!host.current) return;
    const syntax = new Compartment();
    const editing = new Compartment();
    const accessibility = new Compartment();
    const view = new EditorView({
      parent: host.current,
      state: EditorState.create({
        doc: initial.current.value,
        extensions: [
          codeTheme,
          codePhrases,
          lineNumbers(),
          drawSelection(),
          history(),
          indentOnInput(),
          bracketMatching(),
          highlightSelectionMatches(),
          keymap.of([...defaultKeymap, ...historyKeymap, ...searchKeymap]),
          syntax.of([]),
          editing.of(editability(initial.current.readOnly)),
          accessibility.of(attributes(initial.current.ariaLabel, initial.current.readOnly)),
          EditorView.updateListener.of((update) => {
            if (
              update.transactions.some(
                (transaction) => transaction.docChanged && !transaction.annotation(externalValue),
              )
            ) {
              change.current?.(update.state.doc.toString());
            }
          }),
        ],
      }),
    });
    editor.current = { view, syntax, editing, accessibility };
    return () => {
      editor.current = null;
      view.destroy();
    };
  }, []);

  useLayoutEffect(() => {
    const current = editor.current;
    if (!current) return;
    const { view, editing, accessibility } = current;
    view.dispatch({
      effects: [editing.reconfigure(editability(readOnly)), accessibility.reconfigure(attributes(ariaLabel, readOnly))],
      changes: value === view.state.doc.toString() ? undefined : { from: 0, to: view.state.doc.length, insert: value },
      annotations: [externalValue.of(true), Transaction.addToHistory.of(false)],
    });
  }, [value, readOnly, ariaLabel]);

  useLayoutEffect(() => {
    const current = editor.current;
    if (!current) return;
    const { view, syntax } = current;
    let cancelled = false;
    view.dispatch({ effects: syntax.reconfigure([]) });
    if (language) {
      void loadLanguage(language).then(
        (support) => {
          if (!cancelled) view.dispatch({ effects: syntax.reconfigure(support ?? []) });
        },
        () => {
          // A missing parser chunk must not prevent viewing or editing the text.
        },
      );
    }
    return () => {
      cancelled = true;
    };
  }, [language]);

  return <div ref={host} />;
}
