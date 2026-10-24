import { expect, test } from "vitest";
import { noteTaskPrompt } from "./noteTask";

test("selected text wins, otherwise the whole note is used", () => {
  expect(noteTaskPrompt("whole note", "selection")).toEqual({ prompt: "selection", truncated: false });
  expect(noteTaskPrompt("whole note", "")).toEqual({ prompt: "whole note", truncated: false });
});
test("the 64 KiB prompt bound keeps UTF-8 characters whole and reports truncation", () => {
  const result = noteTaskPrompt("한🙂".repeat(20000), "");
  expect(new TextEncoder().encode(result.prompt).length).toBeLessThanOrEqual(65536);
  expect(result.prompt).not.toContain("\uFFFD");
  expect(result.truncated).toBe(true);
});
