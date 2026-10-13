import { useState } from "react";
import s from "./DiffView.module.css";
import { type DiffFile, type DiffRange, selectDiffRange } from "./diff";
export function DiffView({
  file,
  onSelectLines,
  maxLines = 5000,
}: {
  file: DiffFile;
  onSelectLines?: (range: DiffRange) => void;
  maxLines?: number;
}) {
  const [anchor, setAnchor] = useState<{ side: "old" | "new"; line: number; identity: string }>();
  const identity = JSON.stringify([file.oldPath, file.newPath, file.hunks]);
  const all = file.hunks.flatMap((hunk) => hunk.lines);
  const limit = Math.max(0, Math.floor(Number.isFinite(maxLines) ? maxLines : 5000));
  const visible = all.slice(0, limit);
  let count = 0;
  const select = (side: "old" | "new", line: number, shift: boolean) => {
    const range = selectDiffRange(
      visible,
      side,
      shift && anchor?.side === side && anchor.identity === identity ? anchor.line : line,
      line,
    );
    if (range) onSelectLines?.(range);
    else onSelectLines?.({ side, from: line, to: line, text: visible.find((item) => item[side] === line)?.text ?? "" });
    if (!shift || anchor?.side !== side || anchor.identity !== identity || !range) setAnchor({ side, line, identity });
  };
  if (file.binary) return <p>바이너리 파일의 내용은 표시하지 않습니다.</p>;
  if (!all.length) return <p>표시할 텍스트 변경이 없습니다. 파일 이름·권한 또는 빈 파일의 변경을 확인하세요.</p>;
  return (
    <div className={s.view} aria-label="파일 변경 내용">
      {file.hunks.map((hunk, hi) => {
        const rows = hunk.lines.slice(0, Math.max(0, limit - count));
        count += hunk.lines.length;
        return (
          rows.length > 0 && (
            <div key={`${hi}:${hunk.header}`}>
              <div className={s.header}>{hunk.header}</div>
              {rows.map((line, i) => (
                <div key={`${hi}:${i}`} className={s.line} data-kind={line.kind}>
                  {(["old", "new"] as const).map((side) =>
                    line[side] === undefined ? (
                      <span key={side} className={s.number} />
                    ) : (
                      <button
                        key={side}
                        type="button"
                        className={s.number}
                        aria-label={`${side === "old" ? "변경 전" : "변경 후"} ${line[side]}줄`}
                        onClick={(event) => select(side, line[side]!, event.shiftKey)}
                      >
                        {line[side]}
                      </button>
                    ),
                  )}
                  <span className={s.marker}>{line.kind === "add" ? "+" : line.kind === "del" ? "−" : " "}</span>
                  <code>{line.text}</code>
                </div>
              ))}
            </div>
          )
        );
      })}
      {all.length > limit && <p role="status">변경 내용이 길어 {limit.toLocaleString("ko")}줄까지만 표시합니다.</p>}
    </div>
  );
}
