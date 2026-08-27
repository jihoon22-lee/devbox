import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { isTauri } from "./lib/isTauri";
import { applyToRequest, type EnvVariable } from "./lib/environments";
import {
  buildCookieHeader,
  hasCookieSourceConflict,
  isCookieEnabled,
  validateCookies,
} from "./lib/cookies";
import { isHeaderEnabled } from "./lib/headers";
import {
  buildGraphqlBody,
  buildGraphqlGetUrl,
  extractGraphqlCredentialLiterals,
  isGraphqlDerivedHeader,
  projectGraphqlResponse,
  parseGraphqlVariables,
  resolveGraphqlRequest,
  validateGraphqlEndpoint,
  validateGraphqlHeaders,
} from "./lib/graphql";
import {
  SseEventBuffer,
  SseParseError,
  SseParser,
  MAX_DECODED_BYTES,
  MAX_EVENT_DATA_BYTES,
  MAX_EVENT_ID_BYTES,
  MAX_EVENT_NAME_BYTES,
  MAX_RETRY_MS,
} from "./lib/sse";
import {
  isMultipartPartEnabled,
  isMultipartDerivedHeader,
  safeMultipartFileName,
  validateMultipartParts,
  type PickedMultipartFile,
} from "./lib/multipart";
import {
  buildWebSocketUrl,
  encodeBase64,
  hexToBytes,
  makeBinaryMessage,
  makeTextMessage,
  MAX_BINARY_PREVIEW_BYTES,
  MAX_CLOSE_REASON_BYTES,
  MAX_CONTROL_PAYLOAD_BYTES,
  MAX_MESSAGE_BYTES,
  MAX_TEXT_PREVIEW_BYTES,
  MESSAGE_TOO_LARGE,
  WebSocketMessageBuffer,
  toNativeMessageInput,
  textToBytes,
  utf8Bytes,
  utf8Truncate,
  validateCloseCode,
  validateCloseReason,
  validateWebSocketRequest,
} from "./lib/websocket";
import type {
  WebSocketConnectionState,
  WebSocketMessage,
  WebSocketMessageInput,
  WebSocketUpdate,
} from "./types";
import type {
  ApiRequestHandoffPreview,
  ApiResponse,
  OpenRequest,
  RequestTemplate,
  SseOptions,
  SseUpdate,
} from "./types";

const SSE_EVENT = "api-playground/sse";
const MAX_SSE_URL_BYTES = 8 * 1024;
const MAX_SSE_HEADERS = 100;
const MAX_SSE_PARAMS = 100;
const MAX_SSE_ENVIRONMENT_VARIABLES = 100;
const MAX_SSE_BODY_BYTES = 4 * 1024 * 1024;
const MAX_SSE_RECONNECT_ATTEMPTS = 5;
const MIN_SSE_RETRY_MS = 250;
const DEFAULT_SSE_RETRY_MS = 1_000;
const MIN_SSE_CONNECT_TIMEOUT_MS = 100;
const MAX_SSE_CONNECT_TIMEOUT_MS = 30_000;
const MIN_SSE_IDLE_TIMEOUT_MS = 100;
const MAX_SSE_IDLE_TIMEOUT_MS = 300_000;
const MIN_SSE_TOTAL_TIMEOUT_MS = 1_000;
const MAX_SSE_TOTAL_TIMEOUT_MS = 3_600_000;

const MAX_RESPONSE_HEADERS = 100;
const MAX_RESPONSE_HEADER_BYTES = 64 * 1024;
let nativeRequestSequence = 0;

const HANDOFF_BROWSER_ERROR =
  "API Playground handoff는 데스크톱 앱에서만 사용할 수 있습니다. 클립보드로 자동 전환하지 않습니다";

function nextNativeRequestId(): string {
  nativeRequestSequence = (nativeRequestSequence + 1) % Number.MAX_SAFE_INTEGER;
  const randomId = globalThis.crypto?.randomUUID?.().replace(/-/g, "");
  return randomId
    ? `request-${randomId}`
    : `request-${Date.now().toString(36)}-${nativeRequestSequence.toString(36)}`;
}

export interface RemoteOpenApiSource {
  text: string;
  format: "json" | "yaml";
}

/** URL 문서를 native bounded fetch 경계에서 읽는다. URL 원문은 결과나 오류에 포함하지 않는다. */
export async function fetchOpenApiSource(url: string): Promise<RemoteOpenApiSource> {
  if (!isTauri()) throw new Error("URL 가져오기는 데스크톱 앱에서만 사용할 수 있습니다");
  return invoke<RemoteOpenApiSource>("fetch_openapi_source", { url });
}

const SAFE_SSE_UPDATE_MESSAGES = new Set([
  "SSE 요청을 보낼 수 없습니다",
  "SSE stream 시간이 초과되었습니다",
  "SSE stream 연결에 실패했습니다",
  "SSE 응답 형식이 아닙니다",
  "SSE 리다이렉트 정책으로 요청을 차단했습니다",
  "SSE stream 데이터가 올바르지 않습니다",
]);

function utf8ByteLength(value: string): number {
  return new TextEncoder().encode(value).byteLength;
}

/** HTTP 요청 전송. 브라우저 미리보기에서는 fetch(CORS 제약 존재)로 대체한다. */
export async function sendRequest(
  req: RequestTemplate,
  environment: EnvVariable[],
  signal?: AbortSignal,
): Promise<ApiResponse> {
  if (!isTauri()) return browserFetch(req, environment, signal);
  if (signal?.aborted) throw new Error("요청이 취소되었습니다");
  const requestId = nextNativeRequestId();
  const onAbort = () => { void cancelRequest(requestId); };
  signal?.addEventListener("abort", onAbort, { once: true });
  try {
    return await invoke<ApiResponse>("send_request", { req, environment, requestId });
  } finally {
    signal?.removeEventListener("abort", onAbort);
  }
}

async function cancelRequest(requestId: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("cancel_request", { requestId });
}

/** 값을 봉인해 base64 blob을 반환한다. */
export async function sealSecret(value: string): Promise<string> {
  if (!isTauri()) throw new Error("secret 봉인은 데스크톱 앱에서만 사용할 수 있습니다");
  return invoke<string>("seal_secret", { value });
}

/** 저장 후보를 backend secret 경계에서 한 번 더 정화한다. */
export async function sanitizePersistedJson(
  serialized: string,
  environment: EnvVariable[],
): Promise<string> {
  if (!isTauri()) {
    if (environment.some((variable) => variable.secret)) {
      throw new Error("secret 검증은 데스크톱 앱에서만 사용할 수 있습니다");
    }
    return serialized;
  }
  return invoke<string>("sanitize_persisted_json", { serialized, environment });
}

/** 확인 뒤 한 번만 원문 cURL을 만들어 반환한다. 호출자는 즉시 사용하고 저장하지 않는다. */
export async function buildRevealedCurl(
  req: RequestTemplate,
  environment: EnvVariable[],
): Promise<string> {
  if (!isTauri()) throw new Error("원문 cURL 복사는 데스크톱 앱에서만 사용할 수 있습니다");
  return invoke<string>("build_revealed_curl", { req, environment });
}

/** 확인된 현재 응답의 원문 header를 backend 메모리에서 한 번만 가져온다. */
export async function copyRawResponseHeaders(responseId: string): Promise<string> {
  if (!isTauri()) throw new Error("원문 응답 header 복사는 데스크톱 앱에서만 사용할 수 있습니다");
  return invoke<string>("copy_raw_response_headers", { responseId });
}

/** 확인된 현재 응답의 원문 Set-Cookie만 backend 메모리에서 한 번 가져온다. */
export async function copyRawResponseCookies(responseId: string): Promise<string> {
  if (!isTauri()) throw new Error("원문 응답 Cookie 복사는 데스크톱 앱에서만 사용할 수 있습니다");
  return invoke<string>("copy_raw_response_cookies", { responseId });
}

