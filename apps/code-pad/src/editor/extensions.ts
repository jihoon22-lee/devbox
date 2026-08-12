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
  drawSelection,
  dropCursor,
  EditorView,
  highlightActiveLine,
  highlightSpecialChars,
  keymap,
  lineNumbers,
  type KeyBinding,
} from "@codemirror/view";
import { Annotation, Compartment, EditorState, type Extension } from "@codemirror/state";
import { history, historyKeymap } from "@codemirror/commands";
import { openSearchPanel, search, searchKeymap } from "@codemirror/search";
import type { EncodingKind } from "../types";

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
  onChange: (text: string) => void;
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

/**
 * Explicit CM6 extensions keep the editor shell small and avoid enabling the
 * multicursor/rectangular-selection/bookmark features reserved for a later PR.
 */
export function editorExtensions({
  language,
  syntaxHighlightingEnabled,
  readOnly,
  onChange,
  compartments,
}: EditorExtensionOptions): Extension[] {
  const languageMode = syntaxHighlightingEnabled ? languageExtensionFor(language) : [];
  const syntaxMode = syntaxHighlightingExtension(syntaxHighlightingEnabled);
  const readOnlyMode = readOnlyExtension(readOnly);
  return [
    lineNumbers(),
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
    ]),
    search({ top: true }),
    autocompletion({ override: [currentDocumentWordCompletion] }),
    compartments?.syntax.of(syntaxMode) ?? syntaxMode,
    compartments?.readOnly.of(readOnlyMode) ?? readOnlyMode,
    EditorView.updateListener.of((update) => {
      if (
        update.docChanged &&
        !update.transactions.some((transaction) => transaction.annotation(externalValueSync))
      ) {
        onChange(update.state.doc.toString());
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
