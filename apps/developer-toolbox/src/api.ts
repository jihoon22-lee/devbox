import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { readText } from "@tauri-apps/plugin-clipboard-manager";
import { isTauri } from "./lib/isTauri";
import { generateIdentifiers, type IdentifierOptions } from "./tools/ids";
import {
  browserHmacGenerate,
  browserHmacVerify,
  type HmacRequest,
  type HmacVerifyRequest,
} from "./tools/hmac";
import { browserVerifyJwt, type JwtVerifyRequest } from "./tools/jwt";
import {
  generateQr as generateBrowserQr,
  QR_ERROR_MESSAGES,
  QrGenerationError,
  type GenerateQrRequest,
  type QrErrorCode,
  type QrResult,
} from "./tools/qr";
import type {
  ApiHandoffDispatch,
  DiffHunk,
  KnowledgeDraftHandoffDispatch,
  OpenRequest,
  RegexMatch,
  ToolboxTextHandoffPreview,
  ToolboxTextRenewResult,
} from "./types";

export const API_HANDOFF_BROWSER_ERROR =
  "API Playground handoff는 데스크톱 앱에서만 사용할 수 있습니다. 클립보드로 자동 전환하지 않습니다";

export const TOOLBOX_TEXT_HANDOFF_KIND = "toolbox-text/v1";
export const TOOLBOX_TEXT_BROWSER_ERROR =
  "Developer Toolbox handoff는 데스크톱 앱에서만 사용할 수 있습니다. 클립보드로 자동 전환하지 않습니다";
export const TOOLBOX_TEXT_INVALID_ERROR = "텍스트 handoff 응답을 사용할 수 없습니다";

export const KNOWLEDGE_DRAFT_BROWSER_ERROR =
  "Knowledge draft handoff는 데스크톱 앱에서만 사용할 수 있습니다. 클립보드로 자동 전환하지 않습니다";
export const KNOWLEDGE_DRAFT_INPUT_ERROR = "Knowledge draft로 전달할 텍스트가 유효하지 않습니다";
export const KNOWLEDGE_DRAFT_CREATE_ERROR =
  "Knowledge draft를 만들거나 전달하지 못했습니다. 클립보드로 자동 전환하지 않습니다";
export const KNOWLEDGE_DRAFT_TARGET_UNAVAILABLE_ERROR =
  "Knowledge를 사용할 수 없습니다. 설치 또는 업데이트 후 다시 시도하세요. 클립보드로 자동 전환하지 않습니다";
export const KNOWLEDGE_DRAFT_INVALID_ERROR = "Knowledge draft 응답을 사용할 수 없습니다";

const HANDOFF_ID_PATTERN = /^[0-9a-f]{32}$/u;
const TOOLBOX_TEXT_MAX_BYTES = 512 * 1024;
const TOOLBOX_TEXT_MAX_CHARS = 256_000;
const TOOLBOX_TEXT_ALLOWED_PRODUCERS = new Set([
  "api-playground",
  "devbox-launcher",
  "log-lens",
]);

/** 데이터를 해시한다. browser 미리보기에서는 Web Crypto(SHA)만 지원. */
export async function hash(data: string, algorithm: string): Promise<string> {
  if (!isTauri()) return browserHash(data, algorithm);
  return invoke<string>("hash", { data, algorithm });
}

/** Generates an HMAC without network, persistence, or secret-bearing logs. */
export async function hmacGenerate(request: HmacRequest): Promise<string> {
  if (!isTauri()) return browserHmacGenerate(request);
  return invoke<string>("hmac_generate", { request });
}

/** Verifies an HMAC and returns only the boolean result. */
export async function hmacVerify(request: HmacVerifyRequest): Promise<boolean> {
  if (!isTauri()) return browserHmacVerify(request);
  return invoke<boolean>("hmac_verify", { request });
}

/** Verify a parsed JWT without returning its key, signature, or calculated tag. */
export async function verifyJwt(request: JwtVerifyRequest): Promise<boolean> {
  if (!isTauri()) return browserVerifyJwt(request);
  return invoke<boolean>("jwt_verify", { request });
}

/** Native QR generation is primary; browser preview uses the same bounded contract. */
export async function generateQr(request: GenerateQrRequest): Promise<QrResult> {
  if (!isTauri()) return generateBrowserQr(request);
  try {
    return await invoke<QrResult>("generate_qr", { request });
  } catch (error) {
    throw normalizeQrError(error);
  }
}