/** 데스크톱 file picker의 사용자 선택 결과만 runtime multipart 경로로 반환한다. */
export async function pickMultipartFile(): Promise<PickedMultipartFile | null> {
  if (!isTauri()) throw new Error("파일 선택은 데스크톱 앱에서만 사용할 수 있습니다");
  const selected = await open({
    directory: false,
    multiple: false,
    title: "multipart 파일 선택",
  });
  if (typeof selected !== "string") return null;
  return { path: selected, name: safeMultipartFileName(selected) };
}

/** Takes the one-shot cold/hot AppLink request stored by the native shell. */
export async function takePendingOpen(): Promise<OpenRequest | null> {
  if (!isTauri()) return null;
  return invoke<OpenRequest | null>("take_pending_open");
}

/** Registers the wake-up listener used by the native single-instance plugin. */
export async function onOpenRequest(cb: (request: OpenRequest) => void): Promise<UnlistenFn> {
  if (!isTauri()) return () => undefined;
  return listen<OpenRequest>("devbox://open", (event) => cb(event.payload));
}

/** Claim and validate a pending `api-request/v1` handoff for preview. */
export async function claimApiRequest(handoffId: string): Promise<ApiRequestHandoffPreview> {
  if (!isTauri()) throw new Error(HANDOFF_BROWSER_ERROR);
  return invoke<ApiRequestHandoffPreview>("claim_api_request", { handoffId });
}

/** Acknowledge an applied preview and return its editable request template. */
export async function ackApiRequest(handoffId: string): Promise<RequestTemplate> {
  if (!isTauri()) throw new Error(HANDOFF_BROWSER_ERROR);
  return invoke<RequestTemplate>("ack_api_request", { handoffId });
}

/** Return a cancelled preview to the shared pending queue. */
export async function restoreApiRequest(handoffId: string): Promise<void> {
  if (!isTauri()) throw new Error(HANDOFF_BROWSER_ERROR);
  return invoke<void>("restore_api_request", { handoffId });
}

async function browserFetch(req: RequestTemplate, environment: EnvVariable[], signal?: AbortSignal): Promise<ApiResponse> {
  if (environment.some((variable) => variable.secret)) {
    throw new Error("secret 포함 요청은 데스크톱 앱에서만 전송할 수 있습니다");
  }
  const variables = new Map(environment.map((variable) => [variable.key, variable.value]));
  let resolved = { ...applyToRequest(req, variables), method: req.method.trim().toUpperCase() };
  const graphql = resolved.body_kind === "graphql" && resolved.graphql
    ? resolveGraphqlRequest(resolved.graphql, variables)
    : null;
  if (resolved.body_kind === "graphql") {
    if (!graphql) throw new Error("GraphQL 요청 구성이 올바르지 않습니다");
    validateGraphqlEndpoint(resolved.url);
    validateGraphqlHeaders(resolved.headers);
    if (resolved.method !== "GET" && resolved.method !== "POST") {
      throw new Error("GraphQL 요청 구성이 올바르지 않습니다");
    }
    // Keep the resolved document only in this in-flight call; it is not persisted.
    resolved = { ...resolved, graphql, body: "" };
    buildGraphqlBody(graphql);
  }
  if (validateCookies(resolved.cookies).length > 0) {
    throw new Error("Cookie 이름 또는 값이 올바르지 않습니다");
  }
  if (hasCookieSourceConflict(resolved.cookies, resolved.headers)) {
    throw new Error("Cookie header와 구조화 Cookie를 동시에 전송할 수 없습니다");
  }
  if (resolved.body_kind === "multipart") {
    const issue = validateMultipartParts(resolved.multipart)[0];
    if (issue) throw new Error(issue.message);
    if (resolved.multipart.some((part) =>
      isMultipartPartEnabled(part) && part.kind === "file" && Boolean(part.name || part.file_name),
    )) {
      throw new Error("multipart 파일 전송은 데스크톱 앱에서만 사용할 수 있습니다");
    }
    if (resolved.multipart.some((part) =>
      isMultipartPartEnabled(part) && part.kind === "text" && Boolean(part.content_type),
    )) {
      throw new Error("part별 Content-Type 전송은 데스크톱 앱에서만 사용할 수 있습니다");
    }
  }
  const start = performance.now();
  const headers = new Headers();
  for (const header of resolved.headers) {
    if (
      isHeaderEnabled(header) &&
      header.key &&
      !(resolved.body_kind === "multipart" && isMultipartDerivedHeader(header.key)) &&
      !(resolved.body_kind === "graphql" && isGraphqlDerivedHeader(header.key))
    ) {
      headers.append(header.key, header.value);
    }
  }
  const cookieHeader = buildCookieHeader(resolved.cookies);
  if (cookieHeader) headers.append("Cookie", cookieHeader);
  if (resolved.auth?.kind === "basic") {
    headers.append("Authorization", "Basic " + btoa(`${resolved.auth.username}:${resolved.auth.password}`));
  } else if (resolved.auth?.kind === "bearer") {
    headers.append("Authorization", "Bearer " + resolved.auth.token);
  } else if (resolved.auth?.kind === "apikey" && resolved.auth.api_key) {
    headers.append(resolved.auth.api_key, resolved.auth.api_value);
  }
  const params = new URLSearchParams();
  for (const p of resolved.params) if (p.key) params.append(p.key, p.value);
  const url = resolved.body_kind === "graphql" && graphql && resolved.method === "GET"
    ? buildGraphqlGetUrl(resolved.url, resolved.params, graphql)
    : (() => {
      const sep = resolved.url.includes("?") ? "&" : "?";
      return params.size ? resolved.url + sep + params.toString() : resolved.url;
    })();
  if (resolved.body_kind === "graphql") validateGraphqlEndpoint(url);

  let body: BodyInit | undefined;
  if (resolved.body_kind === "graphql" && graphql && resolved.method === "POST") {
    headers.set("Content-Type", "application/json");
    body = buildGraphqlBody(graphql);
  } else if (resolved.body_kind === "json" && resolved.body.trim()) {
    headers.set("Content-Type", "application/json");
    body = resolved.body;
  } else if (resolved.body_kind === "raw" && resolved.body) {
    body = resolved.body;
  } else if (resolved.body_kind === "multipart") {
    const form = new FormData();
    for (const part of resolved.multipart) {
      if (isMultipartPartEnabled(part) && part.kind === "text" && part.name) {
        form.append(part.name, part.value);
      }
    }
    body = form;
  }

  const resp = await fetch(url, {
    method: resolved.method,
    headers,
    body,
    signal,
    // Browser preview cannot inspect and re-apply native redirect policy safely.
    // Keep GraphQL redirects at the browser boundary instead of forwarding auth.
    redirect: resolved.body_kind === "graphql" ? "manual" : "follow",
  });
  if (resolved.body_kind === "graphql" && resp.type === "opaqueredirect") {
    throw new Error("GraphQL 리다이렉트를 브라우저 미리보기에서 처리할 수 없습니다");
  }
  const duration_ms = Math.round(performance.now() - start);
  const text = await readResponseText(
    resp,
    resolved.body_kind === "graphql" ? 4 * 1024 * 1024 : Number.MAX_SAFE_INTEGER,
  );
  const maskedBody = redactBrowserText(text, resolved);
  const respHeaders: { key: string; value: string }[] = [];
  let responseHeaderBytes = 0;
  let headersTruncated = false;
  const encoder = new TextEncoder();
  resp.headers.forEach((v, k) => {
    if (headersTruncated) return;
    const lineBytes = encoder.encode(k).byteLength + encoder.encode(v).byteLength + 2;
    if (respHeaders.length >= MAX_RESPONSE_HEADERS
      || responseHeaderBytes + lineBytes > MAX_RESPONSE_HEADER_BYTES) {
      headersTruncated = true;
      return;
    }
    responseHeaderBytes += lineBytes;
    respHeaders.push({ key: k, value: isSensitiveName(k) ? "[REDACTED]" : v });
  });

  return {
    status: resp.status,
    status_text: resp.statusText,
    headers: respHeaders,
    duration_ms,
    size_bytes: new TextEncoder().encode(text).byteLength,
    body: maskedBody,
    is_json: (resp.headers.get("content-type") ?? "").includes("json"),
    final_url: redactUrl(resp.url, resolved.body_kind === "graphql"),
    redirects: [],
    cookies: [],
    response_id: null,
    raw_headers_available: false,
    headers_truncated: headersTruncated,
    ...(resolved.body_kind === "graphql" ? { graphql: projectGraphqlResponse(maskedBody) } : {}),
  };
}

