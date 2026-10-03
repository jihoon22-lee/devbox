import { normalizeFilter } from "./lib/applink";
import type { SearchFilter, RootStatus } from "./types";

export const MAX_SEARCH_QUERY_BYTES = 4 * 1024;
export const MAX_SAVED_NAME_BYTES = 128;
export const MAX_SAVED_QUERY_BYTES = 512;
export const EMPTY_FILTER: SearchFilter = {};

function utf8ByteLength(value: string): number {
  return new TextEncoder().encode(value).byteLength;
}

export function isSearchQueryAllowed(value: string): boolean {
  return (
    utf8ByteLength(value) <= MAX_SEARCH_QUERY_BYTES &&
    !Array.from(value).some((character) => {
      const code = character.codePointAt(0) ?? 0;
      return code < 0x20 || code === 0x7f;
    })
  );
}

export function isSavedDefinitionAllowed(name: string, value: string): boolean {
  return utf8ByteLength(name.trim()) <= MAX_SAVED_NAME_BYTES && utf8ByteLength(value.trim()) <= MAX_SAVED_QUERY_BYTES;
}

export function isFilterEmpty(filter: SearchFilter): boolean {
  return (
    !filter.extensions?.length &&
    filter.modifiedAfter == null &&
    filter.modifiedBefore == null &&
    filter.minSize == null &&
    filter.maxSize == null &&
    filter.sourceRootId == null &&
    !filter.contentStatus
  );
}

export function normalizeUiFilter(filter: SearchFilter): SearchFilter | null {
  const normalized = normalizeFilter(filter);
  if (normalized) return normalized;
  return isFilterEmpty(filter) ? EMPTY_FILTER : null;
}

export function filterCount(filter: SearchFilter): number {
  return [
    Boolean(filter.extensions?.length),
    filter.modifiedAfter != null,
    filter.modifiedBefore != null,
    filter.minSize != null,
    filter.maxSize != null,
    filter.sourceRootId != null,
    Boolean(filter.contentStatus),
  ].filter(Boolean).length;
}

export function parseExtensions(value: string): string[] {
  return [
    ...new Set(
      value
        .split(",")
        .map((extension) => extension.trim().replace(/^\.+/, "").toLowerCase())
        .filter(Boolean),
    ),
  ];
}

export function optionalSize(value: string): number | null | undefined {
  if (!value.trim()) return undefined;
  const parsed = Number(value);
  return Number.isSafeInteger(parsed) && parsed >= 0 ? parsed : null;
}

export function dateInputValue(timestamp: number | undefined): string {
  if (timestamp === undefined) return "";
  const date = new Date(timestamp);
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

export function contentStatusLabel(status: string | null | undefined, truncated = false): string {
  if (truncated || status === "truncated" || status === "partial") return "일부/잘림";
  if (!status) return "색인되지 않음";
  return status === "indexed" ? "색인됨" : status.replace(/_/g, " ");
}

export function fmtSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

export function watcherLabel(status: RootStatus): string {
  if (status.error === "watcher_state_poisoned") return "색인 중단";
  if (status.error === "root_unavailable") return "연결 끊김";
  if (status.error === "root_scan_limit") return "범위 상한";
  if (status.error === "root_scan_incomplete") return "부분 스캔";
  if (status.error) return "확인 필요";
  if (status.pending > 0) return `${status.pending}개 반영 대기`;
  return status.watchMode === "polling" ? "WSL 주기 확인" : "실시간";
}

export function watcherTitle(status: RootStatus): string {
  if (status.error === "watcher_state_poisoned")
    return "색인 상태 오류로 자동 갱신을 중단했습니다. 앱을 다시 시작하세요.";
  if (status.error === "root_unavailable") {
    return status.sourceKind === "wsl"
      ? "WSL 배포판 또는 검색 루트에 연결할 수 없어 기존 인덱스를 보존했습니다. 연결되면 자동으로 다시 확인합니다."
      : "검색 루트에 연결할 수 없어 기존 인덱스를 보존했습니다.";
  }
  if (status.error === "root_scan_limit") {
    return "파일 수 상한을 넘어 기존 인덱스를 보존했습니다. 검색 루트를 더 작게 나누세요.";
  }
  if (status.error === "root_scan_incomplete") {
    return "읽을 수 없는 하위 경로가 있어 삭제를 추정하지 않고 기존 인덱스를 보존했습니다.";
  }
  if (status.error) return "증분 인덱스를 확인해야 합니다.";
  return status.watchMode === "polling"
    ? "WSL UNC 루트는 Linux 경로 대소문자를 보존하며 제한된 메타데이터 폴링으로 반영합니다."
    : "네이티브 파일 시스템 감시";
}
