import { describe, expect, it } from "vitest";
import {
  generateLorem,
  LOREM_ERROR_MESSAGES,
  MAX_LOREM_COUNT,
  MAX_LOREM_COUNT_DIGITS,
  MAX_LOREM_OUTPUT_BYTES,
  parseLoremCount,
} from "./lorem";

describe("generateLorem", () => {
  it("generates an exact, repeatable sentence fixture", () => {
    const expected = [
      "Lorem ipsum dolor sit amet, consectetur adipiscing elit.",
      "Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.",
    ].join(" ");
    const first = generateLorem({ unit: "sentences", count: 2 });
    const second = generateLorem({ unit: "sentences", count: 2 });

    expect(first).toEqual(second);
    expect(first.output).toBe(expected);
    expect(first.error).toBeNull();
    expect(first.unitCount).toBe(2);
    expect(first.byteLength).toBe(new TextEncoder().encode(expected).byteLength);
  });

  it("keeps word counts exact and paragraph boundaries deterministic", () => {
    const words = generateLorem({ unit: "words", count: 10 });
    expect(words.output).toBe("Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do");
    expect(words.output.trim().split(/\s+/u)).toHaveLength(10);

    const paragraphs = generateLorem({ unit: "paragraphs", count: 2 });
    expect(paragraphs.output.split("\n\n")).toHaveLength(2);
    expect(paragraphs.output.split("\n\n").every((paragraph) => paragraph.endsWith("."))).toBe(true);
    expect(paragraphs.output.split("\n\n").every((paragraph) => (paragraph.match(/\./gu) ?? []).length === 5)).toBe(true);
    expect(generateLorem({ unit: "paragraphs", count: 2 })).toEqual(paragraphs);
  });

  it("rejects unsupported units and every invalid count with fixed empty results", () => {
    const invalidUnit = generateLorem({ unit: "characters" as never, count: 1 });
    expect(invalidUnit).toEqual({
      output: "",
      error: { code: "INVALID_UNIT", message: LOREM_ERROR_MESSAGES.INVALID_UNIT },
      unitCount: 0,
      byteLength: 0,
    });

    for (const count of [0, -1, 1.5, Number.NaN, Number.POSITIVE_INFINITY, MAX_LOREM_COUNT + 1]) {
      const result = generateLorem({ unit: "words", count });
      expect(result.output).toBe("");
      expect(result.error).toEqual({ code: "INVALID_COUNT", message: LOREM_ERROR_MESSAGES.INVALID_COUNT });
    }
  });

  it("keeps the largest supported generation within its UTF-8 byte bound", () => {
    const result = generateLorem({ unit: "paragraphs", count: MAX_LOREM_COUNT });
    expect(result.error).toBeNull();
    expect(result.byteLength).toBeLessThanOrEqual(MAX_LOREM_OUTPUT_BYTES);
    expect(result.output.split("\n\n")).toHaveLength(MAX_LOREM_COUNT);
  });
});

describe("parseLoremCount", () => {
  it("accepts only a bounded decimal token", () => {
    expect(parseLoremCount("1")).toBe(1);
    expect(parseLoremCount(" 100 ")).toBe(100);
    expect(parseLoremCount("1e2")).toBeNull();
    expect(parseLoremCount("+2")).toBeNull();
    expect(parseLoremCount("1.5")).toBeNull();
    expect(parseLoremCount("1".repeat(MAX_LOREM_COUNT_DIGITS + 1))).toBeNull();
    expect(parseLoremCount("101")).toBeNull();
    expect(parseLoremCount("")).toBeNull();
    expect(parseLoremCount(null as unknown as string)).toBeNull();
  });
});
