// App.tsx에 인라인돼 있던 저장된 프로젝트 경로 목록 로직을 테스트를 위해 순수 함수로 추출했다.
// wsl-desktop/src/lib/storage.ts의 최근 경로 저장 선례를 따른다. 동작은 바꾸지 않았다.
const LS_KEY = "wsld-projects";

export function loadSavedPaths(): string[] {
  try {
    return JSON.parse(localStorage.getItem(LS_KEY) ?? "[]");
  } catch {
    return [];
  }
}

export function savePaths(paths: string[]): void {
  localStorage.setItem(LS_KEY, JSON.stringify(paths));
}

/** trim한 경로가 이미 목록에 있으면 그대로, 없으면 끝에 추가한다. 빈 문자열은 무시. */
export function addPath(paths: string[], path: string): string[] {
  const p = path.trim();
  if (!p) return paths;
  return paths.includes(p) ? paths : [...paths, p];
}

export function removePath(paths: string[], path: string): string[] {
  return paths.filter((x) => x !== path);
}
