import type { MultiplexerKind } from "../types";

export type CursorStyle = "block" | "underline" | "bar";
export type TerminalThemeName = "dark" | "light" | "highContrast";

export interface TerminalTheme {
  background: string;
  foreground: string;
  cursor: string;
  selectionBackground: string;
}

/** 사용자 문자열을 그대로 CSS에 넣지 않도록 글꼴은 고정 목록에서만 고른다. */
export interface FontChoice {
  id: string;
  label: string;
  family: string;
}

export const FONT_CHOICES: readonly FontChoice[] = [
  { id: "cascadia-code", label: "Cascadia Code", family: '"Cascadia Code", Consolas, monospace' },
  { id: "cascadia-mono", label: "Cascadia Mono", family: '"Cascadia Mono", Consolas, monospace' },
  { id: "consolas", label: "Consolas", family: "Consolas, monospace" },
  { id: "courier-new", label: "Courier New", family: '"Courier New", monospace' },
  { id: "system-mono", label: "시스템 고정폭", family: 'ui-monospace, "Segoe UI Mono", Consolas, monospace' },
];

export const TERMINAL_THEMES: Readonly<Record<TerminalThemeName, TerminalTheme>> = {
  dark: {
    background: "#111418",
    foreground: "#e6e9ef",
    cursor: "#4f8cff",
    selectionBackground: "#264f78",
  },
  light: {
    background: "#fbfcfe",
    foreground: "#1c2128",
    cursor: "#1f6feb",
    selectionBackground: "#bcd7ff",
  },
  highContrast: {
    background: "#000000",
    foreground: "#ffffff",
    cursor: "#ffff00",
    selectionBackground: "#0000c0",
  },
};

export const THEME_LABELS: Readonly<Record<TerminalThemeName, string>> = {
  dark: "어두움",
  light: "밝음",
  highContrast: "고대비",
};

export const CURSOR_LABELS: Readonly<Record<CursorStyle, string>> = {
  block: "블록",
  underline: "밑줄",
  bar: "세로 막대",
};

export const MIN_SCROLLBACK_LINES = 1_000;
export const MAX_SCROLLBACK_LINES = 100_000;
export const DEFAULT_SCROLLBACK_LINES = 10_000;

export interface TerminalSettings {
  /** 팬 하나를 닫을 때 확인할지. 탭·다중 팬 닫기 확인은 이 설정과 무관하게 항상 묻는다. */
  confirmSinglePaneClose: boolean;
  /** 복원할 레이아웃이 없을 때 기본 배포판 터미널을 하나 연다. */
  openTerminalOnStart: boolean;
  sidePanelOpen: boolean;
  multiplexer: MultiplexerKind;
  fontId: string;
  cursorStyle: CursorStyle;
  cursorBlink: boolean;
  scrollbackLines: number;
  theme: TerminalThemeName;
}

export const DEFAULT_SETTINGS: TerminalSettings = {
  confirmSinglePaneClose: true,
  openTerminalOnStart: true,
  sidePanelOpen: true,
  multiplexer: "native",
  fontId: FONT_CHOICES[0].id,
  cursorStyle: "block",
  cursorBlink: true,
  scrollbackLines: DEFAULT_SCROLLBACK_LINES,
  theme: "dark",
};

const SETTINGS_KEY = "wsl-desktop:settings";
const SETTINGS_VERSION = 1;

export function clampScrollbackLines(value: number): number {
  if (!Number.isFinite(value)) return DEFAULT_SCROLLBACK_LINES;
  return Math.min(MAX_SCROLLBACK_LINES, Math.max(MIN_SCROLLBACK_LINES, Math.round(value)));
}

export function fontFamilyFor(fontId: string): string {
  return (FONT_CHOICES.find((choice) => choice.id === fontId) ?? FONT_CHOICES[0]).family;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function boolean(value: unknown, fallback: boolean): boolean {
  return typeof value === "boolean" ? value : fallback;
}

/** 알 수 없는 값은 개별 필드 단위로 기본값으로 되돌린다 — 한 필드가 깨져도 나머지는 산다. */
export function normalizeSettings(value: unknown): TerminalSettings {
  if (!isRecord(value)) return { ...DEFAULT_SETTINGS };
  const multiplexer = value.multiplexer;
  const cursorStyle = value.cursorStyle;
  const theme = value.theme;
  const fontId = value.fontId;
  return {
    confirmSinglePaneClose: boolean(value.confirmSinglePaneClose, DEFAULT_SETTINGS.confirmSinglePaneClose),
    openTerminalOnStart: boolean(value.openTerminalOnStart, DEFAULT_SETTINGS.openTerminalOnStart),
    sidePanelOpen: boolean(value.sidePanelOpen, DEFAULT_SETTINGS.sidePanelOpen),
    multiplexer: multiplexer === "tmux" || multiplexer === "zellij" || multiplexer === "native"
      ? multiplexer
      : DEFAULT_SETTINGS.multiplexer,
    fontId: typeof fontId === "string" && FONT_CHOICES.some((choice) => choice.id === fontId)
      ? fontId
      : DEFAULT_SETTINGS.fontId,
    cursorStyle: cursorStyle === "block" || cursorStyle === "underline" || cursorStyle === "bar"
      ? cursorStyle
      : DEFAULT_SETTINGS.cursorStyle,
    cursorBlink: boolean(value.cursorBlink, DEFAULT_SETTINGS.cursorBlink),
    scrollbackLines: typeof value.scrollbackLines === "number"
      ? clampScrollbackLines(value.scrollbackLines)
      : DEFAULT_SETTINGS.scrollbackLines,
    theme: theme === "dark" || theme === "light" || theme === "highContrast"
      ? theme
      : DEFAULT_SETTINGS.theme,
  };
}

export function loadSettings(): TerminalSettings {
  try {
    const raw: unknown = JSON.parse(localStorage.getItem(SETTINGS_KEY) ?? "null");
    if (!isRecord(raw) || raw.version !== SETTINGS_VERSION) return { ...DEFAULT_SETTINGS };
    return normalizeSettings(raw);
  } catch {
    return { ...DEFAULT_SETTINGS };
  }
}

export function saveSettings(settings: TerminalSettings): void {
  try {
    localStorage.setItem(SETTINGS_KEY, JSON.stringify({ version: SETTINGS_VERSION, ...settings }));
  } catch {
    /* 저장 실패는 현재 창의 동작을 막지 않는다 */
  }
}
