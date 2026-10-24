/** Pure engine called only from the dedicated worker (and unit tests). Offsets are UTF-16. */
export interface RegexRange {
  startUtf16: number;
  endUtf16: number;
}
export interface RegexResult {
  matches: RegexRange[];
  truncated: boolean;
}
export function regexMatches(pattern: string, flags: string, text: string, limit = 10_000): RegexResult {
  if (pattern.length > 8192 || new TextEncoder().encode(text).length > 1024 * 1024)
    throw new Error("입력이 너무 큽니다.");
  if (
    !Number.isInteger(limit) ||
    limit < 1 ||
    limit > 10_000 ||
    /[^gimsuy]/.test(flags) ||
    new Set(flags).size !== flags.length
  )
    throw new Error("정규식 옵션이 올바르지 않습니다.");
  const expression = new RegExp(pattern, flags.includes("g") ? flags : `${flags}g`);
  const matches: RegexRange[] = [];
  for (;;) {
    const found = expression.exec(text);
    if (!found) return { matches, truncated: false };
    if (matches.length === limit) return { matches, truncated: true };
    matches.push({ startUtf16: found.index, endUtf16: found.index + found[0].length });
    if (found[0].length === 0) {
      const point = text.codePointAt(expression.lastIndex);
      expression.lastIndex += expression.unicode && point !== undefined && point > 0xffff ? 2 : 1;
    }
  }
}
export function segments(text: string, matches: RegexRange[]): { text: string; matched: boolean }[] {
  const result: { text: string; matched: boolean }[] = [];
  let end = 0;
  for (const match of matches) {
    if (match.startUtf16 > end) result.push({ text: text.slice(end, match.startUtf16), matched: false });
    result.push({ text: text.slice(match.startUtf16, match.endUtf16), matched: true });
    end = match.endUtf16;
  }
  if (end < text.length || !result.length) result.push({ text: text.slice(end), matched: false });
  return result;
}
