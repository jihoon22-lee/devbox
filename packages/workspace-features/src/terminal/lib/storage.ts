import { readTerminalPreference, writeTerminalPreference } from "./storageNamespace";
import { DEFAULT_TERMINAL_FONT_SIZE, clampTerminalFontSize } from "./terminalUx";

// wsl-dashboard의 localStorage 프로젝트 경로 저장 선례(apps/wsl-dashboard/src/App.tsx)를
// 따르되, 별도 앱이므로 이름 충돌을 피하려고 접두사를 둔다.
const PINNED_KEY = "wsl-desktop:cwd-pinned";
const CWD_KEY = "wsl-desktop:cwd-value";
const RECENT_KEY = "wsl-desktop:recent-paths";
const COPY_ON_SELECT_KEY = "wsl-desktop:copy-on-select";
const FONT_SIZE_KEY = "wsl-desktop:font-size";
const MAX_RECENT = 12;

export function loadPinned(): boolean {
  return readTerminalPreference(PINNED_KEY) === "1";
}

export function savePinned(pinned: boolean): void {
  writeTerminalPreference(PINNED_KEY, pinned ? "1" : "0");
}

export function loadPinnedCwd(): string {
  return readTerminalPreference(CWD_KEY) ?? "";
}

export function savePinnedCwd(cwd: string): void {
  writeTerminalPreference(CWD_KEY, cwd);
}

export function loadRecentPaths(): string[] {
  try {
    const raw: unknown = JSON.parse(readTerminalPreference(RECENT_KEY) ?? "[]");
    return Array.isArray(raw) ? raw.filter((p): p is string => typeof p === "string") : [];
  } catch {
    return [];
  }
}

/** MRU(최근 사용 순) 목록 갱신 + 저장. 최대 12개, 중복 제거. */
export function pushRecentPath(path: string): string[] {
  const trimmed = path.trim();
  if (!trimmed) return loadRecentPaths();
  const next = [trimmed, ...loadRecentPaths().filter((p) => p !== trimmed)].slice(0, MAX_RECENT);
  writeTerminalPreference(RECENT_KEY, JSON.stringify(next));
  return next;
}

/** 설정이 없을 때는 Windows Terminal과 유사하게 selection 자동 복사를 켠다. */
export function loadCopyOnSelect(): boolean {
  return readTerminalPreference(COPY_ON_SELECT_KEY) !== "0";
}

export function saveCopyOnSelect(enabled: boolean): void {
  writeTerminalPreference(COPY_ON_SELECT_KEY, enabled ? "1" : "0");
}

export function loadTerminalFontSize(): number {
  const raw = readTerminalPreference(FONT_SIZE_KEY);
  if (raw === null || raw.trim() === "") return DEFAULT_TERMINAL_FONT_SIZE;
  return clampTerminalFontSize(Number(raw));
}

export function saveTerminalFontSize(fontSize: number): void {
  writeTerminalPreference(FONT_SIZE_KEY, String(clampTerminalFontSize(fontSize)));
}
