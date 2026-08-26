import type { RequestTemplate, WebSocketMessage, WebSocketMessageInput } from "../types";

export const MAX_RETAINED_MESSAGES = 10_000;
export const MAX_BUFFER_BYTES = 20 * 1024 * 1024;
export const MAX_MESSAGE_BYTES = 4 * 1024 * 1024;
export const MAX_TEXT_PREVIEW_BYTES = 64 * 1024;
export const MAX_BINARY_PREVIEW_BYTES = 4096;
export const MAX_CONTROL_PAYLOAD_BYTES = 125;
export const MAX_CLOSE_REASON_BYTES = 123;
export const MAX_REQUEST_HEADERS = 100;
export const MAX_REQUEST_COOKIES = 100;
export const MAX_REQUEST_PARAMS = 100;
export const MAX_URL_BYTES = 8 * 1024;
export const MIN_TIMEOUT_MS = 100;
export const MAX_TIMEOUT_MS = 120_000;

export const ENDPOINT_ERROR = "WebSocket endpoint URL이 올바르지 않습니다";
export const CREDENTIAL_QUERY_ERROR = "WebSocket endpoint query에 credential을 넣을 수 없습니다";
export const MESSAGE_TOO_LARGE = "WebSocket message가 허용된 크기를 초과했습니다";
export const CLOSE_CODE_INVALID = "WebSocket close code가 올바르지 않습니다";
export const CLOSE_REASON_INVALID = "WebSocket close reason이 올바르지 않습니다";

const SENSITIVE_NAME = /(authorization|cookie|set[-_]?cookie|api[-_]?key|api[-_]?value|token|secret|password|passwd|private[-_]?key|username)/iu;
const KNOWN_TOKEN = /(?:sk-|ghp_|github_pat_|glpat-|xox[bprsa]-)[A-Za-z0-9_-]{12,}/gu;
const decoder = new TextDecoder("utf-8", { fatal: true, ignoreBOM: false });
const encoder = new TextEncoder();

export function utf8Bytes(value: string): number {
  return encoder.encode(value).byteLength;
}

export function utf8Truncate(value: string, maxBytes: number): { value: string; truncated: boolean } {
  if (utf8Bytes(value) <= maxBytes) return { value, truncated: false };
  let output = "";
  for (const character of value) {
    if (utf8Bytes(output + character) > maxBytes) break;
    output += character;
  }
  return { value: output, truncated: true };
}

export function isSensitiveName(name: string): boolean {
  return SENSITIVE_NAME.test(name);
}

export function validateWebSocketEndpoint(value: string): void {
  if (utf8Bytes(value) > MAX_URL_BYTES || /[\u0000-\u001f\u007f]/u.test(value)) {
    throw new Error(ENDPOINT_ERROR);
  }
  let url: URL;
  try {
    url = new URL(value);
  } catch {
    throw new Error(ENDPOINT_ERROR);
  }
  if ((url.protocol !== "ws:" && url.protocol !== "wss:") || !url.hostname
    || url.username || url.password || url.hash) {
    throw new Error(ENDPOINT_ERROR);
  }
  for (const key of url.searchParams.keys()) {
    if (isSensitiveName(key)) throw new Error(CREDENTIAL_QUERY_ERROR);
  }
}

export function buildWebSocketUrl(base: string, params: readonly { key: string; value: string }[]): string {
  validateWebSocketEndpoint(base);
  const url = new URL(base);
  for (const parameter of params) {
    if (!parameter.key) continue;
    if (isSensitiveName(parameter.key)) throw new Error(CREDENTIAL_QUERY_ERROR);
    url.searchParams.append(parameter.key, parameter.value);
  }
  const output = url.toString();
  if (utf8Bytes(output) > MAX_URL_BYTES) throw new Error("WebSocket 요청 URL이 너무 깁니다");
  return output;
}

export function validateCloseCode(code: number | undefined): number {
  const candidate = code ?? 1000;
  if (!Number.isInteger(candidate)
    || !(candidate === 1000 || (candidate >= 1001 && candidate <= 1003)
      || (candidate >= 1007 && candidate <= 1014)
      || (candidate >= 3000 && candidate <= 4999))) {
    throw new Error(CLOSE_CODE_INVALID);
  }
  return candidate;
}

export function validateCloseReason(reason: string): void {
  if (utf8Bytes(reason) > MAX_CLOSE_REASON_BYTES || reason.includes("\0")) {
    throw new Error(CLOSE_REASON_INVALID);
  }
}

