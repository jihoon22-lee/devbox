export const DEFAULT_TERMINAL_FONT_SIZE = 13;
export const MIN_TERMINAL_FONT_SIZE = 9;
export const MAX_TERMINAL_FONT_SIZE = 24;
export const MAX_TERMINAL_PASTE_CHARACTERS = 1_000_000;
export const MAX_TERMINAL_SEARCH_CHARACTERS = 512;

const MAX_CWD_LENGTH = 4096;
const MAX_TITLE_LENGTH = 120;
const MAX_LINK_LENGTH = 2048;
const CONTROL_CHARACTERS = /[\u0000-\u001f\u007f-\u009f]/u;
const CONTROL_CHARACTER_RUN = /[\u0000-\u001f\u007f-\u009f]+/gu;

export type TerminalKeyAction =
  | "copy"
  | "paste"
  | "search"
  | "font-increase"
  | "font-decrease"
  | "font-reset";

type KeyboardLike = Pick<
  KeyboardEvent,
  "altKey" | "code" | "ctrlKey" | "key" | "metaKey" | "shiftKey"
>;

/** 셸에 전달하면 안 되는 터미널 로컬 단축키만 분류한다. bare Ctrl+C는 SIGINT로 남긴다. */
export function matchTerminalKey(event: KeyboardLike): TerminalKeyAction | null {
  if (!event.ctrlKey || event.altKey || event.metaKey) return null;
  const key = event.key.toLowerCase();

  if (event.shiftKey) {
    if (key === "c") return "copy";
    if (key === "v") return "paste";
    if (key === "f") return "search";
  }

  if (event.code === "Equal" || event.code === "NumpadAdd" || key === "+") {
    return "font-increase";
  }
  if (event.code === "Minus" || event.code === "NumpadSubtract" || key === "-") {
    return "font-decrease";
  }
  if (event.code === "Digit0" || event.code === "Numpad0" || key === "0") {
    return "font-reset";
  }
  return null;
}

export function clampTerminalFontSize(value: number): number {
  if (!Number.isFinite(value)) return DEFAULT_TERMINAL_FONT_SIZE;
  return Math.min(MAX_TERMINAL_FONT_SIZE, Math.max(MIN_TERMINAL_FONT_SIZE, Math.round(value)));
}

/** OSC 7 payload에서 현재 WSL 경로만 꺼낸다. 잘못된/원격이 아닌 형식은 상태를 바꾸지 않는다. */
export function parseOsc7Cwd(payload: string): string | null {
  if (!payload || payload.length > MAX_CWD_LENGTH || CONTROL_CHARACTERS.test(payload)) return null;
  try {
    const url = new URL(payload);
    if (url.protocol !== "file:" || url.username || url.password || url.port || url.search || url.hash) {
      return null;
    }
    const path = decodeURIComponent(url.pathname);
    if (!path.startsWith("/") || path.length > MAX_CWD_LENGTH || CONTROL_CHARACTERS.test(path)) {
      return null;
    }
    return path;
  } catch {
    return null;
  }
}

export function normalizeTerminalTitle(input: string): string | null {
  const title = input
    .replace(CONTROL_CHARACTER_RUN, " ")
    .replace(/\s+/gu, " ")
    .trim()
    .slice(0, MAX_TITLE_LENGTH);
  return title || null;
}

/** 터미널 출력이 실행 가능한 스킴이나 자격 증명이 든 URL을 열지 못하게 한다. */
export function normalizeTerminalLink(input: string): string | null {
  const value = input.trim();
  if (!value || value.length > MAX_LINK_LENGTH || CONTROL_CHARACTERS.test(value)) return null;
  try {
    const url = new URL(value);
    if ((url.protocol !== "http:" && url.protocol !== "https:") || url.username || url.password) {
      return null;
    }
    return url.href;
  } catch {
    return null;
  }
}

export function pasteLineCount(text: string): number {
  if (!text) return 0;
  return text.replace(/\r\n?/gu, "\n").split("\n").length;
}

export function hasMultilinePaste(text: string): boolean {
  return /[\r\n]/u.test(text);
}
