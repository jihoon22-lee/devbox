/** 응답 비교(ResponseCompare)용 줄 단위 diff. 결과는 `ui/DiffView`가 그리는 `DiffFile` 모양이다. */
import type { DiffFile, DiffLine } from "../../../../ui/diff";

const DEFAULT_MAX_LINES = 2000;

export function lineDiff(
  before: string,
  after: string,
  options: { oldLabel?: string; newLabel?: string; maxLines?: number } = {},
): DiffFile {
  const max = Math.max(1, Math.floor(options.maxLines ?? DEFAULT_MAX_LINES));
  const allA = before.split(/\r?\n/);
  const allB = after.split(/\r?\n/);
  const a = allA.slice(0, max);
  const b = allB.slice(0, max);
  const truncated = allA.length > max || allB.length > max;
  const file: DiffFile = {
    oldPath: options.oldLabel ?? null,
    newPath: options.newLabel ?? null,
    binary: false,
    hunks: [],
  };
  if (before === after) return file;
  // 최장 공통 부분열(뒤에서부터 길이 표).
  const width = b.length + 1;
  const table = new Uint32Array((a.length + 1) * width);
  for (let i = a.length - 1; i >= 0; i--)
    for (let j = b.length - 1; j >= 0; j--)
      table[i * width + j] =
        a[i] === b[j]
          ? table[(i + 1) * width + j + 1]! + 1
          : Math.max(table[(i + 1) * width + j]!, table[i * width + j + 1]!);
  const lines: DiffLine[] = [];
  let i = 0;
  let j = 0;
  while ((i < a.length || j < b.length) && lines.length < max) {
    if (i < a.length && j < b.length && a[i] === b[j]) lines.push({ kind: "ctx", old: ++i, new: ++j, text: a[i - 1]! });
    else if (i < a.length && (j >= b.length || table[(i + 1) * width + j]! >= table[i * width + j + 1]!))
      lines.push({ kind: "del", old: ++i, text: a[i - 1]! });
    else lines.push({ kind: "add", new: ++j, text: b[j - 1]! });
  }
  if (truncated || i < a.length || j < b.length)
    lines.push({ kind: "meta", text: `줄이 많아 처음 ${max}줄만 비교했습니다.` });
  return { ...file, hunks: [{ header: "@@ 응답 비교 @@", lines }] };
}
