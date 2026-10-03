import type { KnowledgeWatcherStatus } from "./types";

export function indent(path: string): number {
  return path.split("/").length - 1;
}

export function isMarkdown(path: string | null): boolean {
  return !!path && path.endsWith(".md");
}

export function normalizeRelativePath(path: string): string {
  return path.trim().replace(/\\/g, "/").replace(/\/+/g, "/");
}

export function parentPath(path: string): string {
  const separator = path.lastIndexOf("/");
  return separator < 0 ? "" : path.slice(0, separator);
}

export function childPath(parent: string, name: string): string {
  return parent ? `${parent}/${name}` : name;
}

export function isSameOrChild(path: string | null, parent: string): boolean {
  return path === parent || path?.startsWith(`${parent}/`) === true;
}

export function utf8Bytes(value: string): number {
  return new TextEncoder().encode(value).byteLength;
}

export function draftNeedsRegeneration(cause: unknown): boolean {
  return cause instanceof Error && cause.name === "draft_stale";
}

export function remapPath(path: string | null, from: string, to: string): string | null {
  if (path === from) return to;
  if (path?.startsWith(`${from}/`)) return `${to}${path.slice(from.length)}`;
  return path;
}

export function watcherStatusLabel(status: KnowledgeWatcherStatus): string {
  const source = status.sourceKind === "wsl" ? "WSL 저장소 · 5초 폴링" : "Windows 저장소 · 실시간 감시";
  const error =
    status.error === "watcher_state_poisoned"
      ? "색인 중단 · 앱을 다시 시작하세요"
      : status.error === "vault_unconfigured"
        ? "저장소 미설정"
        : status.error === "vault_unavailable"
          ? "저장소 연결 끊김 · 마지막 색인 유지"
          : status.error === "vault_scan_limit"
            ? "안전 색인 한도 초과 · 마지막 색인 유지"
            : status.error === "vault_scan_incomplete"
              ? "일부 파일 읽기 실패 · 마지막 색인 유지"
              : status.error === "vault_index_failed"
                ? "색인 갱신 실패 · 마지막 색인 유지"
                : null;
  return error ? `${source} · ${error}` : source;
}
