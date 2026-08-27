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
export const MAX_QUICK_CAPTURE_PREVIEW_ID_BYTES = 96;
export const MAX_QUICK_CAPTURE_CLIPBOARD_BYTES = MAX_QUICK_CAPTURE_RAW_BODY_BYTES;

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
  "title-too-long": "제목은 UTF-8 800바이트·200자 이내로 입력하세요",
  "body-too-large": "본문은 LF 기준 64 KiB(원문 128 KiB) 이내로 입력하세요",
  "too-many-tags": "태그는 최대 20개까지 입력하세요",
  "tag-too-long": "태그 하나는 UTF-8 192바이트·48자 이내로 입력하세요",
  "tags-too-large": "태그 전체는 UTF-8 1 KiB 이내로 입력하세요",
  "invalid-tag": "태그에 줄바꿈·쉼표·대괄호·따옴표를 사용할 수 없습니다",
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
  if (containsUnpairedSurrogate(input.title)) {
    throw new QuickCaptureValidationError("invalid-text");
  }
  if (utf8ByteLengthAtMost(input.title, MAX_QUICK_CAPTURE_TITLE_BYTES) === null) {
    throw new QuickCaptureValidationError("title-too-long");
  }
  if (containsSingleLineForbidden(input.title)) {
    throw new QuickCaptureValidationError("invalid-text");
  }
  const title = input.title.trim();
  if (containsSingleLineForbidden(title)) throw new QuickCaptureValidationError("invalid-text");
  if (
    utf8ByteLengthAtMost(title, MAX_QUICK_CAPTURE_TITLE_BYTES) === null
    || [...title].length > MAX_QUICK_CAPTURE_TITLE_CHARS
  ) {
    throw new QuickCaptureValidationError("title-too-long");
  }

  if (containsUnpairedSurrogate(input.body)) {
    throw new QuickCaptureValidationError("invalid-text");
  }
  if (utf8ByteLengthAtMost(input.body, MAX_QUICK_CAPTURE_RAW_BODY_BYTES) === null) {
    throw new QuickCaptureValidationError("body-too-large");
  }
  const body = input.body.replace(/\r\n/g, "\n").replace(/\r/g, "\n");
  if (!body.trim()) throw new QuickCaptureValidationError("empty-body");
  if (utf8ByteLengthAtMost(body, MAX_QUICK_CAPTURE_BODY_BYTES) === null) {
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
    if (containsUnpairedSurrogate(raw)) {
      throw new QuickCaptureValidationError("invalid-tag");
    }
    if (utf8ByteLengthAtMost(raw, MAX_QUICK_CAPTURE_TAG_ITEM_BYTES) === null) {
      throw new QuickCaptureValidationError("tag-too-long");
    }
    if (containsSingleLineForbidden(raw)) {
      throw new QuickCaptureValidationError("invalid-tag");
    }
    const tag = raw.trim();
    if (!tag) continue;
    if ([...tag].length > MAX_QUICK_CAPTURE_TAG_CHARS) {
      throw new QuickCaptureValidationError("tag-too-long");
    }
    if (utf8ByteLengthAtMost(tag, MAX_QUICK_CAPTURE_TAG_ITEM_BYTES) === null) {
      throw new QuickCaptureValidationError("tag-too-long");
    }
    if (
      containsSingleLineForbidden(tag)
      || [...tag].some((character) => [",", "[", "]", '"'].includes(character))
    ) {
      throw new QuickCaptureValidationError("invalid-tag");
    }
    tagBytes += utf8ByteLengthAtMost(tag, MAX_QUICK_CAPTURE_TAG_ITEM_BYTES) ?? (MAX_QUICK_CAPTURE_TAG_BYTES + 1);
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
  // Keep malformed/direct API input bounded before the native validator sees
  // it.  The DOM also has a maxlength, but IPC callers and tests do not.
  const tags: string[] = [];
  let start = 0;
  for (let index = 0; index <= value.length && tags.length <= MAX_QUICK_CAPTURE_TAGS; index += 1) {
    if (index !== value.length && value[index] !== ",") continue;
    const boundedEnd = Math.min(index, start + MAX_QUICK_CAPTURE_TAG_ITEM_BYTES * 2 + 1);
    const tag = value.slice(start, boundedEnd).trim();
    if (tag) tags.push(tag);
    start = index + 1;
  }
  return tags;
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

/** Return the UTF-8 byte count when it fits, without allocating an encoded copy. */
function utf8ByteLengthAtMost(value: string, limit: number): number | null {
  let bytes = 0;
  for (let index = 0; index < value.length; index += 1) {
    const code = value.charCodeAt(index);
    if (code >= 0xd800 && code <= 0xdbff) {
      const next = value.charCodeAt(index + 1);
      if (next < 0xdc00 || next > 0xdfff) return null;
      bytes += 4;
      index += 1;
    } else if (code >= 0xdc00 && code <= 0xdfff) {
      return null;
    } else if (code <= 0x7f) {
      bytes += 1;
    } else if (code <= 0x7ff) {
      bytes += 2;
    } else {
      bytes += 3;
    }
    if (bytes > limit) return null;
  }
  return bytes;
}

export function quickCaptureUtf8Bytes(value: string): number {
  return utf8ByteLengthAtMost(value, Number.MAX_SAFE_INTEGER) ?? new TextEncoder().encode(value).byteLength;
}

export function isQuickCaptureUtf8Within(value: string, limit: number): boolean {
  return utf8ByteLengthAtMost(value, limit) !== null;
}

export function quickCaptureUnicodeScalars(value: string): number {
  let count = 0;
  for (let index = 0; index < value.length; index += 1) {
    const code = value.charCodeAt(index);
    if (code >= 0xd800 && code <= 0xdbff && value.charCodeAt(index + 1) >= 0xdc00 && value.charCodeAt(index + 1) <= 0xdfff) {
      index += 1;
    }
    count += 1;
  }
  return count;
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
  // Identifier separators are boundaries so provider-shaped keys such as
  // `AZURE_CLIENT_SECRET` still match the `client_secret` marker.
  return !!character && /^[A-Za-z0-9]$/u.test(character);
}

function hasAssignment(value: string, marker: string): boolean {
  const lower = value.toLocaleLowerCase("en-US");
  let offset = 0;
  while (offset < lower.length) {
    const found = lower.indexOf(marker, offset);
    if (found < 0) return false;
    if (!isWord(lower[found - 1]) && !isWord(lower[found + marker.length])) {
      let rest = lower.slice(found + marker.length).trimStart();
      // Accept JSON/YAML-style quoted keys (`"api_key": value`) without
      // broadening the marker match into arbitrary prose.
      if (/^["'`]/u.test(rest)) rest = rest.slice(1).trimStart();
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
  if (lower.includes("private key")) return true;
  for (const marker of [
    "api_key",
    "api-key",
    "apikey",
    "access_key",
    "access-key",
    "accesskey",
    "secret_key",
    "secret-key",
    "secretkey",
    "auth_token",
    "auth-token",
    "authtoken",
    "access_token",
    "access-token",
    "accesstoken",
    "refresh_token",
    "refresh-token",
    "refreshtoken",
    "session_token",
    "session-token",
    "sessiontoken",
    "client_secret",
    "client-secret",
    "client_key",
    "client-key",
    "authorization",
    "x-api-key",
    "x_api_key",
    "x-auth-token",
    "x_auth_token",
    "cookie",
    "set-cookie",
    "set_cookie",
    "credential",
    "credentials",
    "connection_string",
    "connection-string",
    "database_url",
    "database-url",
    "connection_uri",
    "connection-uri",
    "client_id",
    "client-id",
    "api_token",
    "api-token",
    "app_token",
    "app-token",
    "private_token",
    "private-token",
    "deploy_token",
    "deploy-token",
    "webhook_secret",
    "webhook-secret",
    "webhook_token",
    "webhook-token",
    "signing_key",
    "signing-key",
    "encryption_key",
    "encryption-key",
    "secret_access_key",
    "secret-access-key",
    "aws_secret_access_key",
    "aws-secret-access-key",
    "aws_session_token",
    "aws-session-token",
    "account_key",
    "account-key",
    "shared_access_signature",
    "shared-access-signature",
    "google_application_credentials",
    "google-application-credentials",
    "service_account",
    "service-account",
    "private_key",
    "private-key",
    "password",
    "passwd",
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
    "xoxa-",
    "xoxb-",
    "xoxp-",
    "xoxr-",
    "xoxs-",
    "akia",
    "asia",
    "aiza",
    "sk-",
    "hf_",
    "r8_",
    "vercel_",
    "dop_v1_",
    "whsec_",
    "sq0atp-",
    "atatt-",
    "ya29.",
    "pk_live_",
    "rk_live_",
    "sk_live_",
    "sg.",
    "glpat-",
    "npm_",
    "pypi-",
    "mfa.",
    "sntrys_",
    "hvs.",
    "hvb.",
    "sbp_",
    "lin_api_",
    "pmak-",
    "xapp-",
  ]) {
    if (containsCredentialPrefix(lower, prefix)) {
      return true;
    }
  }
  return containsAuthCredential(lower)
    || containsJwtLikeToken(lower)
    || containsTelegramBotToken(lower);
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

/** Scan common auth schemes without materializing an untrusted word array. */
function containsAuthCredential(value: string): boolean {
  let offset = 0;
  let sawScheme = false;
  while (offset < value.length) {
    while (offset < value.length && /\s/u.test(value[offset] ?? "")) offset += 1;
    const start = offset;
    while (offset < value.length && !/\s/u.test(value[offset] ?? "")) offset += 1;
    if (start === offset) break;
    const word = value.slice(start, offset).replace(/^[:=,;()[\]"'`]+|[:=,;()[\]"'`]+$/gu, "");
    if (sawScheme && utf8ByteLengthAtMost(word, 11) === null) return true;
    sawScheme = word === "bearer" || word === "basic" || word === "token";
  }
  return false;
}

function containsJwtLikeToken(value: string): boolean {
  return value.split(/\s+/u).some((token) => {
    const trimmed = token.replace(/^[:=,;()[\]"']+|[:=,;()[\]"']+$/gu, "");
    const segments = trimmed.split(".");
    return segments.length === 3 && segments.every((segment) =>
      segment.length >= 10 && /^[A-Za-z0-9_-]+$/u.test(segment));
  });
}

function containsTelegramBotToken(value: string): boolean {
  return value.split(/\s+/u).some((token) => {
    const trimmed = token.replace(/^[:=,;()[\]"']+|[:=,;()[\]"']+$/gu, "");
    const separator = trimmed.indexOf(":");
    if (separator < 0) return false;
    const id = trimmed.slice(0, separator);
    const secret = trimmed.slice(separator + 1);
    return id.length >= 8
      && id.length <= 12
      && /^\d+$/u.test(id)
      && secret.length >= 30
      && /^[A-Za-z0-9_-]+$/u.test(secret);
  });
}

export function isSafeQuickCapturePreviewId(value: unknown): value is string {
  return typeof value === "string"
    && value.length <= MAX_QUICK_CAPTURE_PREVIEW_ID_BYTES
    && /^qc-[1-9]\d{0,19}$/u.test(value);
}