function normalizeQrError(error: unknown): QrGenerationError {
  if (error instanceof QrGenerationError) return error;
  if (typeof error === "string") {
    for (const [code, message] of Object.entries(QR_ERROR_MESSAGES) as Array<[QrErrorCode, string]>) {
      if (error === message) return new QrGenerationError(code);
    }
  }
  return new QrGenerationError("render");
}

/** UUID v4/v7 또는 ULID를 제한된 수량으로 생성한다. */
export async function generateIds(options: IdentifierOptions): Promise<string[]> {
  if (!isTauri()) return generateIdentifiers(options);
  return invoke<string[]>("generate_ids", { request: options });
}

/** 기존 UUID v4 호출과의 호환을 유지한다. */
export async function generateUuid(): Promise<string> {
  if (!isTauri()) {
    return generateIdentifiers({
      kind: "uuid-v4",
      count: 1,
      uppercase: false,
      hyphens: true,
    })[0];
  }
  return invoke<string>("generate_uuid");
}

/** 정규식 전체 매치 목록을 반환한다. */
export async function regexTest(pattern: string, text: string): Promise<RegexMatch[]> {
  if (!isTauri()) return browserRegex(pattern, text);
  return invoke<RegexMatch[]>("regex_test", { pattern, text });
}

/** 두 텍스트의 라인 단위 변경 구간을 반환한다. */
export async function diff(a: string, b: string): Promise<DiffHunk[]> {
  if (!isTauri()) return browserDiff(a, b);
  return invoke<DiffHunk[]>("diff", { a, b });
}

/**
 * Paste처럼 사용자가 명시적으로 요청한 순간에만 plain text clipboard를 읽는다.
 * Browser preview에서는 표준 Clipboard API를 사용하고 Tauri에서는 명시적으로 허용한
 * read-text command만 호출한다.
 */
export async function readClipboardText(): Promise<string> {
  if (!isTauri()) return navigator.clipboard.readText();
  return readText();
}

/** Publish only the explicit output currently shown by a tool. */
export async function createApiRequestHandoff(output: string): Promise<ApiHandoffDispatch> {
  if (!isTauri()) throw new Error(API_HANDOFF_BROWSER_ERROR);
  return invoke<ApiHandoffDispatch>("create_api_request_handoff", { output });
}

const KNOWLEDGE_DRAFT_FIXED_ERRORS = new Set([
  KNOWLEDGE_DRAFT_INPUT_ERROR,
  KNOWLEDGE_DRAFT_CREATE_ERROR,
  KNOWLEDGE_DRAFT_TARGET_UNAVAILABLE_ERROR,
  KNOWLEDGE_DRAFT_BROWSER_ERROR,
]);

function safeKnowledgeDraftError(cause: unknown): Error {
  const raw = cause instanceof Error ? cause.message : typeof cause === "string" ? cause : "";
  const message = raw.replace(/^Error:\s*/u, "");
  return new Error(KNOWLEDGE_DRAFT_FIXED_ERRORS.has(message)
    ? message
    : KNOWLEDGE_DRAFT_CREATE_ERROR);
}

function parseKnowledgeDraftDispatch(value: unknown): KnowledgeDraftHandoffDispatch {
  if (!isRecord(value)
    || Object.keys(value).some((key) => !["handoffId", "redacted"].includes(key))
    || typeof value.handoffId !== "string"
    || !HANDOFF_ID_PATTERN.test(value.handoffId)
    || typeof value.redacted !== "boolean") {
    throw new Error(KNOWLEDGE_DRAFT_INVALID_ERROR);
  }
  return {
    handoffId: value.handoffId,
    redacted: value.redacted,
  };
}

/** Publish an explicit bounded output as a one-time Knowledge draft handoff. */
export async function createKnowledgeDraftHandoff(
  output: string,
): Promise<KnowledgeDraftHandoffDispatch> {
  if (!isTauri()) throw new Error(KNOWLEDGE_DRAFT_BROWSER_ERROR);
  if (typeof output !== "string" || !isBoundedToolboxText(output)) {
    throw new Error(KNOWLEDGE_DRAFT_INPUT_ERROR);
  }
  let response: unknown;
  try {
    response = await invoke<unknown>("create_knowledge_draft_handoff", { output });
  } catch (cause) {
    throw safeKnowledgeDraftError(cause);
  }
  return parseKnowledgeDraftDispatch(response);
}

const isRecord = (value: unknown): value is Record<string, unknown> => (
  typeof value === "object" && value !== null
);

