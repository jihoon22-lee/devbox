import { expect, it } from "vitest";
import { regexMatches, segments } from "./regexEngine";

it("regex_ranges_are_utf16_and_cover_the_text", () => {
  const text = "a😀b😀c";
  const result = regexMatches("😀", "u", text);
  expect(result.matches).toEqual([
    { startUtf16: 1, endUtf16: 3 },
    { startUtf16: 4, endUtf16: 6 },
  ]);
  const parts = segments(text, result.matches);
  expect(parts.map((p) => p.text).join("")).toBe(text);
  expect(parts.filter((p) => p.matched).map((p) => p.text)).toEqual(["😀", "😀"]);
});
it("zero-width Unicode advances whole code points and non-Unicode advances code units", () => {
  expect(regexMatches("(?:)", "u", "😀").matches.map((m) => m.startUtf16)).toEqual([0, 2]);
  expect(regexMatches("(?:)", "", "😀").matches.map((m) => m.startUtf16)).toEqual([0, 1, 2]);
});
it("bounds matches and does not falsely mark an exact limit as truncated", () => {
  expect(regexMatches("a", "", "aaaa", 2)).toEqual({
    matches: [
      { startUtf16: 0, endUtf16: 1 },
      { startUtf16: 1, endUtf16: 2 },
    ],
    truncated: true,
  });
  expect(regexMatches("a", "", "aa", 2).truncated).toBe(false);
});
it("reports invalid expressions and flags", () => {
  expect(() => regexMatches("[", "", "x")).toThrow();
  expect(() => regexMatches("x", "z", "x")).toThrow();
});
