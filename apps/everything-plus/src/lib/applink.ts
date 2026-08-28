import type { OpenRequest } from "../api";
import type { SearchFilter } from "../types";

const MAX_QUERY_BYTES = 512;

export type EverythingOpenAction =
  | { kind: "search"; query: string; filter?: SearchFilter }
  | { kind: "error"; message: string };

const MAX_EXTENSIONS = 64;
const MAX_EXTENSION_BYTES = 16;
const FILTER_KEYS = new Set([
  "extensions",
  "modifiedAfter",
  "modifiedBefore",
  "minSize",
  "maxSize",
  "sourceRootId",
  "contentStatus",
]);
const CONTENT_STATUSES = new Set([
  "indexed",
  "truncated",
  "partial",
  "failed",
  "not_indexed",
  "too_large",
  "unsupported_encoding",
  "read_error",
  "timeout",
  "changed_during_read",
  "skipped_sensitive",
  "no_text",
  "unsupported_encrypted",
  "extract_error",
]);

function normalizeFilter(value: SearchFilter | null | undefined): SearchFilter | undefined {
  if (!value || typeof value !== "object" || Array.isArray(value)) return undefined;
  if (Object.keys(value).some((key) => !FILTER_KEYS.has(key))) return undefined;
  // Missing is the only legacy-compatible empty value. An explicit null is
  // malformed for the array field (unlike nullable scalar options), so a
  // corrupt snapshot/request cannot silently drop the user's filter.
  const extensions = value.extensions === undefined ? [] : value.extensions;
  if (!Array.isArray(extensions) || extensions.length > MAX_EXTENSIONS) return undefined;
  const normalizedExtensions = [...new Set(extensions.map((extension) => {
    if (typeof extension !== "string") return "";
    return extension.trim().replace(/^\.+/, "").toLowerCase();
  }))];
  if (normalizedExtensions.some((extension) =>
    extension.length === 0 ||
    new TextEncoder().encode(extension).byteLength > MAX_EXTENSION_BYTES ||
    !/^[a-z0-9_+\-]+$/.test(extension),
  )) return undefined;

  const optionalNumber = (candidate: unknown): number | undefined => {
    if (candidate === undefined || candidate === null) return undefined;
    return typeof candidate === "number" && Number.isSafeInteger(candidate) && candidate >= 0
      ? candidate
      : Number.NaN;
  };
  const modifiedAfter = optionalNumber(value.modifiedAfter);
  const modifiedBefore = optionalNumber(value.modifiedBefore);
  const minSize = optionalNumber(value.minSize);
  const maxSize = optionalNumber(value.maxSize);
  if ([modifiedAfter, modifiedBefore, minSize, maxSize].some((candidate) => candidate !== undefined && Number.isNaN(candidate))) {
    return undefined;
  }
  if (modifiedAfter !== undefined && modifiedBefore !== undefined && modifiedAfter > modifiedBefore) return undefined;
  if (minSize !== undefined && maxSize !== undefined && minSize > maxSize) return undefined;
  const sourceRootId = optionalNumber(value.sourceRootId);
  if ((sourceRootId !== undefined && Number.isNaN(sourceRootId)) || (sourceRootId !== undefined && sourceRootId <= 0)) return undefined;
  if (
    value.contentStatus !== undefined &&
    value.contentStatus !== null &&
    typeof value.contentStatus !== "string"
  ) return undefined;
  const contentStatus = typeof value.contentStatus === "string"
    ? value.contentStatus.trim().toLowerCase()
    : undefined;
  if (contentStatus && !CONTENT_STATUSES.has(contentStatus)) return undefined;

  const filter: SearchFilter = {};
  if (normalizedExtensions.length) filter.extensions = normalizedExtensions.sort();
  if (modifiedAfter !== undefined) filter.modifiedAfter = modifiedAfter;
  if (modifiedBefore !== undefined) filter.modifiedBefore = modifiedBefore;
  if (minSize !== undefined) filter.minSize = minSize;
  if (maxSize !== undefined) filter.maxSize = maxSize;
  if (sourceRootId !== undefined) filter.sourceRootId = sourceRootId;
  if (contentStatus) filter.contentStatus = contentStatus;
  return Object.keys(filter).length ? filter : undefined;
}

export function routeOpenRequest(request: OpenRequest): EverythingOpenAction {
  if (request.target.kind !== "query") {
    return { kind: "error", message: "지원하지 않는 열기 요청입니다" };
  }

  const query = request.target.text.trim();
  if (
    query.length === 0 ||
    new TextEncoder().encode(query).byteLength > MAX_QUERY_BYTES ||
    Array.from(query).some((character) => {
      const code = character.codePointAt(0) ?? 0;
      return code < 0x20 || code === 0x7f;
    })
  ) {
    return { kind: "error", message: "요청한 검색어를 사용할 수 없습니다" };
  }
  if (request.target.filter !== undefined && request.target.filter !== null) {
    if (typeof request.target.filter !== "object" || Array.isArray(request.target.filter)) {
      return { kind: "error", message: "요청한 검색 필터를 사용할 수 없습니다" };
    }
    if (Object.keys(request.target.filter).some((key) => !FILTER_KEYS.has(key))) {
      return { kind: "error", message: "요청한 검색 필터를 사용할 수 없습니다" };
    }
    const filter = normalizeFilter(request.target.filter);
    const hasMeaningfulValue = Object.entries(request.target.filter).some(([key, candidate]) => {
      if (key === "extensions" && candidate === null) return true;
      if (candidate === null || candidate === undefined || candidate === "") return false;
      return !(Array.isArray(candidate) && candidate.length === 0);
    });
    if (!filter && hasMeaningfulValue) {
      return { kind: "error", message: "요청한 검색 필터를 사용할 수 없습니다" };
    }
    return filter ? { kind: "search", query, filter } : { kind: "search", query };
  }
  return { kind: "search", query };
}

export { normalizeFilter };