export function validateWebSocketRequest(request: RequestTemplate): void {
  if (request.headers.length > MAX_REQUEST_HEADERS
    || request.cookies.length > MAX_REQUEST_COOKIES
    || request.params.length > MAX_REQUEST_PARAMS) {
    throw new Error("WebSocket 요청 항목 수가 제한을 초과했습니다");
  }
  if (request.timeout_ms < MIN_TIMEOUT_MS || request.timeout_ms > MAX_TIMEOUT_MS) {
    throw new Error("WebSocket 연결 timeout 범위가 올바르지 않습니다");
  }
  for (const header of request.headers) {
    if (utf8Bytes(header.key) > 256 || utf8Bytes(header.value) > 64 * 1024) {
      throw new Error("WebSocket 요청 header가 너무 깁니다");
    }
    if (header.enabled !== false && header.key.trim()) {
      try {
        // Headers performs the same token/value checks required by browser preview.
        new Headers([[header.key.trim(), header.value]]);
      } catch {
        throw new Error("WebSocket 요청 header가 올바르지 않습니다");
      }
    }
  }
  for (const parameter of request.params) {
    if (utf8Bytes(parameter.key) > 64 * 1024 || utf8Bytes(parameter.value) > 64 * 1024) {
      throw new Error("WebSocket 요청 parameter가 올바르지 않습니다");
    }
  }
  validateWebSocketEndpoint(request.url);
  buildWebSocketUrl(request.url, request.params);
  if (request.auth && !["none", "basic", "bearer", "apikey"].includes(request.auth.kind)) {
    throw new Error("WebSocket 인증 설정이 올바르지 않습니다");
  }
}

export function messageSize(message: WebSocketMessage): number {
  if (message.kind === "binary" || message.kind === "ping" || message.kind === "pong") {
    return message.binarySize ?? 0;
  }
  if (message.kind === "close") return utf8Bytes(message.closeReason ?? "");
  return utf8Bytes(message.text ?? "");
}

/** Oldest-first bounded retention shared by native DTO consumers and browser preview. */
export class WebSocketMessageBuffer {
  private messagesValue: WebSocketMessage[] = [];
  private bytesValue = 0;
  private evictedValue = 0;
  private evictedIdsValue: number[] = [];

  push(message: WebSocketMessage): number {
    this.evictedIdsValue = [];
    this.messagesValue.push(message);
    this.bytesValue += messageSize(message);
    let removed = 0;
    while (this.messagesValue.length > MAX_RETAINED_MESSAGES || this.bytesValue > MAX_BUFFER_BYTES) {
      const oldest = this.messagesValue.shift();
      if (!oldest) {
        this.bytesValue = 0;
        break;
      }
      this.bytesValue = Math.max(0, this.bytesValue - messageSize(oldest));
      this.evictedValue += 1;
      this.evictedIdsValue.push(oldest.id);
      removed += 1;
    }
    return removed;
  }

  get messages(): readonly WebSocketMessage[] { return this.messagesValue; }
  get bytes(): number { return this.bytesValue; }
  get evicted(): number { return this.evictedValue; }

  takeEvictedIds(): readonly number[] {
    const ids = this.evictedIdsValue;
    this.evictedIdsValue = [];
    return ids;
  }
}

export function binaryToHex(payload: Uint8Array): string {
  const shown = payload.slice(0, MAX_BINARY_PREVIEW_BYTES);
  let output = "";
  for (const byte of shown) output += byte.toString(16).padStart(2, "0");
  return payload.byteLength > shown.byteLength ? `${output}…` : output;
}

export function encodeBase64(payload: Uint8Array): string {
  let binary = "";
  const chunk = 0x8000;
  for (let index = 0; index < payload.length; index += chunk) {
    binary += String.fromCharCode(...payload.subarray(index, index + chunk));
  }
  return globalThis.btoa(binary);
}

export function textToBytes(value: string, maxBytes = MAX_CONTROL_PAYLOAD_BYTES): Uint8Array {
  const payload = encoder.encode(value);
  if (payload.byteLength > maxBytes) throw new Error(MESSAGE_TOO_LARGE);
  return payload;
}

export function hexToBytes(value: string): Uint8Array {
  const normalized = value.replace(/[\s:_-]/gu, "");
  if (normalized.length % 2 !== 0 || !/^[0-9a-f]*$/iu.test(normalized)) {
    throw new Error("WebSocket binary hex가 올바르지 않습니다");
  }
  if (normalized.length > MAX_MESSAGE_BYTES * 2) throw new Error(MESSAGE_TOO_LARGE);
  const payload = new Uint8Array(normalized.length / 2);
  for (let index = 0; index < payload.length; index += 1) {
    payload[index] = Number.parseInt(normalized.slice(index * 2, index * 2 + 2), 16);
  }
  if (payload.byteLength > MAX_MESSAGE_BYTES) throw new Error(MESSAGE_TOO_LARGE);
  return payload;
}

