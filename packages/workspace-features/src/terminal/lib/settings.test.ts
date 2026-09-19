import { beforeEach, describe, expect, it } from "vitest";
import {
  DEFAULT_SETTINGS,
  MAX_SCROLLBACK_LINES,
  MIN_SCROLLBACK_LINES,
  clampScrollbackLines,
  fontFamilyFor,
  loadSettings,
  normalizeSettings,
  saveSettings,
} from "./settings";

beforeEach(() => localStorage.clear());

describe("terminal settings", () => {
  it("저장하고 다시 읽으면 같은 값을 돌려준다", () => {
    saveSettings({
      ...DEFAULT_SETTINGS,
      confirmSinglePaneClose: false,
      sidePanelOpen: false,
      multiplexer: "tmux",
      theme: "light",
      scrollbackLines: 20_000,
    });
    expect(loadSettings()).toEqual({
      ...DEFAULT_SETTINGS,
      confirmSinglePaneClose: false,
      sidePanelOpen: false,
      multiplexer: "tmux",
      theme: "light",
      scrollbackLines: 20_000,
    });
    expect(JSON.parse(localStorage.getItem("wsl-desktop:settings") ?? "null").version).toBe(2);
  });

  it("설정이 없으면 기본값을 쓴다", () => {
    expect(loadSettings()).toEqual(DEFAULT_SETTINGS);
  });

  it("손상된 값이나 다른 version은 기본값으로 되돌린다", () => {
    localStorage.setItem("wsl-desktop:settings", "{not json");
    expect(loadSettings()).toEqual(DEFAULT_SETTINGS);
    localStorage.setItem("wsl-desktop:settings", JSON.stringify({ version: 99, theme: "light" }));
    expect(loadSettings()).toEqual(DEFAULT_SETTINGS);
  });

  it("version 1 설정은 보존하면서 빠른 호출 기본값을 채워 version 2로 이관한다", () => {
    localStorage.setItem("wsl-desktop:settings", JSON.stringify({
      version: 1,
      theme: "light",
      sidePanelOpen: false,
    }));

    expect(loadSettings()).toMatchObject({
      theme: "light",
      sidePanelOpen: false,
      quickSummonEnabled: true,
      quickSummonShortcut: "Ctrl+Alt+Space",
      keepInTray: false,
    });
  });

  it("알 수 없는 필드만 기본값으로 되돌리고 나머지는 보존한다", () => {
    const normalized = normalizeSettings({
      confirmSinglePaneClose: false,
      multiplexer: "screen",
      theme: "solarized",
      cursorStyle: "beam",
      fontId: "../../evil",
      scrollbackLines: "many",
      quickSummonShortcut: "Alt+F4",
      keepInTray: "yes",
    });
    expect(normalized.confirmSinglePaneClose).toBe(false);
    expect(normalized.multiplexer).toBe(DEFAULT_SETTINGS.multiplexer);
    expect(normalized.theme).toBe(DEFAULT_SETTINGS.theme);
    expect(normalized.cursorStyle).toBe(DEFAULT_SETTINGS.cursorStyle);
    expect(normalized.fontId).toBe(DEFAULT_SETTINGS.fontId);
    expect(normalized.scrollbackLines).toBe(DEFAULT_SETTINGS.scrollbackLines);
    expect(normalized.quickSummonShortcut).toBe(DEFAULT_SETTINGS.quickSummonShortcut);
    expect(normalized.keepInTray).toBe(DEFAULT_SETTINGS.keepInTray);
  });

  it("scrollback은 상·하한으로 clamp한다", () => {
    expect(clampScrollbackLines(10)).toBe(MIN_SCROLLBACK_LINES);
    expect(clampScrollbackLines(10_000_000)).toBe(MAX_SCROLLBACK_LINES);
    expect(clampScrollbackLines(Number.NaN)).toBe(DEFAULT_SETTINGS.scrollbackLines);
    expect(clampScrollbackLines(12_345.6)).toBe(12_346);
  });

  it("글꼴은 고정 목록에서만 해석하고 알 수 없는 id는 기본 글꼴로 떨어진다", () => {
    expect(fontFamilyFor("consolas")).toBe("Consolas, monospace");
    expect(fontFamilyFor("'; content: url(evil)")).toBe(fontFamilyFor(DEFAULT_SETTINGS.fontId));
  });
});
