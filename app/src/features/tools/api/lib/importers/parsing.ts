import { ImportError } from "./model";
export type JsonObject = Record<string, unknown>;
export function object(value: unknown): JsonObject {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new ImportError("가져올 수 없는 형식입니다.");
  return value as JsonObject;
}
export const optionalObject = (value: unknown) => (value == null ? {} : object(value));
export function rows(value: unknown): unknown[] {
  if (value == null) return [];
  if (!Array.isArray(value)) throw new ImportError("가져올 수 없는 형식입니다.");
  return value;
}
export const text = (value: unknown, fallback = ""): string => (typeof value === "string" ? value : fallback);
export function readJson(source: string): JsonObject {
  if (new TextEncoder().encode(source).length > 16 * 1024 * 1024) throw new ImportError("가져올 수 없는 형식입니다.");
  try {
    return object(JSON.parse(source));
  } catch {
    throw new ImportError("가져올 수 없는 형식입니다.");
  }
}
export function formRows(value: unknown): string {
  return rows(value)
    .map(object)
    .filter((row) => row.disabled !== true)
    .map((row) => `${encodeURIComponent(text(row.key))}=${encodeURIComponent(text(row.value))}`)
    .join("\n");
}
