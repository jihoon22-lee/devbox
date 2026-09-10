import type { Completion } from "@codemirror/autocomplete";
import type { Diagnostic } from "@codemirror/lint";
import type { EditorView } from "@codemirror/view";
import type {
  LspCompletionItem,
  LspCompletionResult,
  LspDiagnosticResult,
  LspHoverResult,
  LspPosition,
  LspPositionEncoding,
} from "./types";

/** Convert a JavaScript UTF-16 offset to the position encoding negotiated by the server. */
export function positionForOffset(
  text: string,
  offset: number,
  encoding: LspPositionEncoding = "utf-16",
): LspPosition {
  const safeOffset = Math.min(Math.max(0, offset), text.length);
  const lineStart = text.lastIndexOf("\n", safeOffset - 1) + 1;
  const line = text.slice(0, lineStart).split("\n").length - 1;
  const lineText = text.slice(lineStart, safeOffset);
  return {
    line,
    character: encoding === "utf-8" ? new TextEncoder().encode(lineText).length : lineText.length,
  };
}

/** Convert a server position back to a JavaScript UTF-16 offset for display. */
export function offsetForPosition(
  text: string,
  position: LspPosition,
  encoding: LspPositionEncoding = "utf-16",
): number {
  const lines = text.split("\n");
  const line = Math.min(Math.max(0, position.line), Math.max(0, lines.length - 1));
  const lineText = lines[line] ?? "";
  if (encoding === "utf-16") return Math.min(position.character, lineText.length) + lines
    .slice(0, line)
    .reduce((total, item) => total + item.length + 1, 0);
  const bytes = new TextEncoder().encode(lineText);
  let byteCount = Math.min(Math.max(0, position.character), bytes.length);
  let codeUnits = 0;
  while (byteCount > 0 && codeUnits < lineText.length) {
    const codePoint = lineText.codePointAt(codeUnits)!;
    const width = new TextEncoder().encode(String.fromCodePoint(codePoint)).length;
    if (width > byteCount) break;
    byteCount -= width;
    codeUnits += String.fromCodePoint(codePoint).length;
  }
  return lines.slice(0, line).reduce((total, item) => total + item.length + 1, 0) + codeUnits;
}

function severityForCodeMirror(severity: number | null | undefined): Diagnostic["severity"] {
  switch (severity) {
    case 1: return "error";
    case 2: return "warning";
    case 4: return "hint";
    default: return "info";
  }
}

export function diagnosticsForCodeMirror(
  text: string,
  result: LspDiagnosticResult,
  encoding: LspPositionEncoding = "utf-16",
): Diagnostic[] {
  if (result.stale) return [];
  return result.diagnostics.map((item) => ({
    from: offsetForPosition(text, item.range.start, encoding),
    to: offsetForPosition(text, item.range.end, encoding),
    severity: severityForCodeMirror(item.severity),
    message: item.message,
    source: [item.source, item.code].filter(Boolean).join(" ") || undefined,
  }));
}

interface CompletionTextEdit {
  range: { start: LspPosition; end: LspPosition };
  newText: string;
}

function completionTextEdit(item: LspCompletionItem): CompletionTextEdit | null {
  if (!item.textEdit || typeof item.textEdit !== "object") return null;
  const edit = item.textEdit as {
    range?: unknown;
    insert?: unknown;
    replace?: unknown;
    newText?: unknown;
  };
  if (typeof edit.newText !== "string") return null;
  // InsertReplaceEdit has separate insertion and replacement ranges. The
  // selected completion replaces the existing word, so use its replace range
  // while preserving the same strict encoding validation as TextEdit.
  const rangeValue = edit.range ?? edit.replace;
  if (!rangeValue || typeof rangeValue !== "object") return null;
  const range = rangeValue as { start?: unknown; end?: unknown };
  if (!range.start || !range.end || typeof range.start !== "object" || typeof range.end !== "object") return null;
  const start = range.start as { line?: unknown; character?: unknown };
  const end = range.end as { line?: unknown; character?: unknown };
  if (
    !Number.isInteger(start.line) || !Number.isInteger(start.character)
    || !Number.isInteger(end.line) || !Number.isInteger(end.character)
  ) return null;
  return {
    range: {
      start: { line: start.line as number, character: start.character as number },
      end: { line: end.line as number, character: end.character as number },
    },
    newText: edit.newText,
  };
}

function plainCompletionText(item: LspCompletionItem, textEdit: CompletionTextEdit | null): string {
  // Snippet syntax is not safe to place directly in a plain CodeMirror
  // completion. The label is the server-provided human/plain fallback.
  if (item.insertTextFormat === 2) return item.label;
  return textEdit?.newText ?? item.insertText ?? item.label;
}

function strictOffsetForPosition(
  text: string,
  position: LspPosition,
  encoding: LspPositionEncoding,
): number | null {
  if (position.line < 0 || position.character < 0) return null;
  const lines = text.split("\n");
  const lineText = lines[position.line];
  if (lineText === undefined) return null;
  const maxCharacter = encoding === "utf-8"
    ? new TextEncoder().encode(lineText).length
    : lineText.length;
  if (position.character > maxCharacter) return null;
  const offset = offsetForPosition(text, position, encoding);
  const roundTrip = positionForOffset(text, offset, encoding);
  return roundTrip.line === position.line && roundTrip.character === position.character ? offset : null;
}

export interface CompletionOptionsContext {
  text: string;
  encoding: LspPositionEncoding;
  /** Called when CodeMirror accepts a completion, before dispatching its edit. */
  isCurrent?: () => boolean;
}

export function completionOptions(
  result: LspCompletionResult,
  context?: CompletionOptionsContext,
): Completion[] {
  return result.items.flatMap((item) => {
    const textEdit = completionTextEdit(item);
    const text = plainCompletionText(item, textEdit);
    if (!text) return [];
    const editRange = context && textEdit
      ? {
          from: strictOffsetForPosition(context.text, textEdit.range.start, context.encoding),
          to: strictOffsetForPosition(context.text, textEdit.range.end, context.encoding),
        }
        : null;
    if (editRange && (editRange.from === null || editRange.to === null || editRange.from > editRange.to)) return [];
    const fixedRange = editRange && editRange.from !== null && editRange.to !== null
      ? { from: editRange.from, to: editRange.to }
      : null;
    const apply = context
      ? (view: EditorView, _completion: Completion, from: number, to: number) => {
          if (context.isCurrent && !context.isCurrent()) return;
          if (view.state.doc.toString() !== context.text) return;
          const replacement = fixedRange ?? { from, to };
          if (replacement.from < 0 || replacement.to > view.state.doc.length || replacement.from > replacement.to) return;
          view.dispatch({ changes: { from: replacement.from, to: replacement.to, insert: text } });
        }
      : text;
    return [{
      label: item.label,
      detail: item.detail ?? undefined,
      type: "text" as const,
      apply,
    }];
  });
}

export function hoverText(result: LspHoverResult | null): string | null {
  return result?.text?.trim() ? result.text : null;
}

/** Decode a file URI only for navigation display; native filtering remains authoritative. */
export function pathFromFileUri(uri: string): string | null {
  try {
    const parsed = new URL(uri);
    if (parsed.protocol !== "file:") return null;
    const path = decodeURIComponent(parsed.pathname);
    if (parsed.host) return `//${parsed.host}${path}`;
    return /^\/[A-Za-z]:\//u.test(path) ? path.slice(1) : path;
  } catch {
    return null;
  }
}
