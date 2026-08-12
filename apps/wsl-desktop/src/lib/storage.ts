// wsl-dashboard의 localStorage 프로젝트 경로 저장 선례(apps/wsl-dashboard/src/App.tsx)를
// 따르되, 별도 앱이므로 이름 충돌을 피하려고 접두사를 둔다.
const PINNED_KEY = "wsl-desktop:cwd-pinned";
const CWD_KEY = "wsl-desktop:cwd-value";
const RECENT_KEY = "wsl-desktop:recent-paths";
const MAX_RECENT = 5;

export function loadPinned(): boolean {
  return localStorage.getItem(PINNED_KEY) === "1";
}

export function savePinned(pinned: boolean): void {
  localStorage.setItem(PINNED_KEY, pinned ? "1" : "0");
}

export function loadPinnedCwd(): string {
  return localStorage.getItem(CWD_KEY) ?? "";
}

export function savePinnedCwd(cwd: string): void {
  localStorage.setItem(CWD_KEY, cwd);
}

export function loadRecentPaths(): string[] {
  try {
    const raw: unknown = JSON.parse(localStorage.getItem(RECENT_KEY) ?? "[]");
    return Array.isArray(raw) ? raw.filter((p): p is string => typeof p === "string") : [];
  } catch {
    return [];
  }
}

/** MRU(최근 사용 순) 목록 갱신 + 저장. 최대 5개, 중복 제거. */
export function pushRecentPath(path: string): string[] {
  const trimmed = path.trim();
  if (!trimmed) return loadRecentPaths();
  const next = [trimmed, ...loadRecentPaths().filter((p) => p !== trimmed)].slice(0, MAX_RECENT);
  localStorage.setItem(RECENT_KEY, JSON.stringify(next));
  return next;
}