async function readResponseText(response: Response, maxBytes: number): Promise<string> {
  if (!response.body) {
    const output = await response.text();
    if (new TextEncoder().encode(output).byteLength > maxBytes) {
      throw new Error("응답 본문이 허용된 크기를 초과했습니다");
    }
    return output;
  }
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let total = 0;
  let output = "";
  try {
    while (true) {
      const next = await reader.read();
      if (next.done) {
        output += decoder.decode();
        return output;
      }
      total += next.value.byteLength;
      if (total > maxBytes) throw new Error("응답 본문이 허용된 크기를 초과했습니다");
      output += decoder.decode(next.value, { stream: true });
    }
  } catch (error) {
    await reader.cancel().catch(() => undefined);
    throw error;
  } finally {
    reader.releaseLock();
  }
}

function isSensitiveName(name: string): boolean {
  return /(authorization|cookie|set[-_]?cookie|api[-_]?key|api[-_]?value|token|secret|password|passwd|private[-_]?key|username)/i.test(name);
}

function redactUrl(value: string, maskGraphql = false): string {
  try {
    const url = new URL(value);
    for (const key of [...url.searchParams.keys()]) {
      if (isSensitiveName(key) || (maskGraphql && ["query", "variables", "operationName"].includes(key))) {
        url.searchParams.set(key, "[REDACTED]");
      }
    }
    if (url.username) url.username = "REDACTED";
    if (url.password) url.password = "REDACTED";
    return url.toString();
  } catch {
    return value;
  }
}

function redactBrowserText(text: string, req: RequestTemplate): string {
  const directSecrets = [
    req.auth?.username,
    req.auth?.password,
    req.auth?.token,
    req.auth?.api_value,
    ...req.headers
      .filter((header) => isHeaderEnabled(header) && isSensitiveName(header.key))
      .map((header) => header.value),
    ...req.cookies
      .filter((cookie) => isCookieEnabled(cookie))
      .map((cookie) => cookie.value),
    ...req.params.filter((param) => isSensitiveName(param.key)).map((param) => param.value),
    ...req.multipart
      .filter((part) =>
        isMultipartPartEnabled(part) && part.kind === "text" && isSensitiveName(part.name),
      )
      .map((part) => part.value),
    ...(req.body_kind === "graphql" && req.graphql ? graphqlSecrets(req.graphql) : []),
  ].filter((value): value is string => Boolean(value));
  try {
    const url = new URL(req.url);
    for (const [key, value] of url.searchParams) {
      if (isSensitiveName(key) && value) directSecrets.push(value);
    }
  } catch {
    // The request boundary reports a fixed URL error; redaction itself never reflects it.
  }
  const exactRedacted = directSecrets.sort((a, b) => b.length - a.length).reduce(
    (result, secret) => result.split(secret).join("[REDACTED]"),
    text,
  );
  try {
    return redactBrowserTokens(JSON.stringify(redactBrowserJson(JSON.parse(exactRedacted) as unknown)));
  } catch {
    return redactBrowserTokens(exactRedacted.replace(
      /((?:authorization|cookie|set[-_]?cookie|api[-_]?key|api[-_]?value|token|secret|password|passwd|private[-_]?key|username)\s*[=:]\s*)([^\s,;&]+)/gi,
      "$1[REDACTED]",
    ));
  }
}

function graphqlSecrets(request: NonNullable<RequestTemplate["graphql"]>): string[] {
  const values = extractGraphqlCredentialLiterals(request.query);
  try {
    const variables = parseGraphqlVariables(request.variables);
    const visit = (value: unknown, key = "") => {
      // Match the native redactor: variable values are secrets when their
      // variable name is credential-shaped, while ordinary values (for
      // example an echoed id) remain useful in a response preview.
      if (typeof value === "string" && value && isSensitiveName(key)) values.push(value);
      else if (Array.isArray(value)) value.forEach((item) => visit(item, key));
      else if (value && typeof value === "object") {
        Object.entries(value).forEach(([childKey, child]) => visit(child, childKey));
      }
    };
    visit(variables);
  } catch {
    // The request builder reports malformed variables before fetch. Do not echo them
    // from a later browser error path.
  }
  return values;
}

function redactBrowserTokens(value: string): string {
  return value.replace(
    /(?:sk-|ghp_|github_pat_|glpat-|xox[bprsa]-)[A-Za-z0-9_\-]{12,}/g,
    "[REDACTED]",
  );
}

function redactBrowserJson(value: unknown, key = ""): unknown {
  if (isSensitiveName(key)) return "[REDACTED]";
  if (Array.isArray(value)) return value.map((item) => redactBrowserJson(item));
  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value as Record<string, unknown>).map(([childKey, child]) => [
        childKey,
        redactBrowserJson(child, childKey),
      ]),
    );
  }
  return value;
}

export interface SseStreamHandle {
  sessionId: string;
  stop: () => Promise<void>;
}

/**
 * Start one bounded SSE stream.  The callback receives only a validated, masked event envelope;
 * callers must not persist it automatically.  Desktop uses the native task/event bridge and the
 * browser preview uses the same parser with Fetch/CORS limitations.
 */
export async function startSseStream(
  req: RequestTemplate,
  environment: EnvVariable[],
  options: SseOptions,
  onUpdate: (update: SseUpdate) => void,
): Promise<SseStreamHandle> {
  validateSseOptions(options);
  validateSseEnvironment(environment);
  if (isTauri()) return startNativeSseStream(req, environment, options, onUpdate);
  return startBrowserSseStream(req, environment, options, onUpdate);
}

async function startNativeSseStream(
  req: RequestTemplate,
  environment: EnvVariable[],
  options: SseOptions,
  onUpdate: (update: SseUpdate) => void,
): Promise<SseStreamHandle> {
  const { listen } = await import("@tauri-apps/api/event");
  let sessionId: string | null = null;
  const pending: SseUpdate[] = [];
  const unlisten = await listen<unknown>(SSE_EVENT, (event) => {
    const update = parseSseUpdate(event.payload);
    if (!update) return;
    if (!sessionId) {
      if (pending.length < 64) pending.push(update);
      return;
    }
    if (update.sessionId === sessionId) onUpdate(update);
  });
  let started: string;
  try {
    started = await invoke<string>("start_sse_stream", { req, environment, options });
    if (!isSseSessionId(started)) throw new Error("invalid session");
    sessionId = started;
    for (const update of pending.splice(0)) {
      if (update.sessionId === started) onUpdate(update);
    }
  } catch (cause) {
    await unlisten();
    throw new Error(safeSseStartError(cause));
  }

  const activeSessionId = started;
  let stopped = false;
  return {
    sessionId: activeSessionId,
    stop: async () => {
      if (stopped) return;
      stopped = true;
      try {
        await invoke("stop_sse_stream", { sessionId: activeSessionId });
      } catch {
        throw new Error("SSE stream을 중지하지 못했습니다.");
      } finally {
        await unlisten();
      }
    },
  };
}