function parseOpenRequest(value: unknown): OpenRequest | null {
  if (!isRecord(value) || !isRecord(value.target)) return null;
  const from = value.from;
  if (from !== null && from !== undefined && typeof from !== "string") return null;
  const target = value.target;
  switch (target.kind) {
    case "path":
      if (typeof target.path !== "string") return null;
      return {
        target: {
          kind: "path",
          path: target.path,
          line: typeof target.line === "number" ? target.line : null,
          column: typeof target.column === "number" ? target.column : null,
        },
        from: typeof from === "string" ? from : null,
      };
    case "profile":
      if (typeof target.id !== "string") return null;
      return {
        target: { kind: "profile", id: target.id },
        from: typeof from === "string" ? from : null,
      };
    case "workspace":
      if (typeof target.path !== "string") return null;
      return {
        target: { kind: "workspace", path: target.path },
        from: typeof from === "string" ? from : null,
      };
    case "task":
      if (typeof target.id !== "string") return null;
      return {
        target: { kind: "task", id: target.id },
        from: typeof from === "string" ? from : null,
      };
    case "query":
      if (typeof target.text !== "string") return null;
      return {
        target: { kind: "query", text: target.text, filter: target.filter },
        from: typeof from === "string" ? from : null,
      };
    case "install":
      if (typeof target.appId !== "string") return null;
      return {
        target: { kind: "install", appId: target.appId },
        from: typeof from === "string" ? from : null,
      };
    case "handoff":
      if (
        typeof target.handoffKind !== "string"
        || typeof target.id !== "string"
        || !HANDOFF_ID_PATTERN.test(target.id)
      ) return null;
      return {
        target: { kind: "handoff", handoffKind: target.handoffKind, id: target.id },
        from: typeof from === "string" ? from : null,
      };
    default:
      return null;
  }
}

/** Takes the one-shot cold-start request left by the native AppLink shell. */
export async function takePendingOpen(): Promise<OpenRequest | null> {
  if (!isTauri()) return null;
  return parseOpenRequest(await invoke<unknown>("take_pending_open"));
}

/**
 * Registers only a wake-up listener.  The event payload is intentionally
 * ignored; callers must take the native pending slot to obtain the request.
 */
export async function onOpenRequest(handler: () => void): Promise<UnlistenFn> {
  if (!isTauri()) return () => undefined;
  return listen<unknown>("devbox://open", () => handler());
}

function utf8ByteLength(value: string): number {
  return new TextEncoder().encode(value).length;
}

function hasWellFormedText(value: string): boolean {
  for (let index = 0; index < value.length; index += 1) {
    const code = value.charCodeAt(index);
    if (code >= 0xd800 && code <= 0xdbff) {
      const next = value.charCodeAt(index + 1);
      if (next < 0xdc00 || next > 0xdfff) return false;
      index += 1;
    } else if (code >= 0xdc00 && code <= 0xdfff) {
      return false;
    }
  }
  return true;
}

function isBoundedToolboxText(value: string): boolean {
  return value.trim().length > 0
    && value.length <= TOOLBOX_TEXT_MAX_CHARS * 2
    && Array.from(value).length <= TOOLBOX_TEXT_MAX_CHARS
    && utf8ByteLength(value) <= TOOLBOX_TEXT_MAX_BYTES
    && !value.includes("\0")
    && hasWellFormedText(value)
    && !Array.from(value).some((character) => {
      const code = character.charCodeAt(0);
      return (code < 0x20 && ![0x09, 0x0a, 0x0d].includes(code))
        || (code >= 0x7f && code <= 0x9f);
    });
}

function parseToolboxTextPreview(value: unknown, requestedId: string): ToolboxTextHandoffPreview {
  const expiresAtMs = isRecord(value) ? value.expiresAtMs : undefined;
  if (!isRecord(value)
    || Object.keys(value).some((key) => !["handoffId", "producerId", "expiresAtMs", "text", "redacted"].includes(key))
    || typeof value.handoffId !== "string"
    || value.handoffId !== requestedId
    || !HANDOFF_ID_PATTERN.test(value.handoffId)
    || typeof value.producerId !== "string"
    || !TOOLBOX_TEXT_ALLOWED_PRODUCERS.has(value.producerId)
    || !Number.isSafeInteger(expiresAtMs)
    || (expiresAtMs as number) <= 0
    || typeof value.text !== "string"
    || !isBoundedToolboxText(value.text)
    || typeof value.redacted !== "boolean") {
    throw new Error(TOOLBOX_TEXT_INVALID_ERROR);
  }
  const safeExpiresAtMs = expiresAtMs as number;
  return {
    handoffId: value.handoffId,
    producerId: value.producerId,
    expiresAtMs: safeExpiresAtMs,
    text: value.text,
    redacted: value.redacted,
  };
}

