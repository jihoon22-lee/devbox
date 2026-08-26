import { describe, expect, it } from "vitest";
import {
  normalizeQuickCapture,
  parseQuickCaptureTags,
  QUICK_CAPTURE_DEFAULT_TITLE,
} from "./quickCapture";

describe("quick capture input policy", () => {
  it("normalizes line endings, title whitespace, and duplicate tags", () => {
    expect(normalizeQuickCapture({
      title: "  Idea  ",
      body: "one\r\ntwo\rthree",
      tags: ["rust", " rust ", "offline"],
    })).toEqual({ title: "Idea", body: "one\ntwo\nthree", tags: ["rust", "offline"] });
  });

  it("uses the fixed title only when the title is blank", () => {
    expect(normalizeQuickCapture({ title: " ", body: "note", tags: [] }).title)
      .toBe(QUICK_CAPTURE_DEFAULT_TITLE);
    expect(() => normalizeQuickCapture({ title: "title", body: " \n\t", tags: [] }))
      .toThrow("빠른 캡처 본문을 입력하세요");
  });

  it("rejects line breaks in title and tags before the native call", () => {
    expect(() => normalizeQuickCapture({ title: "bad\nname", body: "note", tags: [] }))
      .toThrow("빠른 캡처 입력이 올바르지 않습니다");
    expect(() => normalizeQuickCapture({ title: "title", body: "note", tags: ["bad\n tag"] }))
      .toThrow("빠른 캡처 입력이 올바르지 않습니다");
    expect(() => normalizeQuickCapture({ title: "title", body: "bad\u0085text", tags: [] }))
      .toThrow("빠른 캡처 입력이 올바르지 않습니다");
  });

  it("rejects credential-like values without exposing their content", () => {
    expect(() => normalizeQuickCapture({
      title: "title",
      body: "Authorization: Bearer abcdefghijklmnop",
      tags: [],
    })).toThrow("민감한 정보가 포함되어 있어 저장하지 않았습니다");
    try {
      normalizeQuickCapture({ title: "title", body: "api_key=super-secret-value", tags: [] });
    } catch (error) {
      expect(error).toBeInstanceOf(Error);
      expect((error as Error).message).not.toContain("super-secret-value");
    }
  });

  it("parses tags only when the user submits the comma-separated field", () => {
    expect(parseQuickCaptureTags("rust, offline, rust")).toEqual(["rust", "offline", "rust"]);
  });
});