async function startBrowserSseStream(
  req: RequestTemplate,
  environment: EnvVariable[],
  options: SseOptions,
  onUpdate: (update: SseUpdate) => void,
): Promise<SseStreamHandle> {
  if (environment.some((variable) => variable.secret)) {
    throw new Error("secret 포함 SSE stream은 데스크톱 앱에서만 사용할 수 있습니다.");
  }
  const variables = new Map(environment.map((variable) => [variable.key, variable.value]));
  const resolved = {
    ...applyToRequest(req, variables),
    method: req.method.trim().toUpperCase(),
  };
  validateBrowserSseRequest(resolved);
  const controller = new AbortController();
  const sessionId = `browser-sse-${++browserSseSequence}`;
  void runBrowserSse(resolved, options, sessionId, controller.signal, onUpdate);
  return {
    sessionId,
    stop: async () => {
      controller.abort();
    },
  };
}

let browserSseSequence = 0;

async function runBrowserSse(
  req: RequestTemplate,
  options: SseOptions,
  sessionId: string,
  signal: AbortSignal,
  onUpdate: (update: SseUpdate) => void,
): Promise<void> {
  const startedAt = performance.now();
  const deadline = startedAt + options.totalTimeoutMs;
  let attempts = 0;
  let retryMs = DEFAULT_SSE_RETRY_MS;
  let sequence = 0;
  let decodedBytes = 0;
  const history = new SseEventBuffer();

  while (!signal.aborted && performance.now() < deadline) {
    try {
      const response = await browserSseFetch(req, options, signal, deadline);
      onUpdate({ sessionId, kind: "connected", sequence, dropped: history.evicted, attempt: attempts });
      const parser = new SseParser();
      const reader = response.body?.getReader();
      if (!reader) throw new BrowserSseFailure("SSE 응답 형식이 아닙니다", false);
      let streamEnded = false;
      try {
        while (!signal.aborted) {
          const remaining = Math.max(1, deadline - performance.now());
          const result = await readWithTimeout(reader, Math.min(options.idleTimeoutMs, remaining));
          if (result.done) {
            streamEnded = true;
            break;
          }
          const chunk = result.value;
          decodedBytes += chunk.byteLength;
          if (decodedBytes > MAX_DECODED_BYTES) throw new BrowserSseFailure("SSE stream 데이터가 올바르지 않습니다", false);
          for (const event of parser.feed(chunk)) {
            sequence = emitBrowserEvent(event, req, sessionId, onUpdate, sequence, history);
            if (event.retryMs !== undefined) retryMs = event.retryMs;
          }
          if (parser.retryMs !== undefined) retryMs = parser.retryMs;
        }
        for (const event of parser.finish()) {
          sequence = emitBrowserEvent(event, req, sessionId, onUpdate, sequence, history);
          if (event.retryMs !== undefined) retryMs = event.retryMs;
        }
        if (parser.retryMs !== undefined) retryMs = parser.retryMs;
      } finally {
        if (!streamEnded) await reader.cancel().catch(() => undefined);
        reader.releaseLock();
      }

      if (!options.reconnect || attempts >= MAX_SSE_RECONNECT_ATTEMPTS || signal.aborted) {
        if (!signal.aborted) onUpdate({ sessionId, kind: "closed", sequence, dropped: history.evicted });
        return;
      }
    } catch (cause) {
      if (signal.aborted) return;
      const failure = cause instanceof BrowserSseFailure
        ? cause
        : cause instanceof SseParseError
          ? new BrowserSseFailure("SSE stream 데이터가 올바르지 않습니다", false)
          : new BrowserSseFailure("SSE stream 연결에 실패했습니다", true);
      if (!options.reconnect || !failure.retryable || attempts >= MAX_SSE_RECONNECT_ATTEMPTS) {
        onUpdate({ sessionId, kind: "error", sequence, dropped: history.evicted, message: failure.message });
        return;
      }
    }

    attempts += 1;
    const remaining = Math.max(0, deadline - performance.now());
    if (!remaining) break;
    await sleepWithAbort(Math.min(Math.max(retryMs, MIN_SSE_RETRY_MS), MAX_RETRY_MS, remaining), signal);
  }
  if (!signal.aborted) {
    onUpdate({
      sessionId,
      kind: "error",
      sequence,
      dropped: history.evicted,
      message: "SSE stream 시간이 초과되었습니다",
    });
  }
}

class BrowserSseFailure extends Error {
  constructor(readonly message: string, readonly retryable: boolean) {
    super(message);
    this.name = "BrowserSseFailure";
  }
}

async function browserSseFetch(
  req: RequestTemplate,
  _options: SseOptions,
  signal: AbortSignal,
  deadline: number,
): Promise<Response> {
  const url = new URL(req.url);
  const params = new URLSearchParams(url.search);
  for (const parameter of req.params.slice(0, MAX_SSE_PARAMS)) {
    if (parameter.key) params.append(parameter.key, parameter.value);
  }
  url.search = params.toString();
  if (utf8ByteLength(url.toString()) > MAX_SSE_URL_BYTES) {
    throw new BrowserSseFailure("SSE 요청 URL이 너무 깁니다", false);
  }
  const headers = new Headers();
  for (const header of req.headers.slice(0, MAX_SSE_HEADERS)) {
    const headerName = header.key.trim().toLowerCase();
    if (
      isHeaderEnabled(header)
      && header.key
      && headerName !== "last-event-id"
      && headerName !== "accept"
      && !(req.body_kind === "multipart" && isMultipartDerivedHeader(header.key))
    ) {
      try {
        headers.append(header.key, header.value);
      } catch {
        throw new BrowserSseFailure("SSE 요청을 보낼 수 없습니다", false);
      }
    }
  }
  headers.set("Accept", "text/event-stream");
  if (req.auth?.kind === "basic") {
    try {
      headers.set("Authorization", "Basic " + btoa(`${req.auth.username}:${req.auth.password}`));
    } catch {
      throw new BrowserSseFailure("SSE 요청을 보낼 수 없습니다", false);
    }
  } else if (req.auth?.kind === "bearer") {
    try {
      headers.set("Authorization", "Bearer " + req.auth.token);
    } catch {
      throw new BrowserSseFailure("SSE 요청을 보낼 수 없습니다", false);
    }
  } else if (req.auth?.kind === "apikey" && req.auth.api_key) {
    try {
      headers.set(req.auth.api_key, req.auth.api_value);
    } catch {
      throw new BrowserSseFailure("SSE 요청을 보낼 수 없습니다", false);
    }
  }
  const cookieHeader = buildCookieHeader(req.cookies);
  if (cookieHeader) {
    try {
      headers.set("Cookie", cookieHeader);
    } catch {
      throw new BrowserSseFailure("SSE 요청을 보낼 수 없습니다", false);
    }
  }
  let body: BodyInit | undefined;
  if (req.method === "POST" && req.body_kind === "json" && req.body.trim()) {
    headers.set("Content-Type", "application/json");
    body = req.body;
  } else if (req.method === "POST" && req.body_kind === "raw" && req.body) {
    body = req.body;
  } else if (req.method === "POST" && req.body_kind === "form" && req.body.trim()) {
    const form = new URLSearchParams();
    for (const line of req.body.split(/\r?\n/u)) {
      const trimmed = line.trim();
      if (!trimmed || trimmed.startsWith("#")) continue;
      const [key, ...rest] = trimmed.split("=");
      form.append(key?.trim() ?? "", rest.join("=").trim());
    }
    headers.set("Content-Type", "application/x-www-form-urlencoded");
    body = form;
  } else if (req.method === "POST" && req.body_kind === "multipart") {
    const form = new FormData();
    for (const part of req.multipart) {
      if (isMultipartPartEnabled(part) && part.kind === "text" && part.name) form.append(part.name, part.value);
      if (isMultipartPartEnabled(part) && part.kind === "file") {
        throw new BrowserSseFailure("SSE multipart 파일 전송은 데스크톱 앱에서만 사용할 수 있습니다.", false);
      }
    }
    body = form;
  } else if (req.method === "GET" && req.body.trim()) {
    throw new BrowserSseFailure("GET SSE 요청에는 본문을 사용할 수 없습니다", false);
  }
  const remaining = Math.max(1, deadline - performance.now());
  const connectTimeout = Math.min(_options.connectTimeoutMs, remaining);
  const requestController = new AbortController();
  let timedOut = false;
  const relayAbort = () => requestController.abort();
  signal.addEventListener("abort", relayAbort, { once: true });
  const connectTimer = setTimeout(() => {
    timedOut = true;
    requestController.abort();
  }, connectTimeout);
  let response: Response;
  try {
    response = await fetch(url, { method: req.method, headers, body, redirect: "error", signal: requestController.signal });
  } catch {
    if (signal.aborted) throw new BrowserSseFailure("SSE stream이 중지되었습니다", false);
    if (timedOut || remaining <= 1) throw new BrowserSseFailure("SSE stream 시간이 초과되었습니다", true);
    throw new BrowserSseFailure("SSE stream 연결에 실패했습니다", true);
  } finally {
    clearTimeout(connectTimer);
    signal.removeEventListener("abort", relayAbort);
  }
  if (!response.ok || (response.headers.get("content-type") ?? "").split(";", 1)[0].trim().toLowerCase() !== "text/event-stream") {
    throw new BrowserSseFailure("SSE 응답 형식이 아닙니다", false);
  }
  return response;
}

