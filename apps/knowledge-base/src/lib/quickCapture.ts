import type { QuickCaptureInput } from "../types";

export const QUICK_CAPTURE_TARGET = "Inbox";
export const QUICK_CAPTURE_DEFAULT_TITLE = "빠른 캡처";
export const MAX_QUICK_CAPTURE_TITLE_CHARS = 200;
export const MAX_QUICK_CAPTURE_BODY_BYTES = 64 * 1024;
export const MAX_QUICK_CAPTURE_TAGS = 20;
export const MAX_QUICK_CAPTURE_TAG_CHARS = 48;
export const MAX_QUICK_CAPTURE_TAG_BYTES = 1_024;

export type QuickCaptureValidationCode =
  | "empty-body"
  | "invalid-text"
  | "title-too-long"
  | "body-too-large"
  | "too-many-tags"
  | "tag-too-long"
  | "tags-too-large"
  | "invalid-tag"
  | "sensitive-content";

const VALIDATION_MESSAGES: Record<QuickCaptureValidationCode, string> = {
  "empty-body": "빠른 캡처 본문을 입력하세요",
  "invalid-text": "빠른 캡처 입력이 올바르지 않습니다",
  "title-too-long": "빠른 캡처 입력이 올바르지 않습니다",
  "body-too-large": "빠른 캡처 입력이 올바르지 않습니다",
  "too-many-tags": "빠른 캡처 입력이 올바르지 않습니다",
  "tag-too-long": "빠른 캡처 입력이 올바르지 않습니다",
  "tags-too-large": "빠른 캡처 입력이 올바르지 않습니다",
  "invalid-tag": "빠른 캡처 입력이 올바르지 않습니다",
  "sensitive-content": "민감한 정보가 포함되어 있어 저장하지 않았습니다",
};

export class QuickCaptureValidationError extends Error {
  readonly code: QuickCaptureValidationCode;

  constructor(code: QuickCaptureValidationCode) {
    super(VALIDATION_MESSAGES[code]);
    this.name = "QuickCaptureValidationError";
    this.code = code;
  }
}

export function normalizeQuickCapture(input: QuickCaptureInput): QuickCaptureInput {
  const title = input.title.trim();
  if (containsSingleLineForbidden(title)) throw new QuickCaptureValidationError("invalid-text");
  if ([...title].length > MAX_QUICK_CAPTURE_TITLE_CHARS) {
    throw new QuickCaptureValidationError("title-too-long");
  }

  const body = input.body.replace(/\r\n/g, "\n").replace(/\r/g, "\n");
  if (!body.trim()) throw new QuickCaptureValidationError("empty-body");
  if (new TextEncoder().encode(body).byteLength > MAX_QUICK_CAPTURE_BODY_BYTES) {
    throw new QuickCaptureValidationError("body-too-large");
  }
  if (containsForbiddenText(body)) throw new QuickCaptureValidationError("invalid-text");

  if (input.tags.length > MAX_QUICK_CAPTURE_TAGS) {
    throw new QuickCaptureValidationError("too-many-tags");
  }
  const tags: string[] = [];
  const seen = new Set<string>();
  let tagBytes = 0;
  for (const raw of input.tags) {
    const tag = raw.trim();
    if (!tag) continue;
    if ([...tag].length > MAX_QUICK_CAPTURE_TAG_CHARS) {
      throw new QuickCaptureValidationError("tag-too-long");
    }
    if (
      containsSingleLineForbidden(tag)
      || [...tag].some((character) => [",", "[", "]", '"'].includes(character))
    ) {
      throw new QuickCaptureValidationError("invalid-tag");
    }
    tagBytes += new TextEncoder().encode(tag).byteLength;
    if (tagBytes > MAX_QUICK_CAPTURE_TAG_BYTES) {
      throw new QuickCaptureValidationError("tags-too-large");
    }
    if (!seen.has(tag)) {
      seen.add(tag);
      tags.push(tag);
    }
  }

  const normalized = {
    title: title || QUICK_CAPTURE_DEFAULT_TITLE,
    body,
    tags,
  };
  if (
    looksSensitive(normalized.title)
    || looksSensitive(normalized.body)
    || normalized.tags.some(looksSensitive)
  ) {
    throw new QuickCaptureValidationError("sensitive-content");
  }
  return normalized;
}

export function parseQuickCaptureTags(value: string): string[] {
  return value.split(",").map((tag) => tag.trim()).filter(Boolean);
}

function containsForbiddenText(value: string): boolean {
  for (const character of value) {
    const code = character.codePointAt(0) ?? 0;
    if (isControlCode(code) && code !== 0x09 && code !== 0x0a) {
      return true;
    }
  }
  return false;
}

function containsSingleLineForbidden(value: string): boolean {
  for (const character of value) {
    const code = character.codePointAt(0) ?? 0;
    if (isControlCode(code)) return true;
  }
  return false;
}

function isControlCode(code: number): boolean {
  return code < 0x20 || (code >= 0x7f && code <= 0x9f);
}

function isWord(character: string | undefined): boolean {
  return !!character && /^[A-Za-z0-9_-]$/u.test(character);
}

function hasAssignment(value: string, marker: string): boolean {
  const lower = value.toLocaleLowerCase("en-US");
  let offset = 0;
  while (offset < lower.length) {
    const found = lower.indexOf(marker, offset);
    if (found < 0) return false;
    if (!isWord(lower[found - 1]) && !isWord(lower[found + marker.length])) {
      const rest = lower.slice(found + marker.length).trimStart();
      if (rest.startsWith(":") || rest.startsWith("=")) {
        if (rest.slice(1).trim()) return true;
      }
    }
    offset = found + marker.length;
  }
  return false;
}

function looksSensitive(value: string): boolean {
  const lower = value.toLocaleLowerCase("en-US");
  if (lower.includes("-----begin ") && lower.includes("private key-----")) return true;
  for (const marker of [
    "api_key",
    "api-key",
    "access_key",
    "access-key",
    "client_secret",
    "client-secret",
    "authorization",
    "password",
    "passwd",
    "secret",
    "token",
  ]) {
    if (hasAssignment(value, marker)) return true;
  }
  for (const prefix of ["ghp_", "github_pat_", "xoxb-", "xoxp-", "akia"]) {
    const index = lower.indexOf(prefix);
    if (index >= 0 && lower.slice(index + prefix.length).split(/\s/u, 1)[0].length >= 12) {
      return true;
    }
  }
  const words = lower.split(/\s+/u);
  return words.some((word, index) => word === "bearer" && (words[index + 1]?.length ?? 0) >= 12);
}