function assertToolboxTextId(handoffId: string): void {
  if (!HANDOFF_ID_PATTERN.test(handoffId)) throw new Error(TOOLBOX_TEXT_INVALID_ERROR);
}

/** Claims and validates one `toolbox-text/v1` handoff for explicit preview. */
export async function previewToolboxText(handoffId: string): Promise<ToolboxTextHandoffPreview> {
  if (!isTauri()) throw new Error(TOOLBOX_TEXT_BROWSER_ERROR);
  assertToolboxTextId(handoffId);
  const response = await invoke<unknown>("preview_toolbox_text", { handoffId });
  return parseToolboxTextPreview(response, handoffId);
}

/** Renews only the process-local claim lease; envelope expiry is unchanged. */
export async function renewToolboxText(handoffId: string): Promise<ToolboxTextRenewResult> {
  if (!isTauri()) throw new Error(TOOLBOX_TEXT_BROWSER_ERROR);
  assertToolboxTextId(handoffId);
  const response = await invoke<unknown>("renew_toolbox_text", { handoffId });
  const leaseUntilMs = isRecord(response) ? response.leaseUntilMs : undefined;
  if (!isRecord(response)
    || Object.keys(response).some((key) => key !== "leaseUntilMs")
    || !Number.isSafeInteger(leaseUntilMs)
    || (leaseUntilMs as number) <= 0) {
    throw new Error(TOOLBOX_TEXT_INVALID_ERROR);
  }
  return { leaseUntilMs: leaseUntilMs as number };
}

/** Acknowledges the preview and returns the bounded text for renderer memory. */
export async function acceptToolboxText(handoffId: string): Promise<string> {
  if (!isTauri()) throw new Error(TOOLBOX_TEXT_BROWSER_ERROR);
  assertToolboxTextId(handoffId);
  const response = await invoke<unknown>("accept_toolbox_text", { handoffId });
  if (typeof response !== "string" || !isBoundedToolboxText(response)) {
    throw new Error(TOOLBOX_TEXT_INVALID_ERROR);
  }
  return response;
}

/** Restores a cancelled preview to the pending handoff queue. */
export async function discardToolboxText(handoffId: string): Promise<void> {
  if (!isTauri()) throw new Error(TOOLBOX_TEXT_BROWSER_ERROR);
  assertToolboxTextId(handoffId);
  await invoke<void>("discard_toolbox_text", { handoffId });
}

async function browserHash(data: string, algorithm: string): Promise<string> {
  if (algorithm.toLowerCase() === "md5") {
    throw new Error("MD5는 브라우저 미리보기에서 지원되지 않습니다. Tauri 앱에서 사용하세요.");
  }
  const buf = await crypto.subtle.digest(
    algorithm.toLowerCase() === "sha512" ? "SHA-512" : "SHA-256",
    new TextEncoder().encode(data),
  );
  return Array.from(new Uint8Array(buf))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

function browserRegex(pattern: string, text: string): RegexMatch[] {
  const re = new RegExp(pattern, "g");
  const out: RegexMatch[] = [];
  for (const m of text.matchAll(re)) {
    out.push({ start: m.index, end: m.index + m[0].length, text: m[0] });
  }
  return out;
}

/** 단순 라인 diff: 공통 접두/접미를 찾아 중간을 replace로 표시. */
function browserDiff(a: string, b: string): DiffHunk[] {
  const al = a.split("\n");
  const bl = b.split("\n");
  let start = 0;
  while (start < al.length && start < bl.length && al[start] === bl[start]) start++;
  let aEnd = al.length;
  let bEnd = bl.length;
  while (aEnd > start && bEnd > start && al[aEnd - 1] === bl[bEnd - 1]) {
    aEnd--;
    bEnd--;
  }
  const hunks: DiffHunk[] = [];
  if (start > 0) hunks.push({ kind: 0, old_start: 0, old_end: start, new_start: 0, new_end: start });
  if (aEnd > start || bEnd > start) {
    hunks.push({ kind: 2, old_start: start, old_end: aEnd, new_start: start, new_end: bEnd });
  }
  if (aEnd < al.length) hunks.push({ kind: 0, old_start: aEnd, old_end: al.length, new_start: bEnd, new_end: bl.length });
  return hunks;
}