async function readWithTimeout(
  reader: ReadableStreamDefaultReader<Uint8Array>,
  timeoutMs: number,
): Promise<ReadableStreamReadResult<Uint8Array>> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([
      reader.read(),
      new Promise<ReadableStreamReadResult<Uint8Array>>((_, reject) => {
        timer = setTimeout(() => {
          void reader.cancel().catch(() => undefined);
          reject(new BrowserSseFailure("SSE stream 시간이 초과되었습니다", true));
        }, timeoutMs);
      }),
    ]);
  } finally {
    if (timer !== undefined) clearTimeout(timer);
  }
}

function sleepWithAbort(delayMs: number, signal: AbortSignal): Promise<void> {
  return new Promise((resolve) => {
    if (signal.aborted) {
      resolve();
      return;
    }
    const timer = setTimeout(resolve, delayMs);
    signal.addEventListener("abort", () => {
      clearTimeout(timer);
      resolve();
    }, { once: true });
  });
}

function emitBrowserEvent(
  event: import("./lib/sse").SseEvent,
  req: RequestTemplate,
  sessionId: string,
  onUpdate: (update: SseUpdate) => void,
  sequence: number,
  history: SseEventBuffer,
): number {
  const safe: import("./lib/sse").SseEvent = {
    event: redactBrowserText(event.event, req),
    data: redactBrowserText(event.data, req),
    ...(event.id ? { id: redactBrowserText(event.id, req) } : {}),
    ...(event.retryMs === undefined ? {} : { retryMs: event.retryMs }),
  };
  if (
    utf8ByteLength(safe.event) > MAX_EVENT_NAME_BYTES
    || utf8ByteLength(safe.data) > MAX_EVENT_DATA_BYTES
    || (safe.id !== undefined && utf8ByteLength(safe.id) > MAX_EVENT_ID_BYTES)
  ) throw new BrowserSseFailure("SSE stream 데이터가 올바르지 않습니다", false);
  history.push(safe);
  const nextSequence = sequence + 1;
  if (!Number.isSafeInteger(nextSequence)) throw new BrowserSseFailure("SSE stream 데이터가 올바르지 않습니다", false);
  onUpdate({
    sessionId,
    kind: "event",
    event: safe.event,
    data: safe.data,
    ...(safe.id ? { id: safe.id } : {}),
    ...(safe.retryMs === undefined ? {} : { retryMs: safe.retryMs }),
    sequence: nextSequence,
    dropped: history.evicted,
  });
  return nextSequence;
}

function validateSseOptions(options: SseOptions): void {
  if (!Number.isInteger(options.connectTimeoutMs) || options.connectTimeoutMs < MIN_SSE_CONNECT_TIMEOUT_MS || options.connectTimeoutMs > MAX_SSE_CONNECT_TIMEOUT_MS) {
    throw new Error("SSE 연결 timeout 범위가 올바르지 않습니다.");
  }
  if (!Number.isInteger(options.idleTimeoutMs) || options.idleTimeoutMs < MIN_SSE_IDLE_TIMEOUT_MS || options.idleTimeoutMs > MAX_SSE_IDLE_TIMEOUT_MS) {
    throw new Error("SSE idle timeout 범위가 올바르지 않습니다.");
  }
  if (!Number.isInteger(options.totalTimeoutMs) || options.totalTimeoutMs < MIN_SSE_TOTAL_TIMEOUT_MS || options.totalTimeoutMs > MAX_SSE_TOTAL_TIMEOUT_MS) {
    throw new Error("SSE 전체 timeout 범위가 올바르지 않습니다.");
  }
}

function validateSseEnvironment(environment: EnvVariable[]): void {
  if (environment.length > MAX_SSE_ENVIRONMENT_VARIABLES) {
    throw new Error("SSE 환경 변수는 최대 100개까지 사용할 수 있습니다.");
  }
  if (environment.some((variable) =>
    !variable.key
    || utf8ByteLength(variable.key) > 128
    || utf8ByteLength(variable.value) > 64 * 1024
  )) {
    throw new Error("SSE 환경 변수 형식이 올바르지 않습니다.");
  }
}

function validateBrowserSseRequest(req: RequestTemplate): void {
  const method = req.method.trim().toUpperCase();
  if (method !== "GET" && method !== "POST") throw new Error("SSE stream은 GET 또는 POST만 지원합니다.");
  if (
    utf8ByteLength(req.url) > MAX_SSE_URL_BYTES
    || req.headers.length > MAX_SSE_HEADERS
    || req.cookies.length > 100
    || req.params.length > MAX_SSE_PARAMS
  ) {
    throw new Error("SSE 요청 항목 수 또는 URL이 제한을 초과했습니다.");
  }
  if (utf8ByteLength(req.body) > MAX_SSE_BODY_BYTES) throw new Error("SSE 요청 본문이 너무 큽니다.");
  const hasMultipartContent = req.body_kind === "multipart" && req.multipart.some((part) =>
    isMultipartPartEnabled(part)
    && Boolean(part.name || part.value || part.file_path || part.file_name || part.content_type)
  );
  if (method === "GET" && (req.body.trim() || hasMultipartContent)) {
    throw new Error("GET SSE 요청에는 본문을 사용할 수 없습니다.");
  }
  const cookieIssue = validateCookies(req.cookies)[0];
  if (cookieIssue) throw new Error(cookieIssue.message);
  if (hasCookieSourceConflict(req.cookies, req.headers)) {
    throw new Error("Cookie header와 구조화 Cookie를 동시에 전송할 수 없습니다.");
  }
  if (!["none", "json", "form", "multipart", "raw"].includes(req.body_kind)) {
    throw new Error("SSE 요청 본문 형식이 올바르지 않습니다.");
  }
  let url: URL;
  try {
    url = new URL(req.url);
  } catch {
    throw new Error("SSE 요청 URL이 올바르지 않습니다.");
  }
  if (!/^https?:$/u.test(url.protocol) || url.username || url.password || url.hash) {
    throw new Error("SSE 요청 URL이 올바르지 않습니다.");
  }
  if (req.auth && !["none", "basic", "bearer", "apikey"].includes(req.auth.kind)) {
    throw new Error("SSE 인증 설정이 올바르지 않습니다.");
  }
  for (const header of req.headers) {
    if (utf8ByteLength(header.key) > 256 || utf8ByteLength(header.value) > 64 * 1024) throw new Error("SSE 요청 header가 너무 깁니다.");
  }
  for (const cookie of req.cookies) {
    if (utf8ByteLength(cookie.name) > 256 || utf8ByteLength(cookie.value) > 64 * 1024) throw new Error("SSE 요청 Cookie가 너무 깁니다.");
  }
  for (const parameter of req.params) {
    if (utf8ByteLength(parameter.key) > 64 * 1024 || utf8ByteLength(parameter.value) > 64 * 1024) throw new Error("SSE 요청 parameter가 너무 깁니다.");
  }
  if (req.body_kind === "multipart") {
    const multipartIssue = validateMultipartParts(req.multipart)[0];
    if (multipartIssue) throw new Error(multipartIssue.message);
    if (req.multipart.some((part) => utf8ByteLength(part.file_path) > MAX_SSE_URL_BYTES)) {
      throw new Error("SSE multipart 파일 경로가 너무 깁니다.");
    }
    if (req.multipart.some((part) => isMultipartPartEnabled(part) && part.kind === "file")) {
      throw new Error("SSE multipart 파일 전송은 데스크톱 앱에서만 사용할 수 있습니다.");
    }
    if (req.multipart.some((part) =>
      isMultipartPartEnabled(part) && part.kind === "text" && Boolean(part.content_type)
    )) {
      throw new Error("SSE multipart part별 Content-Type은 데스크톱 앱에서만 사용할 수 있습니다.");
    }
  }
  if (req.body_kind === "none" && req.body.trim()) {
    throw new Error("SSE 요청 본문 형식이 올바르지 않습니다.");
  }
  if (req.auth && [
    req.auth.kind,
    req.auth.username,
    req.auth.password,
    req.auth.token,
    req.auth.api_key,
    req.auth.api_value,
  ].some((value) => utf8ByteLength(value) > 64 * 1024)) {
    throw new Error("SSE 인증 설정이 너무 깁니다.");
  }
}

