import type { OpenRequest } from "../api";

const MAX_QUERY_CHARS = 512;
const MAX_PATH_CHARS = 32_768;

export type KnowledgeOpenAction =
  | { kind: "openNote"; path: string }
  | { kind: "search"; query: string }
  | { kind: "draft"; id: string }
  | { kind: "error"; message: string };

/**
 * Pure Knowledge-specific routing. Filesystem authority remains in the Rust
 * `open_inbound_note` command; this layer only bounds input and connects Query
 * to the existing search UI.
 */
export function routeOpenRequest(request: OpenRequest): KnowledgeOpenAction {
  const target = request.target;
  switch (target.kind) {
    case "path":
      return target.path.length > 0 && target.path.length <= MAX_PATH_CHARS && !target.path.includes("\0")
        ? { kind: "openNote", path: target.path }
        : { kind: "error", message: "요청한 노트를 열 수 없습니다" };
    case "query": {
      const query = target.text.trim();
      return query.length > 0 && query.length <= MAX_QUERY_CHARS && !query.includes("\0")
        ? { kind: "search", query }
        : { kind: "error", message: "요청한 검색어를 사용할 수 없습니다" };
    }
    case "profile":
    case "workspace":
      return { kind: "error", message: "지원하지 않는 열기 요청입니다" };
    case "handoff":
      return target.handoffKind === "knowledge-draft/v1"
          && /^[0-9a-f]{32}$/u.test(target.id)
        ? { kind: "draft", id: target.id }
        : { kind: "error", message: "지원하지 않는 handoff 요청입니다" };
  }
}
