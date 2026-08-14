// TODO(workbench): 프로젝트 목록과 git 상태는 Workbench의 ProjectProfile로 이관한다.
// (docs/product-opportunities.md §3.1, §15.2). Workbench 출시 전까지 여기서 유지한다.
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
