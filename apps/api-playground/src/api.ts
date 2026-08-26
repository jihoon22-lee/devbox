import { invoke } from "@tauri-apps/api/core";
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
  isMultipartPartEnabled,
  isMultipartDerivedHeader,
  safeMultipartFileName,
  validateMultipartParts,
  type PickedMultipartFile,
} from "./lib/multipart";
import type { ApiResponse, RequestTemplate } from "./types";

const MAX_RESPONSE_HEADERS = 100;
const MAX_RESPONSE_HEADER_BYTES = 64 * 1024;
let nativeRequestSequence = 0;

function nextNativeRequestId(): string {
  nativeRequestSequence = (nativeRequestSequence + 1) % Number.MAX_SAFE_INTEGER;
  const randomId = globalThis.crypto?.randomUUID?.().replace(/-/g, "");
  return randomId
    ? `request-${randomId}`
    : `request-${Date.now().toString(36)}-${nativeRequestSequence.toString(36)}`;
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

async function browserFetch(req: RequestTemplate, environment: EnvVariable[], signal?: AbortSignal): Promise<ApiResponse> {
  if (environment.some((variable) => variable.secret)) {
    throw new Error("secret 포함 요청은 데스크톱 앱에서만 전송할 수 있습니다");
  }
  const variables = new Map(environment.map((variable) => [variable.key, variable.value]));
  let resolved = applyToRequest(req, variables);
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
  const exactRedacted = directSecrets.sort((a, b) => b.length - a.length).reduce(
    (result, secret) => result.split(secret).join("[REDACTED]"),
    text,
  );
  try {
    return JSON.stringify(redactBrowserJson(JSON.parse(exactRedacted) as unknown));
  } catch {
    return exactRedacted.replace(
      /((?:authorization|cookie|set[-_]?cookie|api[-_]?key|api[-_]?value|token|secret|password|passwd|private[-_]?key|username)\s*[=:]\s*)([^\s,;&]+)/gi,
      "$1[REDACTED]",
    );
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
