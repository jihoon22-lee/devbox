import { describe, expect, it } from "vitest";
import {
  isSafeQuickCapturePath,
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
    expect(() => normalizeQuickCapture({ title: "title\n", body: "note", tags: [] }))
      .toThrow("빠른 캡처 입력이 올바르지 않습니다");
    expect(() => normalizeQuickCapture({ title: "title", body: "note", tags: ["bad\n tag"] }))
      .toThrow("태그에 줄바꿈·쉼표·대괄호·따옴표를 사용할 수 없습니다");
    expect(() => normalizeQuickCapture({ title: "title", body: "bad\u0085text", tags: [] }))
      .toThrow("빠른 캡처 입력이 올바르지 않습니다");
    expect(() => normalizeQuickCapture({ title: "title\u2028name", body: "note", tags: [] }))
      .toThrow("빠른 캡처 입력이 올바르지 않습니다");
    expect(() => normalizeQuickCapture({ title: "title", body: "bad\ud800text", tags: [] }))
      .toThrow("빠른 캡처 입력이 올바르지 않습니다");
    expect(() => normalizeQuickCapture({ title: "title", body: "note", tags: ["safe\n"] }))
      .toThrow("태그에 줄바꿈·쉼표·대괄호·따옴표를 사용할 수 없습니다");
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

  it("keeps Rust and browser byte/scalar bounds aligned", () => {
    expect(normalizeQuickCapture({
      title: "😀".repeat(200),
      body: "note",
      tags: [],
    }).title).toHaveLength(400);
    expect(() => normalizeQuickCapture({
      title: "😀".repeat(201),
      body: "note",
      tags: [],
    })).toThrow("제목은 UTF-8 800바이트·200자 이내로 입력하세요");
    expect(normalizeQuickCapture({
      title: "title",
      body: "😀".repeat(16_384),
      tags: [],
    }).body).toHaveLength(32_768);
    expect(() => normalizeQuickCapture({
      title: "title",
      body: "😀".repeat(32_769),
      tags: [],
    })).toThrow("본문은 LF 기준 64 KiB(원문 128 KiB) 이내로 입력하세요");
    const rawAtLimit = `${"\r\n".repeat(65_535)}x`;
    expect(normalizeQuickCapture({ title: "title", body: rawAtLimit, tags: [] }).body)
      .toHaveLength(65_536);
    const rawOverLimit = `${"\r\n".repeat(65_536)}x`;
    expect(() => normalizeQuickCapture({ title: "title", body: rawOverLimit, tags: [] }))
      .toThrow("본문은 LF 기준 64 KiB(원문 128 KiB) 이내로 입력하세요");
    expect(normalizeQuickCapture({ title: "title", body: "note", tags: ["😀".repeat(48)] }).tags)
      .toHaveLength(1);
    expect(() => normalizeQuickCapture({
      title: "title",
      body: "note",
      tags: ["😀".repeat(49)],
    })).toThrow("태그 하나는 UTF-8 192바이트·48자 이내로 입력하세요");
  });

  it("scans all credential prefix occurrences and blocks header-shaped secrets", () => {
    for (const body of [
      "X-API-Key: hidden-value",
      '{"api_key": "hidden-value"}',
      "ghp_short ghp_abcdefghijklmnop",
      "sk-abcdefghijklmnop",
      "Bearer 😀😀😀",
      "Authorization: Basic abcdefghijklmnop",
      "token: abcdefghijklmnop",
      "Bearer `abcdefghijklmnop`",
      "SECRET_KEY=abcdefghijklmnop",
      "Cookie: session=abcdefghijklmnop",
      "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.signaturevalue",
      "hf_abcdefghijklmnop",
      "ya29.abcdefghijklmnop",
      "whsec_abcdefghijklmnop",
      "AWS_SECRET_ACCESS_KEY=abcdefghijklmnop",
      "ACCOUNT_KEY=abcdefghijklmnop",
      "AZURE_CLIENT_SECRET=abcdefghijklmnop",
      "CLOUDFLARE_API_TOKEN=abcdefghijklmnop",
      "private_token: abcdefghijklmnop",
      "123456789:abcdefghijklmnopqrstuvwxyzABCDE",
      "hvs.abcdefghijklmnop",
    ]) {
      expect(() => normalizeQuickCapture({ title: "title", body, tags: [] }))
        .toThrow("민감한 정보가 포함되어 있어 저장하지 않았습니다");
    }
  });

  it("accepts only the native fixed Inbox filename contract", () => {
    expect(isSafeQuickCapturePath("Inbox/quick-capture-2026-08-27-12-30-00.md")).toBe(true);
    expect(isSafeQuickCapturePath("Inbox/quick-capture-2026-08-27-12-30-00-100.md")).toBe(true);
    for (const path of [
      "Inbox/other.md",
      "Inbox/quick-capture-2026-08-27-12-30-00-101.md",
      "Inbox/quick-capture-2026-08-27-12-30-00/child.md",
      "Inbox/../outside.md",
      "C:\\Knowledge\\Inbox\\quick-capture-2026-08-27-12-30-00.md",
      "Inbox/quick-capture-2026-13-27-12-30-00.md",
      "Inbox/quick-capture-2026-08-27-24-30-00.md",
    ]) {
      expect(isSafeQuickCapturePath(path)).toBe(false);
    }
  });

  it("parses tags only when the user submits the comma-separated field", () => {
    expect(parseQuickCaptureTags("rust, offline, rust")).toEqual(["rust", "offline", "rust"]);
  });

  it("caps direct tag parsing before allocating an untrusted tag list", () => {
    const value = `${"x".repeat(100_000)},${"y".repeat(100_000)}`;
    const parsed = parseQuickCaptureTags(value);
    expect(parsed).toHaveLength(2);
    expect(parsed.every((tag) => tag.length === 385)).toBe(true);
  });
});
