import { closeBrackets, closeBracketsKeymap } from "@codemirror/autocomplete";
import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import { markdown } from "@codemirror/lang-markdown";
import { bracketMatching, indentOnInput, type LanguageSupport } from "@codemirror/language";
import { languages } from "@codemirror/language-data";
import { highlightSelectionMatches, searchKeymap } from "@codemirror/search";
import { Annotation, Compartment, EditorState, type Extension, Prec, type TransactionSpec } from "@codemirror/state";
import { drawSelection, EditorView, keymap, lineNumbers } from "@codemirror/view";
import { loadLanguage } from "../../ui/code/languages";
import { codePhrases } from "../../ui/code/phrases";
import { codeTheme } from "../../ui/code/theme";

/** Marks text the store applied (external reload, restore, [disk]); it is not a user edit. */
export const fromStore = Annotation.define<boolean>();
export const languageSlot = new Compartment();
export const editingSlot = new Compartment();

export function editing(readOnly: boolean, label: string): Extension {
  return [
    EditorState.readOnly.of(readOnly),
    EditorView.editable.of(!readOnly),
    EditorView.contentAttributes.of({
      role: "textbox",
      "aria-label": label,
      "aria-multiline": "true",
      "aria-readonly": String(readOnly),
      tabindex: "0",
    }),
  ];
}

export interface DocumentHandlers {
  /** Called with the full text after a user change in the active document. */
  edit(text: string): void;
  save(): void;
}

/** Everything a document state needs; one set per `EditorState` so each document keeps its own history. */
export function documentExtensions(handlers: DocumentHandlers, readOnly: boolean, label: string): Extension[] {
  return [
    Prec.highest(keymap.of([{ key: "Mod-s", preventDefault: true, run: () => (handlers.save(), true) }])),
    codeTheme,
    codePhrases,
    lineNumbers(),
    drawSelection(),
    history(),
    indentOnInput(),
    bracketMatching(),
    closeBrackets(),
    highlightSelectionMatches(),
    keymap.of([...closeBracketsKeymap, ...defaultKeymap, ...historyKeymap, ...searchKeymap]),
    EditorView.lineWrapping,
    languageSlot.of([]),
    editingSlot.of(editing(readOnly, label)),
    EditorView.updateListener.of((update) => {
      if (update.transactions.some((tr) => tr.docChanged && !tr.annotation(fromStore)))
        handlers.edit(update.state.doc.toString());
    }),
  ];
}

/** An empty, read-only state shown while a document is loading. */
export function placeholderState(label: string): EditorState {
  return EditorState.create({ extensions: [codeTheme, editingSlot.of(editing(true, label))] });
}

/** Notes use Markdown with fenced code highlighting; other documents load their parser on demand. */
export async function languageFor(
  language: string | undefined,
  kind: "note" | "code",
): Promise<LanguageSupport | null> {
  if (!language && kind === "note") return markdown({ codeLanguages: languages });
  return language ? loadLanguage(language) : null;
}

/**
 * Replaces the document with `text` as one undoable transaction (history on, the default).
 * Only the changed middle is replaced, so the cursor and unrelated lines stay put.
 */
export function replaceText(state: EditorState, text: string): TransactionSpec | null {
  const current = state.doc.toString();
  if (current === text) return null;
  let from = 0;
  const max = Math.min(current.length, text.length);
  while (from < max && current.charCodeAt(from) === text.charCodeAt(from)) from++;
  let end = 0;
  while (end < max - from && current.charCodeAt(current.length - 1 - end) === text.charCodeAt(text.length - 1 - end))
    end++;
  return {
    changes: { from, to: current.length - end, insert: text.slice(from, text.length - end) },
    annotations: fromStore.of(true),
  };
}