function parseSseUpdate(value: unknown): SseUpdate | null {
  if (!value || typeof value !== "object") return null;
  const candidate = value as Partial<SseUpdate>;
  if (!isSseSessionId(candidate.sessionId) || !["connected", "event", "closed", "error"].includes(candidate.kind ?? "")) return null;
  if (!Number.isSafeInteger(candidate.sequence) || !Number.isSafeInteger(candidate.dropped) || (candidate.sequence ?? 0) < 0 || (candidate.dropped ?? 0) < 0 || (candidate.dropped ?? 0) > MAX_DECODED_BYTES) return null;
  if (candidate.event !== undefined && (typeof candidate.event !== "string" || utf8ByteLength(candidate.event) > MAX_EVENT_NAME_BYTES)) return null;
  if (candidate.data !== undefined && (typeof candidate.data !== "string" || utf8ByteLength(candidate.data) > MAX_EVENT_DATA_BYTES)) return null;
  if (candidate.id !== undefined && (typeof candidate.id !== "string" || utf8ByteLength(candidate.id) > MAX_EVENT_ID_BYTES)) return null;
  if (candidate.message !== undefined && (typeof candidate.message !== "string" || !SAFE_SSE_UPDATE_MESSAGES.has(candidate.message))) return null;
  if (candidate.retryMs !== undefined && (!Number.isSafeInteger(candidate.retryMs) || candidate.retryMs < 0 || candidate.retryMs > MAX_RETRY_MS)) return null;
  if (candidate.attempt !== undefined && (!Number.isSafeInteger(candidate.attempt) || candidate.attempt < 0 || candidate.attempt > MAX_SSE_RECONNECT_ATTEMPTS)) return null;
  return candidate as SseUpdate;
}

function isSseSessionId(value: unknown): value is string {
  return typeof value === "string" && value.length <= 40 && /^(?:sse|browser-sse)-[0-9]+$/u.test(value);
}

function safeSseStartError(cause: unknown): string {
  const raw = cause instanceof Error ? cause.message : typeof cause === "string" ? cause : "";
  const allowed = [
    "SSE 연결 timeout 범위가 올바르지 않습니다",
    "SSE idle timeout 범위가 올바르지 않습니다",
    "SSE 전체 timeout 범위가 올바르지 않습니다",
    "SSE 환경 변수는 최대 100개까지 사용할 수 있습니다",
    "SSE 환경 변수 형식이 올바르지 않습니다",
    "SSE stream은 GET 또는 POST만 지원합니다",
    "SSE 요청 URL이 너무 깁니다",
    "SSE 요청 URL이 올바르지 않습니다",
    "SSE 요청 항목 수가 제한을 초과했습니다",
    "SSE 요청 항목 수 또는 URL이 제한을 초과했습니다",
    "SSE 요청 본문이 너무 큽니다",
    "SSE 요청 header가 너무 깁니다",
    "SSE 요청 Cookie가 너무 깁니다",
    "SSE 요청 parameter가 너무 깁니다",
    "SSE 인증 설정이 너무 깁니다",
    "SSE 인증 설정이 올바르지 않습니다",
    "SSE 요청 header가 올바르지 않습니다",
    "SSE multipart 파일 경로가 너무 깁니다",
    "SSE 요청 본문 형식이 올바르지 않습니다",
    "SSE 요청을 보낼 수 없습니다",
    "SSE 리다이렉트 정책으로 요청을 차단했습니다",
    "Cookie header와 구조화 Cookie를 동시에 전송할 수 없습니다.",
    "Cookie는 최대 100행까지 사용할 수 있습니다.",
    "이름이 필요합니다.",
    "이름에 Cookie token으로 쓸 수 없는 문자가 있습니다.",
    "값에 공백, 세미콜론, 따옴표 또는 제어 문자를 사용할 수 없습니다.",
    "SSE 응답 형식이 아닙니다",
    "SSE multipart 파일 전송은 데스크톱 앱에서만 사용할 수 있습니다.",
    "SSE multipart part별 Content-Type은 데스크톱 앱에서만 사용할 수 있습니다.",
    "GET SSE 요청에는 본문을 사용할 수 없습니다",
    "secret 포함 SSE stream은 데스크톱 앱에서만 사용할 수 있습니다.",
  ];
  const normalized = raw.replace(/^Error:\s*/u, "").replace(/\.$/u, "");
  return allowed.includes(normalized) || allowed.includes(raw) ? raw : "SSE stream을 시작하지 못했습니다.";
}

const WEBSOCKET_EVENT = "api-playground/websocket";
const SAFE_WEBSOCKET_MESSAGES = new Set([
  "WebSocket endpoint URL이 올바르지 않습니다",
  "WebSocket endpoint query에 credential을 넣을 수 없습니다",
  "WebSocket 요청 URL이 너무 깁니다",
  "WebSocket 요청 header가 올바르지 않습니다",
  "WebSocket 요청 header가 너무 깁니다",
  "WebSocket 요청 parameter가 올바르지 않습니다",
  "WebSocket 요청 항목 수가 제한을 초과했습니다",
  "WebSocket 연결 timeout 범위가 올바르지 않습니다",
  "WebSocket 인증 설정이 올바르지 않습니다",
  "WebSocket 연결을 시작할 수 없습니다",
  "이미 실행 중인 WebSocket 연결이 있습니다",
  "WebSocket 연결이 열려 있지 않습니다",
  "WebSocket message를 보낼 수 없습니다",
  "WebSocket ping을 보낼 수 없습니다",
  "WebSocket 연결을 닫을 수 없습니다",
  "WebSocket binary payload가 올바르지 않습니다",
  "WebSocket binary를 안전하게 저장할 수 없습니다",
  "WebSocket message가 허용된 크기를 초과했습니다",
  "WebSocket close code가 올바르지 않습니다",
  "WebSocket close reason이 올바르지 않습니다",
  "WebSocket 연결 시간이 초과되었습니다",
  "WebSocket 연결에 실패했습니다",
  "WebSocket 연결이 끊어졌습니다",
]);

