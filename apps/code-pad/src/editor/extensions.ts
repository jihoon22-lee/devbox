import { autocompletion, type CompletionContext, type CompletionSource } from "@codemirror/autocomplete";
import { defaultKeymap, indentWithTab } from "@codemirror/commands";
import { css } from "@codemirror/lang-css";
import { html } from "@codemirror/lang-html";
import { javascript } from "@codemirror/lang-javascript";
import { json } from "@codemirror/lang-json";
import { markdown } from "@codemirror/lang-markdown";
import { rust } from "@codemirror/lang-rust";
import { sql } from "@codemirror/lang-sql";
import { python } from "@codemirror/lang-python";
import { defaultHighlightStyle, foldGutter, foldKeymap, indentOnInput, syntaxHighlighting } from "@codemirror/language";
import {
  crosshairCursor,
  drawSelection,
  dropCursor,
  EditorView,
  highlightActiveLine,
  highlightSpecialChars,
  keymap,
  lineNumbers,
  rectangularSelection,
  type KeyBinding,
} from "@codemirror/view";
import { Annotation, Compartment, EditorState, type Extension } from "@codemirror/state";
import { lintGutter } from "@codemirror/lint";
import { hoverTooltip } from "@codemirror/view";
import { history, historyKeymap } from "@codemirror/commands";
import {
  openSearchPanel,
  search,
  searchKeymap,
  selectNextOccurrence,
  selectSelectionMatches,
} from "@codemirror/search";
import type { EncodingKind } from "../types";
import {
  bookmarkExtension,
  bookmarkField,
  nextBookmark,
  previousBookmark,
  toggleBookmark,
} from "./bookmarks";

export type SupportedLanguage =
  | "css"
  | "html"
  | "javascript"
  | "json"
  | "markdown"
  | "python"
  | "rust"
  | "sql"
  | "text";

const EXTENSION_LANGUAGE: Record<string, SupportedLanguage> = {
  css: "css",
  scss: "css",
  less: "css",
  html: "html",
  htm: "html",
  js: "javascript",
  jsx: "javascript",
  mjs: "javascript",
  cjs: "javascript",
  ts: "javascript",
  tsx: "javascript",
  json: "json",
  jsonc: "json",
  md: "markdown",
  markdown: "markdown",
  mmd: "markdown",
  py: "python",
  pyi: "python",
  rs: "rust",
  sql: "sql",
};

export function languageForPath(path: string): SupportedLanguage {
  const fileName = path.split("\\").join("/").split("/").pop() ?? "";
  const extension = fileName.includes(".") ? fileName.split(".").pop()?.toLowerCase() : undefined;
  return extension ? EXTENSION_LANGUAGE[extension] ?? "text" : "text";
}

export function languageLabel(language: SupportedLanguage): string {
  switch (language) {
    case "javascript":
      return "JavaScript/TypeScript";
    case "markdown":
      return "Markdown";
    case "text":
      return "일반 텍스트";
    default:
      return language.toUpperCase();
  }
}

function languageExtension(language: SupportedLanguage): Extension | null {
  switch (language) {
    case "css":
      return css();
    case "html":
      return html();
    case "javascript":
      return javascript({ jsx: true, typescript: true });
    case "json":
      return json();
    case "markdown":
      return markdown();
    case "python":
      return python();
    case "rust":
      return rust();
    case "sql":
      return sql();
    case "text":
      return null;
  }
}

export function languageExtensionFor(language: SupportedLanguage): Extension {
  return languageExtension(language) ?? [];
}

export function syntaxHighlightingExtension(enabled: boolean): Extension {
  return enabled ? syntaxHighlighting(defaultHighlightStyle, { fallback: true }) : [];
}

export function readOnlyExtension(readOnly: boolean): Extension {
  return [EditorState.readOnly.of(readOnly), EditorView.editable.of(!readOnly)];
}

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

/** Marks a transaction as a parent-to-editor value synchronization. */
export const externalValueSync = Annotation.define<boolean>();

/** Ctrl/Cmd+H opens CodeMirror's built-in find/replace panel. */
export const replaceKeymap: KeyBinding[] = [
  { key: "Mod-h", run: openSearchPanel, scope: "editor search-panel", preventDefault: true },
];

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
    lineNumbers(),
    // CM6 rejects additional ranges unless this facet is explicitly enabled.
    // The default keymap plus search keymap then provide the expected
    // Mod-Alt-Arrow and Mod-D multicursor actions.
    EditorState.allowMultipleSelections.of(true),
    rectangularSelection(),
    crosshairCursor(),
    bookmarkExtension(bookmarks),
    highlightSpecialChars(),
    history(),
    foldGutter(),
    drawSelection(),
    dropCursor(),
    indentOnInput(),
    highlightActiveLine(),
    keymap.of([
      ...defaultKeymap,
      ...historyKeymap,
      ...foldKeymap,
      ...searchKeymap,
      ...replaceKeymap,
      indentWithTab,
      { key: "F2", run: nextBookmark },
      { key: "Shift-F2", run: previousBookmark },
      { key: "Mod-F2", run: toggleBookmark },
      { key: "Mod-d", run: selectNextOccurrence, preventDefault: true },
      { key: "Mod-Shift-l", run: selectSelectionMatches, preventDefault: true },
    ]),
    search({ top: true }),
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
