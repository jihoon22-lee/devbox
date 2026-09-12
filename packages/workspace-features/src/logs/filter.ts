import type { FilterSpec, LogRecord } from "./types";

export const MAX_FILTER_BYTES = 512;
export const MAX_FIELD_BYTES = 4 * 1024;
export const MAX_HIGHLIGHT_REGEX_BYTES = 128;

function utf8Width(codePoint: number): number {
  if (codePoint <= 0x7f) return 1;
  if (codePoint <= 0x7ff) return 2;
  if (codePoint <= 0xffff) return 3;
  return 4;
}

export function utf8ByteLength(value: string): number {
  let bytes = 0;
  for (const character of value) bytes += utf8Width(character.codePointAt(0) ?? 0xfffd);
  return bytes;
}

/** Truncate by UTF-8 bytes without splitting a Unicode scalar value. */
export function truncateUtf8(value: string, maxBytes: number): string {
  let bytes = 0;
  let end = 0;
  for (const character of value) {
    const width = utf8Width(character.codePointAt(0) ?? 0xfffd);
    if (bytes + width > maxBytes) break;
    bytes += width;
    end += character.length;
  }
  return end === value.length ? value : value.slice(0, end);
}

function hasControl(value: string): boolean {
  return [...value].some((character) => {
    const codePoint = character.codePointAt(0) ?? 0;
    return codePoint < 0x20 || (codePoint >= 0x7f && codePoint <= 0x9f);
  });
}

function hasUnsafeRegexConstruct(value: string): boolean {
  // Browser mode must fail closed too: unlike the Rust regex engine, native
  // JavaScript RegExp can backtrack catastrophically. Keep highlighting and
  // fixture filtering to a small, predictable subset. The native command
  // remains authoritative in Tauri mode.
  return /\\(?:[1-9]|k<)|\(\?[=!<]/.test(value)
    || /\([^()]*[+*][^()]*\)[+*{]/.test(value)
    || /\([^()]*\|[^()]*\)[+*{]/.test(value)
    || /\{\s*\d{3,}(?:\s*,\s*\d*)?\s*\}/.test(value);
}

/** Build a bounded browser regexp, or return null to fail closed. */
export function createSafeRegex(value: string, flags = ""): RegExp | null {
  if (
    utf8ByteLength(value) > MAX_HIGHLIGHT_REGEX_BYTES
    || hasControl(value)
    || hasUnsafeRegexConstruct(value)
  ) return null;
  try {
    return new RegExp(value, flags);
  } catch {
    return null;
  }
}

function escapeRegex(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

export function createLiteralRegex(value: string, flags = ""): RegExp | null {
  try {
    return new RegExp(escapeRegex(value), flags);
  } catch {
    return null;
  }
}

export function filterRecords(records: LogRecord[], filter: FilterSpec): LogRecord[] {
  if (
    utf8ByteLength(filter.text) > MAX_FILTER_BYTES
    || hasControl(filter.text)
    || (filter.field !== undefined && (utf8ByteLength(filter.field) > MAX_FIELD_BYTES || hasControl(filter.field)))
    || (filter.fieldValue !== undefined && (utf8ByteLength(filter.fieldValue) > MAX_FIELD_BYTES || hasControl(filter.fieldValue)))
    || (filter.startAt !== undefined && (!Number.isSafeInteger(filter.startAt)))
    || (filter.endAt !== undefined && (!Number.isSafeInteger(filter.endAt)))
    || (filter.startAt !== undefined && filter.endAt !== undefined && filter.startAt > filter.endAt)
  ) return [];

  let matcher: RegExp | null = null;
  if (filter.regex && filter.text) {
    matcher = createSafeRegex(filter.text);
    if (!matcher) return [];
  }
  return records.filter((record) => {
    if (filter.sourceId && record.sourceId !== filter.sourceId) return false;
    if (filter.level && record.level !== filter.level) return false;
    if (filter.startAt !== undefined && (record.timestampMillis === null || record.timestampMillis < filter.startAt)) return false;
    if (filter.endAt !== undefined && (record.timestampMillis === null || record.timestampMillis >= filter.endAt)) return false;
    if (filter.field && filter.fieldValue !== undefined && record.fields[filter.field] !== filter.fieldValue) return false;
    if (!filter.text) return true;
    if (matcher) {
      return matcher.test(record.message)
        || Object.entries(record.fields).some(([key, value]) => matcher.test(key) || matcher.test(value));
    }
    return record.message.includes(filter.text)
      || Object.entries(record.fields).some(([key, value]) => key.includes(filter.text) || value.includes(filter.text));
  });
}

export function recordKey(record: LogRecord): string {
  return `${record.sourceId}:${record.sequence}`;
}