function safeWebSocketError(cause: unknown): Error {
  const raw = cause instanceof Error ? cause.message : typeof cause === "string" ? cause : "";
  const message = raw.replace(/^Error:\s*/u, "");
  return new Error(SAFE_WEBSOCKET_MESSAGES.has(message) ? message : "WebSocket 요청에 실패했습니다.");
}

function isWebSocketSessionId(value: unknown): value is string {
  return typeof value === "string" && /^ws-\d{1,26}$/u.test(value);
}

function isConnectionState(value: unknown): value is WebSocketConnectionState {
  return value === "idle" || value === "connecting" || value === "open"
    || value === "closing" || value === "closed" || value === "error";
}

function isMessageKind(value: unknown): value is WebSocketMessage["kind"] {
  return value === "text" || value === "binary" || value === "ping" || value === "pong" || value === "close";
}

function isMessageDirection(value: unknown): value is WebSocketMessage["direction"] {
  return value === "sent" || value === "received";
}

function isBoundedString(value: unknown, maxBytes: number): value is string {
  return typeof value === "string" && utf8Bytes(value) <= maxBytes;
}

function isBinaryPreview(value: unknown): value is string {
  if (value === "[REDACTED]") return true;
  if (typeof value !== "string") return false;
  const normalized = value.endsWith("…") ? value.slice(0, -1) : value;
  return normalized.length <= MAX_BINARY_PREVIEW_BYTES * 2
    && normalized.length % 2 === 0
    && /^[0-9a-f]*$/u.test(normalized);
}

function parseWebSocketUpdate(payload: unknown): WebSocketUpdate | null {
  if (!payload || typeof payload !== "object") return null;
  const candidate = payload as Record<string, unknown>;
  if (!isWebSocketSessionId(candidate.sessionId)
    || (candidate.kind !== "state" && candidate.kind !== "message")
    || !Number.isSafeInteger(candidate.sequence) || Number(candidate.sequence) < 0
    || !Number.isSafeInteger(candidate.dropped) || Number(candidate.dropped) < 0) {
    return null;
  }
  if (candidate.kind === "state") {
    return isConnectionState(candidate.state)
      ? {
        sessionId: candidate.sessionId,
        kind: "state",
        state: candidate.state,
        sequence: Number(candidate.sequence),
        dropped: Number(candidate.dropped),
        ...(typeof candidate.message === "string" && SAFE_WEBSOCKET_MESSAGES.has(candidate.message)
          ? { message: candidate.message } : {}),
      }
      : null;
  }
  if (!isMessageKind(candidate.messageType)
    || !isMessageDirection(candidate.direction)
    || !Number.isSafeInteger(candidate.messageId) || Number(candidate.messageId) < 1) {
    return null;
  }
  if ((candidate.text !== undefined && !isBoundedString(candidate.text, MAX_TEXT_PREVIEW_BYTES))
    || (candidate.binaryHex !== undefined && !isBinaryPreview(candidate.binaryHex))
    || (candidate.binaryText !== undefined && !isBoundedString(candidate.binaryText, MAX_TEXT_PREVIEW_BYTES))
    || (candidate.binarySize !== undefined
      && (!Number.isSafeInteger(candidate.binarySize)
        || Number(candidate.binarySize) < 0
        || Number(candidate.binarySize) > MAX_MESSAGE_BYTES))
    || (candidate.closeCode !== undefined
      && (!Number.isInteger(candidate.closeCode)
        || Number(candidate.closeCode) < 0
        || Number(candidate.closeCode) > 65_535))
    || (candidate.closeReason !== undefined
      && !isBoundedString(candidate.closeReason, MAX_CLOSE_REASON_BYTES))) {
    return null;
  }
  const update: WebSocketUpdate = {
    sessionId: candidate.sessionId,
    kind: "message",
    direction: candidate.direction,
    messageType: candidate.messageType,
    messageId: Number(candidate.messageId),
    sequence: Number(candidate.sequence),
    dropped: Number(candidate.dropped),
  };
  if (typeof candidate.text === "string") update.text = candidate.text;
  if (typeof candidate.textTruncated === "boolean") update.textTruncated = candidate.textTruncated;
  if (typeof candidate.binaryHex === "string") update.binaryHex = candidate.binaryHex;
  if (typeof candidate.binaryText === "string") update.binaryText = candidate.binaryText;
  if (Number.isSafeInteger(candidate.binarySize) && Number(candidate.binarySize) >= 0) {
    update.binarySize = Number(candidate.binarySize);
  }
  if (typeof candidate.binaryTruncated === "boolean") update.binaryTruncated = candidate.binaryTruncated;
  if (Number.isInteger(candidate.closeCode) && Number(candidate.closeCode) >= 0) update.closeCode = Number(candidate.closeCode);
  if (typeof candidate.closeReason === "string") update.closeReason = candidate.closeReason;
  return update;
}

export interface WebSocketHandle {
  sessionId: string;
  send: (kind: "text" | "binary", value: string, encoding?: "text" | "hex") => Promise<void>;
  ping: (value: string, encoding?: "text" | "hex") => Promise<void>;
  close: (code?: number, reason?: string) => Promise<void>;
  saveBinary: (messageId: number) => Promise<boolean>;
  stop: () => Promise<void>;
}

export async function startWebSocket(
  req: RequestTemplate,
  environment: Parameters<typeof sendRequest>[1],
  onUpdate: (update: WebSocketUpdate) => void,
): Promise<WebSocketHandle> {
  if (isTauri()) return startNativeWebSocket(req, environment, onUpdate);
  return startBrowserWebSocket(req, environment, onUpdate);
}

async function startNativeWebSocket(
  req: RequestTemplate,
  environment: Parameters<typeof sendRequest>[1],
  onUpdate: (update: WebSocketUpdate) => void,
): Promise<WebSocketHandle> {
  const { listen } = await import("@tauri-apps/api/event");
  let sessionId: string | null = null;
  const pending: WebSocketUpdate[] = [];
  const unlisten = await listen<unknown>(WEBSOCKET_EVENT, (event) => {
    const update = parseWebSocketUpdate(event.payload);
    if (!update) return;
    if (!sessionId) {
      if (pending.length < 64) pending.push(update);
      return;
    }
    if (update.sessionId === sessionId) onUpdate(update);
  });
  let started: string;
  try {
    started = await invoke<string>("start_websocket", { req, environment });
    if (!isWebSocketSessionId(started)) throw new Error("invalid session");
    sessionId = started;
    for (const update of pending.splice(0)) {
      if (update.sessionId === started) onUpdate(update);
    }
  } catch (cause) {
    await unlisten();
    throw safeWebSocketError(cause);
  }

  const activeSessionId = started;
  let stopped = false;
  const invokeMessage = async (message: WebSocketMessageInput): Promise<void> => {
    if (stopped) throw new Error("WebSocket 연결이 열려 있지 않습니다");
    try {
      await invoke("send_websocket_message", { sessionId: activeSessionId, message });
    } catch (cause) {
      throw safeWebSocketError(cause);
    }
  };
  const close = async (code?: number, reason = "") => {
    validateCloseCode(code);
    validateCloseReason(reason);
    if (stopped) return;
    try {
      await invoke("close_websocket", { sessionId: activeSessionId, close: { code, reason } });
    } catch (cause) {
      throw safeWebSocketError(cause);
    }
  };
  return {
    sessionId: activeSessionId,
    send: async (kind, value, encoding = "text") => {
      await invokeMessage(toNativeMessageInput(kind, value, encoding));
    },
    ping: async (value, encoding = "text") => {
      const payload = encoding === "hex" ? hexToBytes(value) : textToBytes(value);
      if (payload.byteLength > MAX_CONTROL_PAYLOAD_BYTES) throw new Error(MESSAGE_TOO_LARGE);
      try {
        await invoke("ping_websocket", { sessionId: activeSessionId, data: encodeBase64(payload) });
      } catch (cause) {
        throw safeWebSocketError(cause);
      }
    },
    close,
    saveBinary: async (messageId) => {
      if (!Number.isSafeInteger(messageId) || messageId < 1) throw new Error("WebSocket binary payload가 올바르지 않습니다");
      try {
        return await invoke<boolean>("save_websocket_binary", { sessionId: activeSessionId, messageId });
      } catch (cause) {
        throw safeWebSocketError(cause);
      }
    },
    stop: async () => {
      if (stopped) return;
      stopped = true;
      try {
        await invoke("disconnect_websocket", { sessionId: activeSessionId });
      } catch (cause) {
        throw safeWebSocketError(cause);
      } finally {
        await unlisten();
      }
    },
  };
}

