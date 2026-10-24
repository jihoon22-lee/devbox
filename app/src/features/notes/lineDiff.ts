import type { DiffFile, DiffLine } from "../../ui/diff";

/** LCS cells above which the middle is shown as one replaced block (a 2 MiB note must not hang). */
const MAX_CELLS = 4_000_000;

/** A line diff of two texts for `DiffView`, by default `old` = disk and `new` = this buffer. */
export function lineDiff(oldText: string, newText: string, names = { old: "디스크", new: "내 버퍼" }): DiffFile {
  const a = oldText.split("\n");
  const b = newText.split("\n");
  let start = 0;
  while (start < a.length && start < b.length && a[start] === b[start]) start++;
  let end = 0;
  while (end < a.length - start && end < b.length - start && a[a.length - 1 - end] === b[b.length - 1 - end]) end++;
  const lines: DiffLine[] = [];
  for (let i = 0; i < start; i++) lines.push({ kind: "ctx", old: i + 1, new: i + 1, text: a[i] as string });
  const midA = a.slice(start, a.length - end);
  const midB = b.slice(start, b.length - end);
  const push = (kind: "del" | "add" | "ctx", i: number, j: number) => {
    if (kind === "del") lines.push({ kind, old: start + i + 1, text: midA[i] as string });
    else if (kind === "add") lines.push({ kind, new: start + j + 1, text: midB[j] as string });
    else lines.push({ kind, old: start + i + 1, new: start + j + 1, text: midA[i] as string });
  };
  if (midA.length * midB.length > MAX_CELLS) {
    midA.forEach((_, i) => push("del", i, 0));
    midB.forEach((_, j) => push("add", 0, j));
  } else {
    // Longest common subsequence from the end, then walk forward.
    const n = midA.length;
    const m = midB.length;
    const table = Array.from({ length: n + 1 }, () => new Uint32Array(m + 1));
    for (let i = n - 1; i >= 0; i--)
      for (let j = m - 1; j >= 0; j--)
        (table[i] as Uint32Array)[j] =
          midA[i] === midB[j]
            ? ((table[i + 1] as Uint32Array)[j + 1] as number) + 1
            : Math.max((table[i + 1] as Uint32Array)[j] as number, (table[i] as Uint32Array)[j + 1] as number);
    let i = 0;
    let j = 0;
    while (i < n || j < m) {
      if (i < n && j < m && midA[i] === midB[j]) push("ctx", i++, j++);
      else if (
        j < m &&
        (i >= n || ((table[i] as Uint32Array)[j + 1] as number) >= ((table[i + 1] as Uint32Array)[j] as number))
      )
        push("add", i, j++);
      else push("del", i++, j);
    }
  }
  for (let k = end; k > 0; k--)
    lines.push({ kind: "ctx", old: a.length - k + 1, new: b.length - k + 1, text: a[a.length - k] as string });
  return {
    oldPath: names.old,
    newPath: names.new,
    binary: false,
    hunks: [{ header: `${names.old} → ${names.new}`, lines }],
  };
}
