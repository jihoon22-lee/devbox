import { expect, it } from "vitest";
import { validateBatch, type OutputBatch } from "./terminalReplay";
const batch = (cursor: number, truncated = false): OutputBatch => ({
  frames: [{ sequence: cursor, data: "retained" }],
  cursor,
  truncated,
  closed: false,
  more: false,
});
it("requires a gap marker for skipped frames and preserves contiguous cursors", () => {
  expect(() => validateBatch(batch(5), 0)).toThrow();
  expect(validateBatch(batch(5, true), 0)).toBe("retained");
  expect(validateBatch(batch(6), 5)).toBe("retained");
  expect(() => validateBatch(batch(5), 5)).toThrow();
});
it("rejects oversized, malformed and inconsistent frames before rendering", () => {
  const valid = batch(1);
  for (const invalid of [
    { ...valid, frames: [{ sequence: 1, data: "😀".repeat(16385) }] },
    { ...valid, frames: Array.from({ length: 513 }, (_, i) => ({ sequence: i + 1, data: "x" })), cursor: 513 },
    { ...valid, cursor: Number.MAX_SAFE_INTEGER + 1 },
    { ...valid, frames: [] },
    { ...valid, more: true, closed: true },
    { ...valid, frames: [{ sequence: 2, data: "x" }] },
  ])
    expect(() => validateBatch(invalid, 0)).toThrow();
  expect(validateBatch({ frames: [], cursor: 1, truncated: false, more: false, closed: true }, 1)).toBe("");
});
