/**
 * A bounded formatter for pasted, pipe-delimited Markdown table rows.
 *
 * This is intentionally a row formatter rather than a full Markdown parser:
 * it does not inspect files, render HTML, evaluate inline syntax, or reorder
 * user data.  Source row/column order is preserved and all derived spacing is
 * deterministic.
 */

export const MARKDOWN_TABLE_LIMITS = Object.freeze({
  maxInputBytes: 1_000_000,
  maxRows: 1_000,
  maxColumns: 100,
  maxCellCodePoints: 4_096,
  maxOutputBytes: 4_000_000,
});

export type MarkdownTableErrorCode =
  | "INPUT_TOO_LARGE"
  | "TOO_MANY_ROWS"
  | "TOO_MANY_COLUMNS"
  | "CELL_TOO_LARGE"
  | "INVALID_UNICODE"
  | "INVALID_CONTROL"
  | "MALFORMED_ROW"
  | "MALFORMED_SEPARATOR"
  | "OUTPUT_TOO_LARGE"
  | "FORMAT_FAILED";

export interface MarkdownTableError {
  code: MarkdownTableErrorCode;
  message: string;
}

export interface MarkdownTableResult {
  output: string;
  error: MarkdownTableError | null;
}

type Alignment = "default" | "left" | "center" | "right";

const ERROR_MESSAGES: Readonly<Record<MarkdownTableErrorCode, string>> = Object.freeze({
  INPUT_TOO_LARGE: "입력 크기가 제한을 초과했습니다.",
  TOO_MANY_ROWS: "표의 행 수가 제한을 초과했습니다.",
  TOO_MANY_COLUMNS: "표의 열 수가 제한을 초과했습니다.",
  CELL_TOO_LARGE: "표 셀의 길이가 제한을 초과했습니다.",
  INVALID_UNICODE: "표 입력의 Unicode가 올바르지 않습니다.",
  INVALID_CONTROL: "표 셀에 허용되지 않는 제어 문자가 있습니다.",
  MALFORMED_ROW: "Markdown 표 행 형식이 올바르지 않습니다.",
  MALFORMED_SEPARATOR: "Markdown 표 정렬 구분 행이 올바르지 않습니다.",
  OUTPUT_TOO_LARGE: "변환 결과가 크기 제한을 초과했습니다.",
  FORMAT_FAILED: "변환을 완료하지 못했습니다.",
});

const UTF8_ENCODER = new TextEncoder();
const CONTROL_CHARACTER = /[\u0000-\u001f\u007f-\u009f\u2028\u2029]/u;
const SEPARATOR_TOKEN = /^:?-{3,}:?$/u;
const SEPARATOR_CANDIDATE = /^:?-{1,}:?$/u;

interface ParsedRow {
  cells: string[];
}

interface ParsedTable {
  rows: ParsedRow[];
  separator: string[] | null;
}

function utf8ByteLength(value: string): number {
  return UTF8_ENCODER.encode(value).byteLength;
}

function error(code: MarkdownTableErrorCode): MarkdownTableResult {
  return { output: "", error: { code, message: ERROR_MESSAGES[code] } };
}

/** TextEncoder replaces lone surrogates; reject those instead of changing input silently. */
function hasWellFormedUnicode(value: string): boolean {
  for (let index = 0; index < value.length; index += 1) {
    const code = value.charCodeAt(index);
    if (code >= 0xd800 && code <= 0xdbff) {
      const next = value.charCodeAt(index + 1);
      if (next < 0xdc00 || next > 0xdfff) return false;
      index += 1;
    } else if (code >= 0xdc00 && code <= 0xdfff) {
      return false;
    }
  }
  return true;
}

function isEscaped(value: string, position: number): boolean {
  let slashCount = 0;
  for (let index = position - 1; index >= 0 && value[index] === "\\"; index -= 1) {
    slashCount += 1;
  }
  return slashCount % 2 === 1;
}

interface BacktickRun {
  start: number;
  length: number;
  next: number;
}

/**
 * Find table delimiters while treating matched Markdown code spans as cell data.
 * Backtick runs are indexed once so adversarial unmatched runs remain linear.
 */
