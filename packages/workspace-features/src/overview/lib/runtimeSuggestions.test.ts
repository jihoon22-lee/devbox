import { describe, expect, it } from "vitest";
import { formatRuntimeFreshness, mergeSuggestedPorts } from "./runtimeSuggestions";

describe("WSL runtime suggestion draft merge", () => {
  it("preserves existing order and appends sorted unique published ports", () => {
    expect(mergeSuggestedPorts("5173, 3000", [8080, 3000, 4000, 8080])).toEqual({
      nextText: "5173, 3000, 4000, 8080",
    });
  });

  it("never replaces invalid in-progress draft text", () => {
    const result = mergeSuggestedPorts("5173, nope", [8080]);
    expect(result.nextText).toBeNull();
    expect(result.error).toContain("기존 입력은 변경하지 않았습니다");
  });

  it("rejects unsafe ports and the profile bound", () => {
    expect(mergeSuggestedPorts("", [0]).nextText).toBeNull();
    const existing = Array.from({ length: 128 }, (_, index) => index + 1).join(", ");
    expect(mergeSuggestedPorts(existing, [8080]).nextText).toBeNull();
  });

  it("formats bounded provenance age without exposing another value", () => {
    expect(formatRuntimeFreshness(42_000)).toBe("42초 전");
    expect(formatRuntimeFreshness(180_000)).toBe("3분 전");
    expect(formatRuntimeFreshness(null)).toBe("시각 정보 없음");
  });
});