let browserWebSocketSequence = 0;

async function startBrowserWebSocket(
  request: RequestTemplate,
  environment: Parameters<typeof sendRequest>[1],
  onUpdate: (update: WebSocketUpdate) => void,
): Promise<WebSocketHandle> {
  if (environment.some((variable) => variable.secret)) {
    throw new Error("secret 포함 WebSocket은 데스크톱 앱에서만 전송할 수 있습니다");
  }
  const variables = new Map(environment.map((variable) => [variable.key, variable.value]));
  const resolved = {
    ...applyToRequest(request, variables),
    method: request.method.trim().toUpperCase(),
  };
  validateWebSocketRequest(resolved);
  const activeHeaders = resolved.headers.filter((header) => header.enabled !== false && header.key.trim());
  const activeCookies = resolved.cookies.filter((cookie) => cookie.enabled !== false && (cookie.name || cookie.value));
  if (activeHeaders.length > 0 || activeCookies.length > 0 || (resolved.auth && resolved.auth.kind !== "none")) {
    throw new Error("브라우저 미리보기에서는 WebSocket custom header/auth를 사용할 수 없습니다");
  }
  const url = buildWebSocketUrl(resolved.url, resolved.params);
  const socket = new WebSocket(url);
  socket.binaryType = "arraybuffer";
  const sessionId = `browser-ws-${++browserWebSocketSequence}`;
  let nextMessageId = 1;
  let sequence = 0;
  let stopped = false;
  let socketClosed = false;
  let connectTimer: ReturnType<typeof setTimeout> | null = null;
  const retained = new WebSocketMessageBuffer();
  const rawBinary = new Map<number, Uint8Array>();
  const clearConnectTimer = () => {
    if (connectTimer !== null) {
      clearTimeout(connectTimer);
      connectTimer = null;
    }
  };
  const emitState = (state: WebSocketConnectionState, message?: string) => {
    onUpdate({ sessionId, kind: "state", state, sequence, dropped: retained.evicted, ...(message ? { message } : {}) });
  };
  const emitMessage = (message: WebSocketMessage) => {
    retained.push(message);
    for (const id of retained.takeEvictedIds()) rawBinary.delete(id);
    sequence += 1;
    onUpdate({
      sessionId,
      kind: "message",
      direction: message.direction,
      messageId: message.id,
      messageType: message.kind,
      text: message.text,
      textTruncated: message.textTruncated,
      binaryHex: message.binaryHex,
      binaryText: message.binaryText,
      binarySize: message.binarySize,
      binaryTruncated: message.binaryTruncated,
      closeCode: message.closeCode,
      closeReason: message.closeReason,
      sequence,
      dropped: retained.evicted,
    });
  };
  const nextId = () => {
    const id = nextMessageId;
    nextMessageId += 1;
    return id;
  };
  socket.onopen = () => {
    clearConnectTimer();
    if (!stopped) emitState("open");
  };
  socket.onmessage = (event) => {
    if (stopped) return;
    if (typeof event.data === "string") {
      try { emitMessage(makeTextMessage(nextId(), "received", event.data, resolved)); } catch { emitState("error", "WebSocket message가 허용된 크기를 초과했습니다"); }
      return;
    }
    const readBinary = async () => {
      try {
        const bytes = event.data instanceof ArrayBuffer
          ? new Uint8Array(event.data)
          : event.data instanceof Blob
            ? new Uint8Array(await event.data.arrayBuffer())
            : null;
        if (!bytes) throw new Error("binary");
        if (stopped || socketClosed) return;
        const id = nextId();
        const message = makeBinaryMessage(id, "received", bytes, resolved);
        rawBinary.set(id, bytes);
        emitMessage(message);
      } catch {
        emitState("error", "WebSocket binary payload가 올바르지 않습니다");
      }
    };
    void readBinary();
  };
  socket.onerror = () => {
    clearConnectTimer();
    if (!stopped) emitState("error", "WebSocket 연결에 실패했습니다");
  };
  socket.onclose = (event) => {
    clearConnectTimer();
    socketClosed = true;
    if (!stopped) {
      const reason = maskWebSocketCloseReason(event.reason, resolved);
      const id = nextId();
      emitMessage({ id, direction: "received", kind: "close", closeCode: event.code, closeReason: reason });
      emitState("closed");
    }
  };
  emitState("connecting");
  connectTimer = setTimeout(() => {
    if (stopped || socketClosed || socket.readyState !== WebSocket.CONNECTING) return;
    stopped = true;
    emitState("error", "WebSocket 연결 시간이 초과되었습니다");
    try { socket.close(); } catch { /* The browser owns CONNECTING socket teardown. */ }
  }, resolved.timeout_ms);
  return {
    sessionId,
    send: async (kind, value, encoding = "text") => {
      if (stopped || socket.readyState !== WebSocket.OPEN) throw new Error("WebSocket 연결이 열려 있지 않습니다");
      if (kind === "text") {
        const message = makeTextMessage(nextId(), "sent", value, resolved);
        socket.send(value);
        emitMessage(message);
      } else {
        const bytes = encoding === "hex" ? hexToBytes(value) : textToBytes(value, MAX_MESSAGE_BYTES);
        if (bytes.byteLength > MAX_MESSAGE_BYTES) throw new Error(MESSAGE_TOO_LARGE);
        const message = makeBinaryMessage(nextId(), "sent", bytes, resolved);
        socket.send(bytes);
        rawBinary.set(message.id, bytes);
        emitMessage(message);
      }
    },
    ping: async () => {
      throw new Error("브라우저 미리보기에서는 ping/pong을 직접 보낼 수 없습니다");
    },
    close: async (code, reason = "") => {
      if (stopped || socketClosed) return;
      validateCloseCode(code);
      validateCloseReason(reason);
      emitState("closing");
      socket.close(code ?? 1000, reason);
    },
    saveBinary: async (messageId) => {
      const bytes = rawBinary.get(messageId);
      if (!bytes) throw new Error("WebSocket binary payload가 올바르지 않습니다");
      const blob = new Blob([bytes], { type: "application/octet-stream" });
      const anchor = document.createElement("a");
      anchor.href = URL.createObjectURL(blob);
      anchor.download = `websocket-message-${messageId}.bin`;
      anchor.click();
      URL.revokeObjectURL(anchor.href);
      return true;
    },
    stop: async () => {
      if (stopped) return;
      stopped = true;
      clearConnectTimer();
      if (!socketClosed && socket.readyState < WebSocket.CLOSING) socket.close(1000, "client disconnect");
    },
  };
}

function maskWebSocketCloseReason(value: string, request: RequestTemplate): string {
  return value ? utf8Truncate(redactBrowserText(value, request), MAX_CLOSE_REASON_BYTES).value : "";
}