function pipeDelimiters(value: string): number[] {
  const runs: BacktickRun[] = [];
  for (let index = 0; index < value.length;) {
    if (value[index] !== "`" || isEscaped(value, index)) {
      index += 1;
      continue;
    }
    let end = index + 1;
    while (end < value.length && value[end] === "`") end += 1;
    runs.push({ start: index, length: end - index, next: -1 });
    index = end;
  }

  const nextIndexByLength = new Map<number, number>();
  for (let index = runs.length - 1; index >= 0; index -= 1) {
    const run = runs[index];
    run.next = nextIndexByLength.get(run.length) ?? -1;
    nextIndexByLength.set(run.length, index);
  }

  const delimiters: number[] = [];
  let runIndex = 0;
  for (let index = 0; index < value.length;) {
    while (runIndex < runs.length && runs[runIndex].start < index) runIndex += 1;
    const run = runs[runIndex];
    if (run?.start === index) {
      if (run.next >= 0) {
        const closing = runs[run.next];
        index = closing.start + closing.length;
        runIndex = run.next + 1;
        continue;
      }
      index += run.length;
      runIndex += 1;
      continue;
    }
    if (value[index] === "|" && !isEscaped(value, index)) delimiters.push(index);
    index += 1;
  }
  return delimiters;
}

/** Only an escaped pipe is unescaped; all other backslashes remain unchanged. */
function unescapePipe(value: string): string {
  let result = "";
  for (let index = 0; index < value.length; index += 1) {
    if (value[index] === "\\" && value[index + 1] === "|") {
      result += "|";
      index += 1;
    } else {
      result += value[index];
    }
  }
  return result;
}

function splitRow(line: string): string[] | null {
  const trimmed = line.trim();
  const rowDelimiters = pipeDelimiters(trimmed);
  if (rowDelimiters.length === 0) return null;

  const bodyStart = rowDelimiters[0] === 0 ? 1 : 0;
  const bodyEnd = rowDelimiters[rowDelimiters.length - 1] === trimmed.length - 1
    ? trimmed.length - 1
    : trimmed.length;
  const body = trimmed.slice(bodyStart, bodyEnd);
  const delimiters = pipeDelimiters(body);

  const cells: string[] = [];
  let start = 0;
  for (const delimiter of delimiters) {
    cells.push(unescapePipe(body.slice(start, delimiter).trim()));
    start = delimiter + 1;
  }
  cells.push(unescapePipe(body.slice(start).trim()));
  return cells;
}

function isSeparatorCell(cell: string): boolean {
  return SEPARATOR_TOKEN.test(cell.trim());
}

function looksLikeSeparatorRow(cells: readonly string[]): boolean {
  return cells.length > 0 && cells.some((cell) => SEPARATOR_CANDIDATE.test(cell.trim()));
}

function parseRows(input: string): MarkdownTableResult | ParsedTable {
  const normalized = input.replace(/\r\n?/gu, "\n");
  const sourceLines = normalized.split("\n");
  for (const line of sourceLines) {
    // Check before trim() so a control at a row boundary is not hidden.
    if (CONTROL_CHARACTER.test(line)) return error("INVALID_CONTROL");
  }

  let firstLine = 0;
  let lastLine = sourceLines.length;
  while (firstLine < lastLine && sourceLines[firstLine].trim() === "") firstLine += 1;
  while (lastLine > firstLine && sourceLines[lastLine - 1].trim() === "") lastLine -= 1;
  if (firstLine === lastLine) return { rows: [], separator: null };

  // One additional line is allowed for the optional Markdown alignment row.
  if (lastLine - firstLine > MARKDOWN_TABLE_LIMITS.maxRows + 1) {
    return error("TOO_MANY_ROWS");
  }

  const parsed: ParsedRow[] = [];
  for (let lineIndex = firstLine; lineIndex < lastLine; lineIndex += 1) {
    const line = sourceLines[lineIndex];
    if (line.trim() === "") return error("MALFORMED_ROW");
    const cells = splitRow(line);
    if (!cells || cells.length === 0) return error("MALFORMED_ROW");
    if (cells.length > MARKDOWN_TABLE_LIMITS.maxColumns) return error("TOO_MANY_COLUMNS");
    for (const cell of cells) {
      if ([...cell].length > MARKDOWN_TABLE_LIMITS.maxCellCodePoints) {
        return error("CELL_TOO_LARGE");
      }
    }
    parsed.push({ cells });
  }

  let separator: string[] | null = null;
  if (parsed.length >= 2 && looksLikeSeparatorRow(parsed[1].cells)) {
    if (!parsed[1].cells.every((cell) => isSeparatorCell(cell))) {
      return error("MALFORMED_SEPARATOR");
    }
    separator = parsed[1].cells;
    parsed.splice(1, 1);
  }

  if (parsed.length > MARKDOWN_TABLE_LIMITS.maxRows) return error("TOO_MANY_ROWS");
  return { rows: parsed, separator };
}

function escapeCell(value: string): string {
  return value.replace(/\|/gu, "\\|");
}

function displayWidth(value: string): number {
  return [...value].length;
}

