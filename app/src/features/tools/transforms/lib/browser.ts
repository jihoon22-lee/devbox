import type { DiffHunk } from "./browserTypes";
import { generateIdentifiers, type IdentifierOptions } from "./ids";
import { lineDiff } from "./lineDiff";

export { hash } from "./hash";
export { browserHmacGenerate as hmacGenerate, browserHmacVerify as hmacVerify } from "./hmac";
export { browserVerifyJwt as verifyJwt } from "./jwt";
export { generateQr } from "./qr";
export async function generateIds(options: IdentifierOptions): Promise<string[]> {
  return generateIdentifiers(options);
}
export async function readClipboardText(): Promise<string> {
  return navigator.clipboard.readText();
}
export async function diff(a: string, b: string): Promise<DiffHunk[]> {
  if (a.length + b.length > 1024 * 1024 || a.split("\n").length > 2000 || b.split("\n").length > 2000)
    throw new Error("비교할 입력이 너무 큽니다.");
  if (a === b)
    return [{ kind: 0, old_start: 0, old_end: a.split("\n").length, new_start: 0, new_end: b.split("\n").length }];
  const hunks: DiffHunk[] = [];
  let old = 0,
    next = 0;
  const lines = lineDiff(a, b).hunks.flatMap((h) => h.lines);
  if (lines.some((line) => line.kind === "meta")) throw new Error("비교할 줄이 너무 많습니다.");
  for (const line of lines) {
    if (line.kind === "meta") continue;
    const kind = line.kind === "ctx" ? 0 : line.kind === "add" ? 1 : 2;
    const previous = hunks[hunks.length - 1];
    const h =
      previous?.kind === kind ? previous : { kind, old_start: old, old_end: old, new_start: next, new_end: next };
    if (h !== previous) hunks.push(h);
    if (kind !== 1) old++;
    if (kind !== 2) next++;
    h.old_end = old;
    h.new_end = next;
  }
  return hunks;
}
