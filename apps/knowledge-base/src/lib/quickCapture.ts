import type { QuickCaptureInput } from "../types";

export const QUICK_CAPTURE_TARGET = "Inbox";
export const QUICK_CAPTURE_DEFAULT_TITLE = "빠른 캡처";
export const MAX_QUICK_CAPTURE_TITLE_CHARS = 200;
export const MAX_QUICK_CAPTURE_TITLE_BYTES = MAX_QUICK_CAPTURE_TITLE_CHARS * 4;
export const MAX_QUICK_CAPTURE_BODY_BYTES = 64 * 1024;
export const MAX_QUICK_CAPTURE_RAW_BODY_BYTES = MAX_QUICK_CAPTURE_BODY_BYTES * 2;
export const MAX_QUICK_CAPTURE_TAGS = 20;
export const MAX_QUICK_CAPTURE_TAG_CHARS = 48;
export const MAX_QUICK_CAPTURE_TAG_ITEM_BYTES = MAX_QUICK_CAPTURE_TAG_CHARS * 4;
export const MAX_QUICK_CAPTURE_TAG_BYTES = 1_024;
export const MAX_QUICK_CAPTURE_PATH_CHARS = 160;

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
  const encoder = new TextEncoder();
  if (encoder.encode(input.title).byteLength > MAX_QUICK_CAPTURE_TITLE_BYTES) {
    throw new QuickCaptureValidationError("title-too-long");
  }
  const title = input.title.trim();
  if (containsSingleLineForbidden(title)) throw new QuickCaptureValidationError("invalid-text");
  if (
    encoder.encode(title).byteLength > MAX_QUICK_CAPTURE_TITLE_BYTES
    || [...title].length > MAX_QUICK_CAPTURE_TITLE_CHARS
  ) {
    throw new QuickCaptureValidationError("title-too-long");
  }

  if (encoder.encode(input.body).byteLength > MAX_QUICK_CAPTURE_RAW_BODY_BYTES) {
    throw new QuickCaptureValidationError("body-too-large");
  }
  const body = input.body.replace(/\r\n/g, "\n").replace(/\r/g, "\n");
  if (!body.trim()) throw new QuickCaptureValidationError("empty-body");
  if (encoder.encode(body).byteLength > MAX_QUICK_CAPTURE_BODY_BYTES) {
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
    if (encoder.encode(raw).byteLength > MAX_QUICK_CAPTURE_TAG_ITEM_BYTES) {
      throw new QuickCaptureValidationError("tag-too-long");
    }
    const tag = raw.trim();
    if (!tag) continue;
    if ([...tag].length > MAX_QUICK_CAPTURE_TAG_CHARS) {
      throw new QuickCaptureValidationError("tag-too-long");
    }
    if (encoder.encode(tag).byteLength > MAX_QUICK_CAPTURE_TAG_ITEM_BYTES) {
      throw new QuickCaptureValidationError("tag-too-long");
    }
    if (
      containsSingleLineForbidden(tag)
      || [...tag].some((character) => [",", "[", "]", '"'].includes(character))
    ) {
      throw new QuickCaptureValidationError("invalid-tag");
    }
    tagBytes += encoder.encode(tag).byteLength;
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

/** The native command returns only this fixed, root-relative filename shape. */
export function isSafeQuickCapturePath(value: unknown): value is string {
  return typeof value === "string"
    && value.length <= MAX_QUICK_CAPTURE_PATH_CHARS
    && /^Inbox\/quick-capture-\d{4,}-(?:0[1-9]|1[0-2])-(?:0[1-9]|[12]\d|3[01])-(?:[01]\d|2[0-3])-[0-5]\d-[0-5]\d(?:-[2-9]|-[1-9]\d|-100)?\.md$/u.test(value);
}

function containsForbiddenText(value: string): boolean {
  if (containsUnpairedSurrogate(value)) return true;
  for (const character of value) {
    const code = character.codePointAt(0) ?? 0;
    if (isLineSeparator(code) || (isControlCode(code) && code !== 0x09 && code !== 0x0a)) {
      return true;
    }
  }
  return false;
}

function containsSingleLineForbidden(value: string): boolean {
  if (containsUnpairedSurrogate(value)) return true;
  for (const character of value) {
    const code = character.codePointAt(0) ?? 0;
    if (isLineSeparator(code) || isControlCode(code)) return true;
  }
  return false;
}

function isLineSeparator(code: number): boolean {
  return code === 0x2028 || code === 0x2029;
}

function containsUnpairedSurrogate(value: string): boolean {
  for (let index = 0; index < value.length; index += 1) {
    const code = value.charCodeAt(index);
    if (code >= 0xd800 && code <= 0xdbff) {
      const next = value.charCodeAt(index + 1);
      if (next >= 0xdc00 && next <= 0xdfff) {
        index += 1;
      } else {
        return true;
      }
    } else if (code >= 0xdc00 && code <= 0xdfff) {
      return true;
    }
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
  const encoder = new TextEncoder();
  if (lower.includes("-----begin ") && lower.includes("private key-----")) return true;
  for (const marker of [
    "api_key",
    "api-key",
    "access_key",
    "access-key",
    "client_secret",
    "client-secret",
    "authorization",
    "x-api-key",
    "x_api_key",
    "password",
    "passwd",
    "private-key",
    "private_key",
    "secret",
    "token",
  ]) {
    if (hasAssignment(value, marker)) return true;
  }
  for (const prefix of [
    "ghp_",
    "github_pat_",
    "gho_",
    "ghu_",
    "ghs_",
    "ghr_",
    "xoxb-",
    "xoxp-",
    "akia",
    "sk-",
    "glpat-",
    "npm_",
    "pypi-",
  ]) {
    if (containsCredentialPrefix(lower, prefix)) {
      return true;
    }
  }
  return containsBearerCredential(lower, encoder);
}

function containsCredentialPrefix(value: string, prefix: string): boolean {
  let offset = 0;
  while (offset < value.length) {
    const found = value.indexOf(prefix, offset);
    if (found < 0) return false;
    const token = value.slice(found + prefix.length).match(/^\S*/u)?.[0] ?? "";
    if ([...token].length >= 12) return true;
    offset = found + prefix.length;
  }
  return false;
}

/** Scan bearer tokens without materializing an untrusted word array. */
function containsBearerCredential(value: string, encoder: TextEncoder): boolean {
  let offset = 0;
  let sawBearer = false;
  while (offset < value.length) {
    while (offset < value.length && /\s/u.test(value[offset] ?? "")) offset += 1;
    const start = offset;
    while (offset < value.length && !/\s/u.test(value[offset] ?? "")) offset += 1;
    if (start === offset) break;
    const word = value.slice(start, offset);
    if (sawBearer && encoder.encode(word).byteLength >= 12) return true;
    sawBearer = word === "bearer";
  }
  return false;
}
