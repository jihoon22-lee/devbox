import type { Doc, Encoding, LineEnding } from "../types";
import { encodingLabel, languageForPath, languageLabel } from "../editor/extensions";

interface StatusBarProps {
  doc: Doc | null;
  zoom: number;
  onEncodingReopen?: (docId: string, encoding: Encoding) => void;
  onEncodingConvert?: (docId: string, encoding: Encoding) => void;
  onLineEndingChange?: (docId: string, lineEnding: LineEnding) => void;
}

export const ENCODING_OPTIONS: ReadonlyArray<{ value: string; label: string; encoding: Encoding }> = [
  { value: "utf8", label: "UTF-8", encoding: { encodingKind: "utf8", bom: false } },
  { value: "utf8-bom", label: "UTF-8 BOM", encoding: { encodingKind: "utf8", bom: true } },
  // The five-item picker treats UTF-16 LE/BE as the BOM-bearing file formats.
  // The separate `bom` bit is still used on the wire; no `utf16LeBom` kind is
  // invented, and a BOM-less document remains representable in its metadata.
  { value: "utf16-le", label: "UTF-16 LE", encoding: { encodingKind: "utf16Le", bom: true } },
  { value: "utf16-be", label: "UTF-16 BE", encoding: { encodingKind: "utf16Be", bom: true } },
  { value: "cp949", label: "CP949", encoding: { encodingKind: "cp949", bom: false } },
];

export function encodingOptionValue(encoding: Encoding): string {
  if (encoding.encodingKind === "utf8" && encoding.bom) return "utf8-bom";
  if (encoding.encodingKind === "utf16Le") return "utf16-le";
  if (encoding.encodingKind === "utf16Be") return "utf16-be";
  return encoding.encodingKind;
}

/**
 * CodeMirror positions are JavaScript string offsets (UTF-16 code units).
 * Keep the same convention in the status bar so an emoji counts as two
 * columns, just as it does inside the editor.
 */
export function cursorPosition(text: string, cursor: number): { line: number; column: number } {
  const offset = Number.isFinite(cursor)
    ? Math.min(Math.max(0, Math.trunc(cursor)), text.length)
    : 0;
  const lineStart = text.lastIndexOf("\n", Math.max(0, offset - 1));
  return {
    line: text.slice(0, offset).split("\n").length,
    column: offset - lineStart,
  };
}

export function encodingForOption(
  value: string,
  intent: "reopen" | "convert",
): Encoding | null {
  const selected = ENCODING_OPTIONS.find((option) => option.value === value)?.encoding;
  if (!selected) return null;

  // A BOM-less UTF-16 document cannot be auto-detected. Reopening is the
  // explicit escape hatch for that case, while conversion creates the
  // conventional BOM-bearing UTF-16 file format.
  if (
    intent === "reopen"
    && (selected.encodingKind === "utf16Le" || selected.encodingKind === "utf16Be")
  ) {
    return { ...selected, bom: false };
  }
  return selected;
}

export default function StatusBar({
  doc,
  zoom,
  onEncodingReopen,
  onEncodingConvert,
  onLineEndingChange,
}: StatusBarProps) {
  if (!doc) {
    return <footer className="status-bar">문서를 열면 편집 상태가 표시됩니다</footer>;
  }
  const position = cursorPosition(doc.text, doc.cursor);
  return (
    <footer className="status-bar">
      <span>{languageLabel(languageForPath(doc.path))}</span>
      <span className="cursor-position" aria-label={`커서 위치 ${position.line}행 ${position.column}열`}>
        Ln {position.line}, Col {position.column}
      </span>
      <span>{encodingLabel(doc.encoding.encodingKind)}{doc.encoding.bom ? " BOM" : ""}</span>
      <span>{doc.lineEnding.toUpperCase()}</span>
      <span>{doc.readOnly ? "읽기 전용" : doc.dirty ? "수정됨" : "저장됨"}</span>
      {doc.lossy && <span className="status-warning">인코딩 손실 가능성</span>}
      {doc.durabilityWarning && <span className="status-warning">{doc.durabilityWarning}</span>}
      <span>{zoom}%</span>
      <span className="status-path" title={doc.path}>{doc.path}</span>
      <label className="status-control">
        <span className="sr-only">인코딩 다시 열기</span>
        <select
          aria-label="인코딩 다시 열기"
          value=""
          onChange={(event) => {
            const encoding = encodingForOption(event.currentTarget.value, "reopen");
            if (encoding) onEncodingReopen?.(doc.id, encoding);
          }}
        >
          <option value="">인코딩 다시 열기…</option>
          {ENCODING_OPTIONS.map((option) => <option key={option.value} value={option.value}>{option.label}</option>)}
        </select>
      </label>
      <label className="status-control">
        <span className="sr-only">저장 인코딩</span>
        <select
          aria-label="저장 인코딩"
          value={encodingOptionValue(doc.encoding)}
          disabled={doc.readOnly}
          onChange={(event) => {
            const encoding = encodingForOption(event.currentTarget.value, "convert");
            if (encoding) onEncodingConvert?.(doc.id, encoding);
          }}
        >
          {ENCODING_OPTIONS.map((option) => <option key={option.value} value={option.value}>{option.label}</option>)}
        </select>
      </label>
      <label className="status-control">
        <span className="sr-only">줄바꿈 변환</span>
        <select
          aria-label="줄바꿈 변환"
          value={doc.lineEnding}
          disabled={doc.readOnly}
          onChange={(event) => onLineEndingChange?.(doc.id, event.currentTarget.value as LineEnding)}
        >
          <option value="lf">LF</option>
          <option value="crlf">CRLF</option>
          <option value="cr">CR</option>
        </select>
      </label>
    </footer>
  );
}