function padCell(value: string, width: number, alignment: Alignment): string {
  const padding = Math.max(0, width - displayWidth(value));
  if (alignment === "right") return `${" ".repeat(padding)}${value}`;
  if (alignment === "center") {
    const left = Math.ceil(padding / 2);
    return `${" ".repeat(left)}${value}${" ".repeat(padding - left)}`;
  }
  return `${value}${" ".repeat(padding)}`;
}

function separatorFor(width: number, alignment: Alignment): string {
  const minimum = alignment === "center" ? 5 : 3;
  const markerCount = alignment === "center"
    ? 2
    : alignment === "left" || alignment === "right"
      ? 1
      : 0;
  const hyphens = Math.max(3, width - markerCount, minimum - markerCount);
  if (alignment === "center") return `:${"-".repeat(hyphens)}:`;
  if (alignment === "right") return `${"-".repeat(hyphens)}:`;
  if (alignment === "left") return `:${"-".repeat(hyphens)}`;
  return "-".repeat(hyphens);
}

function alignmentFor(cell: string): Alignment {
  const trimmed = cell.trim();
  const left = trimmed.startsWith(":");
  const right = trimmed.endsWith(":");
  if (left && right) return "center";
  if (right) return "right";
  if (left) return "left";
  return "default";
}

function columnCount(rows: readonly ParsedRow[], separator: readonly string[] | null): number {
  return Math.max(1, ...rows.map((row) => row.cells.length), ...(separator ? [separator.length] : []));
}

/**
 * Append one output line while it is still bounded.  This avoids constructing
 * a potentially huge padded string before noticing the output limit.
 */
function appendBoundedLine(lines: string[], line: string, byteLength: { value: number }): boolean {
  const increment = utf8ByteLength(line) + (lines.length > 0 ? 1 : 0);
  if (byteLength.value + increment > MARKDOWN_TABLE_LIMITS.maxOutputBytes) return false;
  lines.push(line);
  byteLength.value += increment;
  return true;
}

/** Format rows into a canonical, padded Markdown table without reordering data. */
export function formatMarkdownTable(input: string): MarkdownTableResult {
  if (typeof input !== "string") return error("FORMAT_FAILED");
  // For well-formed text UTF-8 bytes are never smaller than UTF-16 code units;
  // this cheap guard avoids allocating an encoder buffer for an enormous paste.
  if (input.length > MARKDOWN_TABLE_LIMITS.maxInputBytes) return error("INPUT_TOO_LARGE");
  if (!hasWellFormedUnicode(input)) return error("INVALID_UNICODE");
  if (utf8ByteLength(input) > MARKDOWN_TABLE_LIMITS.maxInputBytes) {
    return error("INPUT_TOO_LARGE");
  }

  const parsed = parseRows(input);
  if ("error" in parsed) return parsed;
  if (parsed.rows.length === 0) return { output: "", error: null };

  const columns = columnCount(parsed.rows, parsed.separator);
  if (columns > MARKDOWN_TABLE_LIMITS.maxColumns) return error("TOO_MANY_COLUMNS");

  const alignments: Alignment[] = Array.from({ length: columns }, (_, index) => (
    parsed.separator && index < parsed.separator.length
      ? alignmentFor(parsed.separator[index])
      : "default"
  ));
  const escapedRows = parsed.rows.map((row) => (
    Array.from({ length: columns }, (_, index) => escapeCell(row.cells[index] ?? ""))
  ));
  const widths = Array.from({ length: columns }, () => 0);
  for (const row of escapedRows) {
    for (let index = 0; index < columns; index += 1) {
      widths[index] = Math.max(widths[index], displayWidth(row[index]));
    }
  }

  const lines: string[] = [];
  const outputBytes = { value: 0 };
  const appendRow = (row: readonly string[]): boolean => {
    const cells = Array.from({ length: columns }, (_, index) => (
      padCell(row[index] ?? "", widths[index], alignments[index])
    ));
    return appendBoundedLine(lines, `| ${cells.join(" | ")} |`, outputBytes);
  };

  if (!appendRow(escapedRows[0])) return error("OUTPUT_TOO_LARGE");
  const separatorCells = widths.map((width, index) => separatorFor(width, alignments[index]));
  if (!appendBoundedLine(lines, `| ${separatorCells.join(" | ")} |`, outputBytes)) {
    return error("OUTPUT_TOO_LARGE");
  }
  for (let index = 1; index < escapedRows.length; index += 1) {
    if (!appendRow(escapedRows[index])) return error("OUTPUT_TOO_LARGE");
  }

  return { output: lines.join("\n"), error: null };
}

export function markdownTableErrorMessage(code: MarkdownTableErrorCode): string {
  return ERROR_MESSAGES[code] ?? ERROR_MESSAGES.FORMAT_FAILED;
}
