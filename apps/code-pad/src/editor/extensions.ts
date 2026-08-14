import { autocompletion, type CompletionContext, type CompletionSource } from "@codemirror/autocomplete";
import { indentWithTab } from "@codemirror/commands";
import { foldGutter, foldKeymap, indentOnInput } from "@codemirror/language";
import {
  crosshairCursor,
  drawSelection,
  dropCursor,
  EditorView,
  highlightSpecialChars,
  keymap,
  rectangularSelection,
  type KeyBinding,
} from "@codemirror/view";
import { Annotation, Compartment, type Extension } from "@codemirror/state";
import { lintGutter } from "@codemirror/lint";
import { hoverTooltip } from "@codemirror/view";
import {
  openSearchPanel,
  selectNextOccurrence,
  selectSelectionMatches,
} from "@codemirror/search";
import {
  baseEditorExtensions,
  languageExtensionFor,
  languageForPath,
  languageLabel,
  readOnlyExtension,
  syntaxHighlightingExtension,
  type SupportedLanguage,
} from "@devbox/editor";
import type { EncodingKind } from "../types";
import {
  bookmarkExtension,
  bookmarkField,
  nextBookmark,
  previousBookmark,
  toggleBookmark,
} from "./bookmarks";

export type { SupportedLanguage };
export { languageForPath, languageLabel, languageExtensionFor, syntaxHighlightingExtension, readOnlyExtension };

/** Marks a transaction as a parent-to-editor value synchronization. */
export const externalValueSync = Annotation.define<boolean>();

/** Ctrl/Cmd+H opens CodeMirror's built-in find/replace panel. */
export const replaceKeymap: KeyBinding[] = [
  { key: "Mod-h", run: openSearchPanel, scope: "editor search-panel", preventDefault: true },
];

/**
 * Word completion intentionally reads only the current CodeMirror document.
 * LSP/symbol completion and cross-file indexes are a later phase.
 */
export const currentDocumentWordCompletion: CompletionSource = (context: CompletionContext) => {
  const word = context.matchBefore(/[\w$-]*/);
  if (!word || (word.from === word.to && !context.explicit)) return null;

  const source = context.state.doc.toString();
  const options = Array.from(new Set(source.match(/[A-Za-z_$][\w$-]*/g) ?? []))
    .filter((label) => label !== word.text)
    .sort((a, b) => a.localeCompare(b))
    .map((label) => ({ label, type: "text" as const }));
  return options.length > 0 ? { from: word.from, options } : null;
};

export interface EditorExtensionOptions {
  language: SupportedLanguage;
  syntaxHighlightingEnabled: boolean;
  readOnly: boolean;
  bookmarks?: number[];
  onChange: (text: string) => void;
  onCursorChange?: (position: number) => void;
  onBookmarksChange?: (bookmarks: number[]) => void;
  completionSource?: CompletionSource;
  hoverSource?: NonNullable<Parameters<typeof hoverTooltip>[0]>;
  compartments?: EditorCompartments;
}

export interface EditorCompartments {
  language: Compartment;
  readOnly: Compartment;
  syntax: Compartment;
}

export function editorExtensions({
  language,
  syntaxHighlightingEnabled,
  readOnly,
  bookmarks = [],
  onChange,
  onCursorChange,
  onBookmarksChange,
  completionSource,
  hoverSource,
  compartments,
}: EditorExtensionOptions): Extension[] {
  const languageMode = syntaxHighlightingEnabled ? languageExtensionFor(language) : [];
  const syntaxMode = syntaxHighlightingExtension(syntaxHighlightingEnabled);
  const readOnlyMode = readOnlyExtension(readOnly);
  return [
    // 공용 기반(lineNumbers·히스토리·활성 라인·기본 keymap·검색)은 @devbox/editor에서.
    ...baseEditorExtensions(),
    rectangularSelection(),
    crosshairCursor(),
    bookmarkExtension(bookmarks),
    highlightSpecialChars(),
    foldGutter(),
    drawSelection(),
    dropCursor(),
    indentOnInput(),
    keymap.of([
      ...foldKeymap,
      ...replaceKeymap,
      indentWithTab,
      { key: "F2", run: nextBookmark },
      { key: "Shift-F2", run: previousBookmark },
      { key: "Mod-F2", run: toggleBookmark },
      { key: "Mod-d", run: selectNextOccurrence, preventDefault: true },
      { key: "Mod-Shift-l", run: selectSelectionMatches, preventDefault: true },
    ]),
    lintGutter(),
    autocompletion({ override: [completionSource ?? currentDocumentWordCompletion] }),
    ...(hoverSource ? [hoverTooltip(hoverSource)] : []),
    compartments?.syntax.of(syntaxMode) ?? syntaxMode,
    compartments?.readOnly.of(readOnlyMode) ?? readOnlyMode,
    EditorView.updateListener.of((update) => {
      if (
        update.docChanged &&
        !update.transactions.some((transaction) => transaction.annotation(externalValueSync))
      ) {
        onChange(update.state.doc.toString());
      }
      if (
        (update.docChanged || update.selectionSet) &&
        !update.transactions.some((transaction) => transaction.annotation(externalValueSync))
      ) {
        onCursorChange?.(update.state.selection.main.head);
      }
      if (
        update.state.field(bookmarkField) !== update.startState.field(bookmarkField)
        && !update.transactions.some((transaction) => transaction.annotation(externalValueSync))
      ) {
        onBookmarksChange?.([...update.state.field(bookmarkField)]);
      }
    }),
    compartments?.language.of(languageMode) ?? languageMode,
  ];
}

export function encodingLabel(kind: EncodingKind): string {
  switch (kind) {
    case "utf8":
      return "UTF-8";
    case "utf16Le":
      return "UTF-16 LE";
    case "utf16Be":
      return "UTF-16 BE";
    case "cp949":
      return "CP949";
  }
}
