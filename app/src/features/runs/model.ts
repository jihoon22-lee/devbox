import type { CSSProperties } from "react";
import type { LogLine, LogSource } from "../../rpc/gen/rpc";

export interface AnsiSpan {
  text: string;
  style: CSSProperties;
}
const COLORS = [
  "#000000",
  "#cd3131",
  "#0dbc79",
  "#e5e510",
  "#2472c8",
  "#bc3fbc",
  "#11a8cd",
  "#e5e5e5",
  "#666666",
  "#f14c4c",
  "#23d18b",
  "#f5f543",
  "#3b8eea",
  "#d670d6",
  "#29b8db",
  "#ffffff",
];
function color256(code: number): string | undefined {
  if (!Number.isInteger(code) || code < 0 || code > 255) return;
  if (code < 16) return COLORS[code];
  if (code >= 232)
    return `rgb(${Array(3)
      .fill(8 + (code - 232) * 10)
      .join(", ")})`;
  const n = code - 16;
  const levels = [0, 95, 135, 175, 215, 255];
  return `rgb(${[levels[Math.floor(n / 36)], levels[Math.floor(n / 6) % 6], levels[n % 6]].join(", ")})`;
}
function sgr(style: CSSProperties, values: number[]): CSSProperties {
  let next = { ...style };
  for (let i = 0; i < values.length; i++) {
    const code = values[i]!;
    if (code === 0) next = {};
    else if (code === 1) next.fontWeight = "bold";
    else if (code === 22) delete next.fontWeight;
    else if (code === 4) next.textDecoration = "underline";
    else if (code === 24) delete next.textDecoration;
    else if (code === 39) delete next.color;
    else if (code === 49) delete next.backgroundColor;
    else if (code >= 30 && code <= 37) next.color = COLORS[code - 30];
    else if (code >= 90 && code <= 97) next.color = COLORS[code - 90 + 8];
    else if (code >= 40 && code <= 47) next.backgroundColor = COLORS[code - 40];
    else if (code >= 100 && code <= 107) next.backgroundColor = COLORS[code - 100 + 8];
    else if (code === 38 || code === 48) {
      let color: string | undefined;
      const mode = values[++i];
      if (mode === 5) color = color256(values[++i]!);
      else if (mode === 2) {
        const rgb = values.slice(i + 1, i + 4);
        i += 3;
        if (rgb.length === 3 && rgb.every((n) => Number.isInteger(n) && n >= 0 && n <= 255)) {
          color = `rgb(${rgb.join(", ")})`;
        }
      }
      if (color) next[code === 38 ? "color" : "backgroundColor"] = color;
    }
  }
  return next;
}

/** SGR is the only executable terminal sequence retained, as ordinary React styles. */
export function ansiToSpans(text: string): AnsiSpan[] {
  const spans: AnsiSpan[] = [];
  let style: CSSProperties = {};
  let plain = "";
  const flush = () => {
    if (plain) spans.push({ text: plain, style: { ...style } });
    plain = "";
  };
  for (let i = 0; i < text.length; ) {
    const code = text.charCodeAt(i);
    const escaped = code === 27;
    const next = escaped ? text.charCodeAt(i + 1) : code;
    if ((escaped && next === 91) || code === 155) {
      const start = i + (escaped ? 2 : 1);
      i = start;
      while (i < text.length && !(text.charCodeAt(i) >= 64 && text.charCodeAt(i) <= 126)) i++;
      if (text[i] === "m" && /^[\d;]*$/.test(text.slice(start, i))) {
        flush();
        style = sgr(style, text.slice(start, i).split(";").map(Number));
      }
      i++;
    } else if ((escaped && [93, 80, 88, 94, 95].includes(next)) || [144, 152, 157, 158, 159].includes(code)) {
      i += escaped ? 2 : 1;
      while (i < text.length && ![7, 156].includes(text.charCodeAt(i)) && !(text[i] === "\x1b" && text[i + 1] === "\\"))
        i++;
      i += text[i] === "\x1b" ? 2 : 1;
    } else if (escaped) {
      i++;
      while (i < text.length && text.charCodeAt(i) >= 32 && text.charCodeAt(i) <= 47) i++;
      i++;
    } else {
      if (code === 9 || (code >= 32 && !(code >= 127 && code <= 159))) plain += text[i];
      i++;
    }
  }
  flush();
  return spans;
}

export function followState(
  follow: boolean,
  event: { type: "follow" | "newLines" } | { type: "userScrolled"; atBottom: boolean },
): boolean {
  return event.type === "follow" ? true : event.type === "userScrolled" ? event.atBottom : follow;
}
export function sourceKey(source: LogSource): string {
  return `${source.kind}:${source.kind === "run" ? source.runId : source.kind === "file" ? source.path : source.id}`;
}
function isSource(value: unknown): value is LogSource {
  if (!value || typeof value !== "object") return false;
  return (
    ("kind" in value && value.kind === "run" && "runId" in value && typeof value.runId === "string") ||
    ("kind" in value && value.kind === "file" && "path" in value && typeof value.path === "string") ||
    ("kind" in value && value.kind === "container" && "id" in value && typeof value.id === "string")
  );
}
/** NDJSON records may span many transport frames, including a split UTF-8 character. */
export class LogDecoder {
  private decoder = new TextDecoder("utf-8", { fatal: true });
  private pending = "";
  push(bytes: Uint8Array): LogLine[] {
    this.pending += this.decoder.decode(bytes, { stream: true });
    const lines: LogLine[] = [];
    let start = 0;
    let end = this.pending.indexOf("\n");
    while (end >= 0) {
      if (end - start > 20 * 1024 * 1024) throw new Error("로그 줄이 표시 한도를 초과했습니다.");
      const value: unknown = JSON.parse(this.pending.slice(start, end));
      if (
        !value ||
        typeof value !== "object" ||
        !("src" in value) ||
        !isSource(value.src) ||
        !("ts" in value) ||
        typeof value.ts !== "string" ||
        !("stream" in value) ||
        typeof value.stream !== "string" ||
        !["stdout", "stderr", "system"].includes(value.stream) ||
        !("text" in value) ||
        typeof value.text !== "string"
      )
        throw new Error("로그 형식을 읽을 수 없습니다.");
      lines.push(value as LogLine);
      start = end + 1;
      end = this.pending.indexOf("\n", start);
    }
    this.pending = this.pending.slice(start);
    if (this.pending.length > 20 * 1024 * 1024) throw new Error("로그 줄이 표시 한도를 초과했습니다.");
    return lines;
  }
  finish(): void {
    this.pending += this.decoder.decode();
    if (this.pending.length) throw new Error("로그 줄을 끝까지 받지 못했습니다.");
  }
}
export function retainLogLines(previous: LogLine[], incoming: LogLine[]): LogLine[] {
  const all = [...previous, ...incoming];
  let length = 0;
  let start = all.length;
  while (start > 0 && all.length - start < 10_000) {
    const next = all[start - 1]!;
    const size = next.text.length + next.ts.length + sourceKey(next.src).length;
    if (length + size > 4 * 1024 * 1024) break;
    length += size;
    start--;
  }
  return all.slice(start);
}
