import type { OpenRequest } from "../api";

const MAX_QUERY_CHARS = 512;

export type EverythingOpenAction =
  | { kind: "search"; query: string }
  | { kind: "error"; message: string };

export function routeOpenRequest(request: OpenRequest): EverythingOpenAction {
  if (request.target.kind !== "query") {
    return { kind: "error", message: "지원하지 않는 열기 요청입니다" };
  }

  const query = request.target.text.trim();
  if (query.length === 0 || query.length > MAX_QUERY_CHARS || query.includes("\0")) {
    return { kind: "error", message: "요청한 검색어를 사용할 수 없습니다" };
  }
  return { kind: "search", query };
}
