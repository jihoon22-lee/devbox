import type { ReactNode } from "react";
import type { LogLine, LogSource } from "../../rpc/gen/rpc";
import s from "./LogView.module.css";
import { ansiToSpans, sourceKey } from "./model";

export interface LogLocation {
  path: string;
  line: number;
  column?: number;
  source: LogSource;
}
export function LogBody({ line, onOpenLocation }: { line: LogLine; onOpenLocation?: (location: LogLocation) => void }) {
  // A single unusually long line must not create millions of DOM nodes.
  const spans = ansiToSpans(line.text.slice(0, 32_768));
  const plain = spans.map((span) => span.text).join("");
  const content: ReactNode[] = [];
  let spanIndex = 0;
  let spanStart = 0;
  // Consume styled intervals in order, so linking many diagnostics remains linear.
  const styled = (start: number, end: number) => {
    const parts: ReactNode[] = [];
    while (spanIndex < spans.length && spanStart < end) {
      const span = spans[spanIndex]!;
      const spanEnd = spanStart + span.text.length;
      const from = Math.max(start, spanStart);
      const to = Math.min(end, spanEnd);
      if (to > from) {
        parts.push(
          <span key={from} style={span.style}>
            {span.text.slice(from - spanStart, to - spanStart)}
          </span>,
        );
      }
      if (spanEnd > end) break;
      spanStart = spanEnd;
      spanIndex++;
    }
    return parts;
  };
  const expression = /("([^"\r\n]+)"|'([^'\r\n]+)'|(?:[A-Za-z]:[\\/])?[^\s"'<>()[\]{}:]+):([1-9]\d*)(?::([1-9]\d*))?/gu;
  let start = 0;
  if (onOpenLocation) {
    for (const match of plain.matchAll(expression)) {
      // A URL's authority/port is not a file location.
      if (match.index > 0 && plain[match.index - 1] === ":") continue;
      const row = Number(match[4]);
      const column = match[5] === undefined ? undefined : Number(match[5]);
      const end = match.index + match[0].length;
      if (onOpenLocation && Number.isSafeInteger(row) && (column === undefined || Number.isSafeInteger(column))) {
        content.push(...styled(start, match.index));
        content.push(
          <button
            key={`location-${match.index}`}
            type="button"
            className={s.location}
            aria-label={match[0]}
            onClick={() =>
              onOpenLocation({
                path: match[2] ?? match[3] ?? match[1]!,
                line: row,
                ...(column === undefined ? {} : { column }),
                source: line.src,
              })
            }
          >
            {styled(match.index, end)}
          </button>,
        );
        start = end;
      }
    }
  }
  content.push(...styled(start, plain.length));
  return (
    <div className={s.row} title={sourceKey(line.src)}>
      <time className={s.time} dateTime={line.ts}>
        {line.ts
          .replace("T", " ")
          .replace(/(?:\.\d+)?Z$/, "")
          .slice(0, 19)}
      </time>
      <span className={s.stream}>{line.stream}</span>
      <span className={s.body}>
        {content}
        {line.text.length > 32_768 && <span className={s.truncated}> … (표시 길이 제한)</span>}
      </span>
    </div>
  );
}
