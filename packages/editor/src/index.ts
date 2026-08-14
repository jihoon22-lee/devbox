//! CodeMirror 6 공용 설정 프리미티브.
//!
//! 추출 근거: code-pad(첫 소비자)와 knowledge-base(두 번째 소비자)가 공유하는
//! 에디터 기반 설정. 테마·전체 state·LSP 연동은 옮기지 않는다 — 각 앱에 남는다.
//!
//! 두 앱에서 동일한 부분만: 기본 extension(라인번호·히스토리·활성 라인·기본 keymap·
//! 검색), 언어 감지, 문법 하이라이팅, read-only. knowledge-base는 마크다운 전용이므로
//! `markdownEditorExtensions()`를 쓴다.

import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { css } from "@codemirror/lang-css";
import { html } from "@codemirror/lang-html";
import { javascript } from "@codemirror/lang-javascript";
import { json } from "@codemirror/lang-json";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { python } from "@codemirror/lang-python";
import { rust } from "@codemirror/lang-rust";
import { sql } from "@codemirror/lang-sql";
import { defaultHighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { search, searchKeymap } from "@codemirror/search";
import { EditorState, type Extension } from "@codemirror/state";
import { EditorView, highlightActiveLine, keymap, lineNumbers, type KeyBinding } from "@codemirror/view";

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

/** 확장자 → 언어 판정. 알 수 없으면 "text". */
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

export function languageExtension(language: SupportedLanguage): Extension | null {
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
      return markdown({ base: markdownLanguage });
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

/** 기본 keymap: 기본 + 히스토리 + 검색. */
export function baseKeymap(): KeyBinding[] {
  return [...defaultKeymap, ...historyKeymap, ...searchKeymap, indentWithTab];
}

/** 두 앱이 공유하는 에디터 기반 extension. */
export function baseEditorExtensions(opts: { searchPanel?: boolean } = {}): Extension[] {
  return [
    // CM6는 다중 선택/다중 커서(Mod-d 등)를 위해 이 facet이 명시적으로 켜져 있어야 한다.
    EditorState.allowMultipleSelections.of(true),
    lineNumbers(),
    highlightActiveLine(),
    history(),
    keymap.of(baseKeymap()),
    ...(opts.searchPanel !== false ? [search({ top: true })] : []),
  ];
}

export function syntaxHighlightingExtension(enabled: boolean): Extension {
  return enabled ? syntaxHighlighting(defaultHighlightStyle, { fallback: true }) : [];
}

export function readOnlyExtension(readOnly: boolean): Extension {
  return [EditorState.readOnly.of(readOnly), EditorView.editable.of(!readOnly)];
}

/** knowledge-base용 마크다운 편집 확장. */
export function markdownEditorExtensions(): Extension[] {
  return [markdown({ base: markdownLanguage }), syntaxHighlightingExtension(true)];
}
