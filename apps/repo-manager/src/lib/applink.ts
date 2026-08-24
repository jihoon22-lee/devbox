import type { OpenRequest } from "../api";

const MAX_PATH_CHARS = 32_767;

export type RepoOpenAction =
  | { kind: "prepareRepository"; path: string }
  | { kind: "error"; message: string };

export function routeOpenRequest(request: OpenRequest): RepoOpenAction {
  if (request.target.kind !== "path") {
    return { kind: "error", message: "지원하지 않는 열기 요청입니다" };
  }

  const path = request.target.path;
  if (path.length === 0 || path.length > MAX_PATH_CHARS || path.includes("\0")) {
    return { kind: "error", message: "요청한 repository 경로를 사용할 수 없습니다" };
  }
  return { kind: "prepareRepository", path };
}

export function sameRepositoryKey(left: string, right: string): boolean {
  if (left.startsWith("win:") && right.startsWith("win:")) {
    return left.toLowerCase() === right.toLowerCase();
  }
  return left === right;
}