export function maskWebSocketText(value: string, request: RequestTemplate): string {
  const secrets = [
    request.auth?.username,
    request.auth?.password,
    request.auth?.token,
    request.auth?.api_value,
    ...request.headers.filter((header) => header.enabled !== false && isSensitiveName(header.key)).map((header) => header.value),
    ...request.cookies.filter((cookie) => cookie.enabled !== false).map((cookie) => cookie.value),
    ...request.params.filter((parameter) => isSensitiveName(parameter.key)).map((parameter) => parameter.value),
  ].filter((secret): secret is string => Boolean(secret));
  let output = secrets.sort((left, right) => right.length - left.length).reduce(
    (result, secret) => result.split(secret).join("[REDACTED]"),
    value,
  );
  output = output.replace(KNOWN_TOKEN, "[REDACTED]");
  try {
    const parsed = JSON.parse(output) as unknown;
    const masked = redactWebSocketJson(parsed);
    if (masked.changed) output = JSON.stringify(masked.value) ?? "[REDACTED]";
  } catch {
    // Plain text follows the assignment masking below.
  }
  return output.replace(
    /((?:authorization|cookie|set[-_]?cookie|api[-_]?key|api[-_]?value|token|secret|password|passwd|private[-_]?key|username)\s*[=:]\s*)([^\s,;&]+)/giu,
    "$1[REDACTED]",
  );
}

function redactWebSocketJson(value: unknown, key = ""): { value: unknown; changed: boolean } {
  if (isSensitiveName(key)) return { value: "[REDACTED]", changed: true };
  if (Array.isArray(value)) {
    let changed = false;
    const next = value.map((item) => {
      const masked = redactWebSocketJson(item);
      changed ||= masked.changed;
      return masked.value;
    });
    return { value: next, changed };
  }
  if (value && typeof value === "object") {
    let changed = false;
    const next = Object.fromEntries(
      Object.entries(value as Record<string, unknown>).map(([childKey, child]) => {
        const masked = redactWebSocketJson(child, childKey);
        changed ||= masked.changed;
        return [childKey, masked.value];
      }),
    );
    return { value: next, changed };
  }
  return { value, changed: false };
}

function containsSecretBytes(payload: Uint8Array, request: RequestTemplate): boolean {
  const secrets = [
    request.auth?.username,
    request.auth?.password,
    request.auth?.token,
    request.auth?.api_value,
    ...request.headers.filter((header) => header.enabled !== false && isSensitiveName(header.key)).map((header) => header.value),
    ...request.cookies.filter((cookie) => cookie.enabled !== false).map((cookie) => cookie.value),
  ].filter((secret): secret is string => Boolean(secret));
  const directMatch = secrets.some((secret) => {
    const bytes = encoder.encode(secret);
    if (!bytes.length || bytes.length > payload.length) return false;
    for (let start = 0; start <= payload.length - bytes.length; start += 1) {
      let matches = true;
      for (let offset = 0; offset < bytes.length; offset += 1) {
        if (payload[start + offset] !== bytes[offset]) {
          matches = false;
          break;
        }
      }
      if (matches) return true;
    }
    return false;
  });
  if (directMatch) return true;
  const decoded = new TextDecoder().decode(payload);
  return maskWebSocketText(decoded, request) !== decoded;
}

export function makeBinaryMessage(
  id: number,
  direction: "sent" | "received",
  payload: Uint8Array,
  request: RequestTemplate,
): WebSocketMessage {
  if (payload.byteLength > MAX_MESSAGE_BYTES) throw new Error(MESSAGE_TOO_LARGE);
  let binaryText: string | undefined;
  let textTruncated = false;
  try {
    const decoded = decoder.decode(payload);
    const safe = utf8Truncate(maskWebSocketText(decoded, request), MAX_TEXT_PREVIEW_BYTES);
    binaryText = safe.value;
    textTruncated = safe.truncated;
  } catch {
    binaryText = undefined;
  }
  const secret = containsSecretBytes(payload, request);
  return {
    id,
    direction,
    kind: "binary",
    binaryHex: secret ? "[REDACTED]" : binaryToHex(payload),
    binaryText,
    binarySize: payload.byteLength,
    binaryTruncated: textTruncated || payload.byteLength > MAX_BINARY_PREVIEW_BYTES,
  };
}

export function makeTextMessage(
  id: number,
  direction: "sent" | "received",
  text: string,
  request: RequestTemplate,
): WebSocketMessage {
  if (utf8Bytes(text) > MAX_MESSAGE_BYTES) throw new Error(MESSAGE_TOO_LARGE);
  const safe = utf8Truncate(maskWebSocketText(text, request), MAX_TEXT_PREVIEW_BYTES);
  return { id, direction, kind: "text", text: safe.value, textTruncated: safe.truncated };
}

export function toNativeMessageInput(
  kind: "text" | "binary" | "ping" | "pong",
  value: string,
  encoding: "text" | "hex" = "text",
): WebSocketMessageInput {
  if (kind === "text") {
    if (utf8Bytes(value) > MAX_MESSAGE_BYTES) throw new Error(MESSAGE_TOO_LARGE);
    return { kind, text: value, data: "" };
  }
  const payload = encoding === "hex" ? hexToBytes(value) : textToBytes(
    value,
    kind === "ping" || kind === "pong" ? MAX_CONTROL_PAYLOAD_BYTES : MAX_MESSAGE_BYTES,
  );
  if (kind !== "ping" && payload.byteLength > MAX_MESSAGE_BYTES) throw new Error(MESSAGE_TOO_LARGE);
  return { kind, text: "", data: encodeBase64(payload) };
}
