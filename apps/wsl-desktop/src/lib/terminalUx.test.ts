import { describe, expect, it } from "vitest";
import {
  DEFAULT_TERMINAL_FONT_SIZE,
  MAX_TERMINAL_FONT_SIZE,
  MIN_TERMINAL_FONT_SIZE,
  clampTerminalFontSize,
  hasMultilinePaste,
  matchTerminalKey,
  normalizeTerminalLink,
  normalizeTerminalTitle,
  parseOsc7Cwd,
  pasteLineCount,
} from "./terminalUx";

function key(overrides: Partial<KeyboardEvent>): KeyboardEvent {
  return {
    altKey: false,
    code: "",
    ctrlKey: true,
    key: "",
    metaKey: false,
    shiftKey: false,
    ...overrides,
  } as KeyboardEvent;
}

describe("terminal UX 입력 경계", () => {
  it("Ctrl+Shift+C/V/F와 글꼴 키만 가로채고 bare Ctrl+C는 셸에 남긴다", () => {
    expect(matchTerminalKey(key({ key: "C", code: "KeyC", shiftKey: true }))).toBe("copy");
    expect(matchTerminalKey(key({ key: "V", code: "KeyV", shiftKey: true }))).toBe("paste");
    expect(matchTerminalKey(key({ key: "F", code: "KeyF", shiftKey: true }))).toBe("search");
    expect(matchTerminalKey(key({ key: "+", code: "Equal", shiftKey: true }))).toBe("font-increase");
    expect(matchTerminalKey(key({ key: "-", code: "Minus" }))).toBe("font-decrease");
    expect(matchTerminalKey(key({ key: "0", code: "Digit0" }))).toBe("font-reset");
    expect(matchTerminalKey(key({ key: "c", code: "KeyC" }))).toBeNull();
    expect(matchTerminalKey(key({ key: "v", code: "KeyV", altKey: true }))).toBeNull();
  });

  it("글꼴 크기를 정수 범위로 제한한다", () => {
    expect(clampTerminalFontSize(2)).toBe(MIN_TERMINAL_FONT_SIZE);
    expect(clampTerminalFontSize(99)).toBe(MAX_TERMINAL_FONT_SIZE);
    expect(clampTerminalFontSize(13.6)).toBe(14);
    expect(clampTerminalFontSize(Number.NaN)).toBe(DEFAULT_TERMINAL_FONT_SIZE);
  });

  it("OSC 7 file URL에서만 절대 cwd를 디코딩한다", () => {
    expect(parseOsc7Cwd("file://wsl-host/mnt/c/My%20Repo")).toBe("/mnt/c/My Repo");
    expect(parseOsc7Cwd("file:///home/me/project")).toBe("/home/me/project");
    expect(parseOsc7Cwd("https://example.com/home/me")).toBeNull();
    expect(parseOsc7Cwd("file://user:pass@host/home/me")).toBeNull();
    expect(parseOsc7Cwd("file://host/home/me?token=secret")).toBeNull();
    expect(parseOsc7Cwd("file://host/%00secret")).toBeNull();
  });

  it("OSC 제목을 한 줄로 제한하고 제어 문자를 제거한다", () => {
    expect(normalizeTerminalTitle("  build\u0000\r\nlogs  ")).toBe("build logs");
    expect(normalizeTerminalTitle(" \u0000 ")).toBeNull();
    expect(normalizeTerminalTitle("x".repeat(121))).toHaveLength(120);
  });

  it("자격 증명 없는 HTTP(S) 링크만 정규화한다", () => {
    expect(normalizeTerminalLink(" https://example.com/a?q=1 ")).toBe("https://example.com/a?q=1");
    expect(normalizeTerminalLink("http://example.com")).toBe("http://example.com/");
    expect(normalizeTerminalLink("javascript:alert(1)")).toBeNull();
    expect(normalizeTerminalLink("https://user:secret@example.com/")).toBeNull();
    expect(normalizeTerminalLink("https://example.com/\u0000secret")).toBeNull();
  });

  it("붙여넣기 줄 수를 CRLF와 LF 모두 일관되게 계산한다", () => {
    expect(hasMultilinePaste("echo one")).toBe(false);
    expect(hasMultilinePaste("echo one\r\necho two")).toBe(true);
    expect(pasteLineCount("echo one\r\necho two\n")).toBe(3);
    expect(pasteLineCount("")).toBe(0);
  });
});
