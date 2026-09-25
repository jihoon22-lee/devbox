import type { FileEntry, ContentResult } from "./types";

/** Search sources have different metadata; normalize display fields, keeping authority in the row reference. */
export function sourceRowValue(
  value: import("../generated/serde_json/JsonValue").JsonValue,
): FileEntry & ContentResult {
  if (
    !value ||
    typeof value !== "object" ||
    Array.isArray(value) ||
    typeof value.path !== "string" ||
    typeof value.name !== "string"
  )
    throw new Error("검색 결과 형식을 확인하지 못했습니다.");
  const text = (key: string, fallback = "") => (typeof value[key] === "string" ? value[key] : fallback);
  const number = (key: string) => (typeof value[key] === "number" && Number.isFinite(value[key]) ? value[key] : 0);
  const nullableNumber = (key: string) =>
    typeof value[key] === "number" && Number.isFinite(value[key]) ? value[key] : null;
  return {
    id: number("id"),
    path: value.path,
    name: value.name,
    ext: text("ext"),
    size: number("size"),
    modified_ts: number("modified_ts"),
    root_id: nullableNumber("root_id"),
    content_status: text("content_status"),
    content_truncated: value.content_truncated === true,
    snippet: text("snippet"),
    truncated: value.truncated === true,
    error_code: typeof value.error_code === "string" ? value.error_code : null,
    extractor_version: text("extractor_version"),
    indexed_at: nullableNumber("indexed_at"),
    encoding: typeof value.encoding === "string" ? value.encoding : null,
    text_chars: number("text_chars"),
  };
}
