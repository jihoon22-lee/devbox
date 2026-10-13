import type { DiffRange } from "../../../ui/diff";
export function quoteForPrompt(path: string, range: DiffRange) {
  const where = range.from === range.to ? `L${range.from}` : `L${range.from}-${range.to}`;
  const which = range.side === "old" ? " (변경 전)" : "";
  const longest = Math.max(0, ...Array.from(range.text.matchAll(/`+/g), (match) => match[0].length));
  const fence = "`".repeat(Math.max(3, longest + 1));
  const pathFence = "`".repeat(Math.max(1, ...Array.from(path.matchAll(/`+/g), (match) => match[0].length + 1)));
  return `${pathFence}${path}${pathFence} ${where}${which}:\n${fence}\n${range.text}\n${fence}\n`;
}
