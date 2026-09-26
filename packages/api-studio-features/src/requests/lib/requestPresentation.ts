import { buildRevealedCurl } from "../api";
import { sanitizeRequestForPersistence } from "./persistence";
import { isExactVariableReference } from "./references";
import { buildCookieHeader, hasCookieSourceConflict, validateCookies } from "./cookies";
import { isHeaderEnabled } from "./headers";
import {
  buildGraphqlBody,
  buildGraphqlGetUrl,
  isGraphqlDerivedHeader,
  validateGraphqlDocument,
  validateGraphqlEndpoint,
  GRAPHQL_OPERATION_INVALID,
  GRAPHQL_VARIABLES_INVALID,
  MAX_GRAPHQL_OPERATION_NAME_BYTES,
  MAX_GRAPHQL_QUERY_BYTES,
  MAX_GRAPHQL_VARIABLES_BYTES,
  GRAPHQL_HEADER_BYTES_ERROR,
  GRAPHQL_HEADER_ROWS_ERROR,
  GRAPHQL_URL_TOO_LARGE,
  MIN_GRAPHQL_TIMEOUT_MS,
  MAX_GRAPHQL_TIMEOUT_MS,
  validateGraphqlHeaders,
  validateGraphqlParams,
} from "./graphql";
import { isMultipartPartEnabled, isMultipartDerivedHeader, validateMultipartParts } from "./multipart";
import type { GraphqlRequest, RequestTemplate, SseOptions } from "../types";

export function sseStateLabel(state: "idle" | "connecting" | "connected" | "stopped" | "closed" | "error"): string {
  return {
    idle: "대기",
    connecting: "연결 중",
    connected: "연결됨",
    stopped: "중지됨",
    closed: "닫힘",
    error: "오류",
  }[state];
}

export function downloadJson(content: string, fileName: string): void {
  const blob = new Blob([content], { type: "application/json;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = fileName;
  anchor.click();
  window.setTimeout(() => URL.revokeObjectURL(url), 0);
}

export const defaultSseOptions = (): SseOptions => ({
  connectTimeoutMs: 10_000,
  idleTimeoutMs: 30_000,
  totalTimeoutMs: 300_000,
  reconnect: false,
});

export const emptyReq = (): RequestTemplate => ({
  method: "GET",
  url: "",
  headers: [],
  cookies: [],
  multipart: [],
  params: [],
  body_kind: "none",
  body: "",
  auth: { kind: "none", username: "", password: "", token: "", api_key: "", api_value: "" },
  timeout_ms: 10000,
  graphql: null,
});

export const emptyGraphql = (): GraphqlRequest => ({ query: "", variables: "", operation_name: "" });

export function graphqlConfigError(request: RequestTemplate): string | null {
  if (request.body_kind !== "graphql") return null;
  if (!request.graphql || !["GET", "POST"].includes(request.method)) {
    return "GraphQL 요청 구성이 올바르지 않습니다";
  }
  if (request.timeout_ms < MIN_GRAPHQL_TIMEOUT_MS || request.timeout_ms > MAX_GRAPHQL_TIMEOUT_MS) {
    return "GraphQL timeout이 허용된 범위를 벗어났습니다";
  }
  const encoder = new TextEncoder();
  if (
    encoder.encode(request.graphql.query).byteLength > MAX_GRAPHQL_QUERY_BYTES ||
    encoder.encode(request.graphql.variables).byteLength > MAX_GRAPHQL_VARIABLES_BYTES ||
    encoder.encode(request.graphql.operation_name).byteLength > MAX_GRAPHQL_OPERATION_NAME_BYTES
  ) {
    return "GraphQL 요청 구성이 올바르지 않습니다";
  }
  try {
    validateGraphqlHeaders(request.headers);
    validateGraphqlParams(request.params);
    validateGraphqlEndpoint(request.url);
    validateGraphqlDocument(request.graphql.query, request.graphql.operation_name);
    buildGraphqlBody(request.graphql);
    if (request.method === "GET") buildGraphqlGetUrl(request.url, request.params, request.graphql);
    return null;
  } catch (cause) {
    const message = cause instanceof Error ? cause.message : "";
    const safe = new Set([
      "GraphQL endpoint URL이 올바르지 않습니다",
      "GraphQL endpoint query에 credential을 넣을 수 없습니다",
      "GraphQL query가 허용된 크기를 초과했습니다",
      "GraphQL variables가 허용된 크기를 초과했습니다",
      "GraphQL 문서 형식이 올바르지 않습니다",
      GRAPHQL_OPERATION_INVALID,
      GRAPHQL_VARIABLES_INVALID,
      "GraphQL variables 구조가 허용된 한계를 초과했습니다",
      "GraphQL 요청 본문이 허용된 크기를 초과했습니다",
      "GraphQL introspection 요청은 지원하지 않습니다",
      "GraphQL subscription은 지원하지 않습니다",
      GRAPHQL_HEADER_ROWS_ERROR,
      GRAPHQL_HEADER_BYTES_ERROR,
      GRAPHQL_URL_TOO_LARGE,
    ]);
    return safe.has(message) ? message : "GraphQL 요청 구성이 올바르지 않습니다";
  }
}

export function tryPretty(json: string): string {
  try {
    return JSON.stringify(JSON.parse(json), null, 2);
  } catch {
    return json;
  }
}

/** 요청 구성을 기본 마스킹된 curl 명령으로 만든다. */
export function buildCurl(template: RequestTemplate): string {
  if (!template.url) return "";
  if (
    validateCookies(template.cookies).length > 0 ||
    hasCookieSourceConflict(template.cookies, template.headers) ||
    (template.body_kind === "multipart" &&
      validateMultipartParts(template.multipart).some((issue) => issue.field !== "file"))
  ) {
    return "";
  }
  if (template.body_kind === "graphql") {
    try {
      validateGraphqlHeaders(template.headers);
      validateGraphqlParams(template.params);
      validateGraphqlEndpoint(template.url);
    } catch {
      return "";
    }
  }
  const req = sanitizeRequestForPersistence(template);
  if (req.body_kind === "graphql" && (!req.graphql || !["GET", "POST"].includes(req.method))) return "";
  const safeGraphql =
    req.graphql &&
    (() => {
      try {
        buildGraphqlBody(req.graphql);
        return req.graphql;
      } catch {
        // A malformed or redacted variables draft remains visible in the editor, but
        // masked cURL uses an empty variables object instead of leaking raw text.
        return { ...req.graphql, variables: "{}" };
      }
    })();
  const safeGraphqlBody =
    req.body_kind === "graphql" && safeGraphql
      ? (() => {
          try {
            return buildGraphqlBody(safeGraphql);
          } catch {
            return JSON.stringify({
              ...(safeGraphql.operation_name ? { operationName: safeGraphql.operation_name } : {}),
              query: safeGraphql.query,
              variables: {},
            });
          }
        })()
      : "";

  const params = new URLSearchParams();
  for (const p of req.params) if (p.key) params.append(p.key, p.value);
  const sep = req.url.includes("?") ? "&" : "?";
  const url =
    req.body_kind === "graphql" && safeGraphql && req.method === "GET"
      ? (() => {
          try {
            return buildGraphqlGetUrl(req.url, req.params, safeGraphql);
          } catch {
            return "";
          }
        })()
      : params.size
        ? req.url + sep + params.toString()
        : req.url;
  if (!url) return "";

  const lines = [`curl --request ${req.method} ${shellQuote(url)}`];

  const headers: [string, string][] = [];
  for (const h of req.headers) {
    if (
      isHeaderEnabled(h) &&
      h.key &&
      !(req.body_kind === "multipart" && isMultipartDerivedHeader(h.key)) &&
      !(req.body_kind === "graphql" && isGraphqlDerivedHeader(h.key))
    ) {
      headers.push([h.key, h.value]);
    }
  }
  const cookieHeader = buildCookieHeader(req.cookies);
  if (cookieHeader) headers.push(["Cookie", cookieHeader]);
  if (req.auth?.kind === "basic" && req.auth.username) {
    headers.push(["Authorization", "Basic [REDACTED]"]);
  } else if (req.auth?.kind === "bearer" && req.auth.token) {
    headers.push([
      "Authorization",
      `Bearer ${isExactVariableReference(req.auth.token) ? req.auth.token : "[REDACTED]"}`,
    ]);
  } else if (req.auth?.kind === "apikey" && req.auth.api_key) {
    headers.push([req.auth.api_key, isExactVariableReference(req.auth.api_value) ? req.auth.api_value : "[REDACTED]"]);
  }
  for (const [k, v] of headers) {
    lines.push(`  --header ${shellQuote(`${k}: ${v}`)}`);
  }

  if (req.body_kind === "multipart") {
    for (const part of req.multipart) {
      if (!isMultipartPartEnabled(part) || !part.name) continue;
      const suffix = part.content_type ? `;type=${part.content_type}` : "";
      const value =
        part.kind === "text"
          ? curlFormQuote(part.value)
          : `@${curlFormQuote(`[RESELECT_FILE:${part.file_name || "file"}]`)}`;
      lines.push(`  --form ${shellQuote(`${part.name}=${value}${suffix}`)}`);
    }
  } else if (req.body_kind === "graphql" && safeGraphql && req.method === "POST") {
    lines.push(`  --header ${shellQuote("Content-Type: application/json")}`);
    lines.push(`  --data ${shellQuote(safeGraphqlBody)}`);
  } else if (req.body_kind !== "none" && req.body_kind !== "graphql" && req.body) {
    lines.push(`  --data ${shellQuote(req.body)}`);
  }

  return lines.join(" \\\n");
}

export function safeRequestError(cause: unknown): string {
  if (
    (typeof DOMException !== "undefined" && cause instanceof DOMException && cause.name === "AbortError") ||
    (cause instanceof Error && cause.name === "AbortError")
  ) {
    return "요청이 취소되었습니다";
  }
  const raw = cause instanceof Error ? cause.message : typeof cause === "string" ? cause : "";
  const message = raw.replace(/^Error:\s*/, "");
  const safeMessages = [
    "multipart는 최대 50개 part까지 사용할 수 있습니다.",
    "part 이름이 필요합니다.",
    "part 이름은 120자 이하의 HTTP token이어야 합니다.",
    "Content-Type은 type/subtype 형식이어야 합니다.",
    "전송할 파일을 선택하세요.",
    "선택한 파일 경로가 올바르지 않습니다.",
    "활성 text part 전체는 UTF-8 기준 1,000,000바이트 이하여야 합니다.",
    "multipart 파일 전송은 데스크톱 앱에서만 사용할 수 있습니다",
    "part별 Content-Type 전송은 데스크톱 앱에서만 사용할 수 있습니다",
    "선택한 multipart 파일을 찾을 수 없습니다",
    "선택한 multipart 파일을 읽을 수 없습니다",
    "multipart 파일은 각각 25 MiB 이하여야 합니다",
    "multipart 파일 전체는 50 MiB 이하여야 합니다",
    "요청 시간이 초과되었습니다",
    "요청이 취소되었습니다",
    "GraphQL 요청 구성이 올바르지 않습니다",
    "GraphQL endpoint URL이 올바르지 않습니다",
    "GraphQL endpoint query에 credential을 넣을 수 없습니다",
    "GraphQL query가 허용된 크기를 초과했습니다",
    "GraphQL variables가 허용된 크기를 초과했습니다",
    "GraphQL 문서 형식이 올바르지 않습니다",
    "GraphQL operation 선택이 올바르지 않습니다",
    "GraphQL variables는 유효한 JSON object여야 합니다",
    "GraphQL variables 구조가 허용된 한계를 초과했습니다",
    "GraphQL 요청 본문이 허용된 크기를 초과했습니다",
    "GraphQL introspection 요청은 지원하지 않습니다",
    "GraphQL subscription은 지원하지 않습니다",
    "GraphQL timeout이 허용된 범위를 벗어났습니다",
    "GraphQL header 행 수가 허용된 한계를 초과했습니다",
    "GraphQL header 크기가 허용된 한계를 초과했습니다",
    GRAPHQL_HEADER_ROWS_ERROR,
    GRAPHQL_HEADER_BYTES_ERROR,
    GRAPHQL_URL_TOO_LARGE,
    "GraphQL 리다이렉트를 브라우저 미리보기에서 처리할 수 없습니다",
    "응답 본문이 허용된 크기를 초과했습니다",
    "SSE 연결 timeout 범위가 올바르지 않습니다.",
    "SSE idle timeout 범위가 올바르지 않습니다.",
    "SSE 전체 timeout 범위가 올바르지 않습니다.",
    "SSE 환경 변수는 최대 100개까지 사용할 수 있습니다.",
    "SSE 환경 변수 형식이 올바르지 않습니다.",
    "SSE stream은 GET 또는 POST만 지원합니다.",
    "SSE 요청 URL이 너무 깁니다",
    "SSE 요청 URL이 올바르지 않습니다",
    "SSE 요청 항목 수가 제한을 초과했습니다",
    "SSE 요청 항목 수 또는 URL이 제한을 초과했습니다.",
    "SSE 요청 본문이 너무 큽니다",
    "SSE 요청 header가 너무 깁니다",
    "SSE 요청 Cookie가 너무 깁니다",
    "SSE 요청 parameter가 너무 깁니다",
    "SSE 인증 설정이 너무 깁니다",
    "SSE 인증 설정이 올바르지 않습니다",
    "SSE 요청 header가 올바르지 않습니다",
    "SSE multipart 파일 경로가 너무 깁니다",
    "SSE 요청 본문 형식이 올바르지 않습니다",
    "SSE 응답 형식이 아닙니다",
    "SSE 요청을 보낼 수 없습니다",
    "SSE 리다이렉트 정책으로 요청을 차단했습니다",
    "SSE multipart 파일 전송은 데스크톱 앱에서만 사용할 수 있습니다.",
    "SSE multipart part별 Content-Type은 데스크톱 앱에서만 사용할 수 있습니다.",
    "GET SSE 요청에는 본문을 사용할 수 없습니다",
    "secret 포함 SSE stream은 데스크톱 앱에서만 사용할 수 있습니다.",
    "SSE stream 시간이 초과되었습니다",
    "SSE stream 연결에 실패했습니다",
    "SSE stream 데이터가 올바르지 않습니다",
    "SSE stream을 시작하지 못했습니다.",
  ];
  if (safeMessages.includes(message) || /^'.+' 파일을 다시 선택하세요\.$/.test(message)) {
    return message;
  }
  return "요청에 실패했습니다. URL, 연결 상태와 secret 설정을 확인하세요.";
}

export const SAFE_WEBSOCKET_UI_MESSAGES = new Set([
  "WebSocket endpoint URL이 올바르지 않습니다",
  "WebSocket endpoint query에 credential을 넣을 수 없습니다",
  "WebSocket 요청 URL이 너무 깁니다",
  "WebSocket 요청 header가 올바르지 않습니다",
  "WebSocket 요청 header가 너무 깁니다",
  "WebSocket 요청 parameter가 올바르지 않습니다",
  "WebSocket 요청 항목 수가 제한을 초과했습니다",
  "WebSocket 연결 timeout 범위가 올바르지 않습니다",
  "WebSocket 인증 설정이 올바르지 않습니다",
  "secret 포함 WebSocket은 데스크톱 앱에서만 전송할 수 있습니다",
  "브라우저 미리보기에서는 WebSocket custom header/auth를 사용할 수 없습니다",
  "브라우저 미리보기에서는 ping/pong을 직접 보낼 수 없습니다",
  "WebSocket 연결을 시작할 수 없습니다",
  "이미 실행 중인 WebSocket 연결이 있습니다",
  "WebSocket 연결이 열려 있지 않습니다",
  "WebSocket message를 보낼 수 없습니다",
  "WebSocket ping을 보낼 수 없습니다",
  "WebSocket 연결을 닫을 수 없습니다",
  "WebSocket binary payload가 올바르지 않습니다",
  "WebSocket binary를 안전하게 저장할 수 없습니다",
  "WebSocket binary hex가 올바르지 않습니다",
  "WebSocket message가 허용된 크기를 초과했습니다",
  "WebSocket close code가 올바르지 않습니다",
  "WebSocket close reason이 올바르지 않습니다",
  "WebSocket 연결 시간이 초과되었습니다",
  "WebSocket 연결에 실패했습니다",
  "WebSocket 연결이 끊어졌습니다",
  "WebSocket 요청에 실패했습니다.",
]);

export function safeWebSocketUiError(cause: unknown): string {
  const raw = cause instanceof Error ? cause.message : typeof cause === "string" ? cause : "";
  const message = raw.replace(/^Error:\s*/u, "");
  return SAFE_WEBSOCKET_UI_MESSAGES.has(message) ? message : "WebSocket 요청에 실패했습니다.";
}

export function formatHandoffExpiry(expiresAtMs: number): string {
  const date = new Date(expiresAtMs);
  return Number.isFinite(date.getTime()) ? date.toLocaleString() : "시간 미상";
}

export function safeHandoffError(cause: unknown): string {
  const message = cause instanceof Error ? cause.message : typeof cause === "string" ? cause : "";
  const safeMessages = new Set([
    "handoff 요청을 사용할 수 없습니다",
    "handoff 요청이 만료되었거나 더 이상 사용할 수 없습니다",
    "handoff 요청이 다른 작업에서 사용 중입니다. 잠시 후 다시 시도하세요",
    "handoff 저장소를 사용할 수 없습니다",
    "handoff 미리보기가 만료되었습니다. 다시 전달하세요",
    "지원하지 않는 handoff 요청입니다",
    "기존 handoff 미리보기를 먼저 적용하거나 취소하세요",
    "API Playground handoff는 데스크톱 앱에서만 사용할 수 있습니다. 클립보드로 자동 전환하지 않습니다",
  ]);
  return safeMessages.has(message) ? message : "handoff 요청을 처리하지 못했습니다";
}

export function isTerminalHandoffError(message: string): boolean {
  return (
    message === "handoff 요청을 사용할 수 없습니다" ||
    message === "handoff 요청이 만료되었거나 더 이상 사용할 수 없습니다" ||
    message === "handoff 미리보기가 만료되었습니다. 다시 전달하세요"
  );
}

export function shellQuote(s: string): string {
  return `'${s.replace(/'/g, `'\\''`)}'`;
}

/** curl -F의 쉼표/세미콜론/@ 및 quote parsing과 shell parsing을 분리한다. */
export function curlFormQuote(value: string): string {
  return `"${value.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`;
}

export async function copyRevealedCurl(
  req: RequestTemplate,
  environment: Parameters<typeof buildRevealedCurl>[1],
  setError: (message: string | null) => void,
): Promise<void> {
  const confirmed = window.confirm(
    "원문 cURL에는 Authorization, Cookie, API key와 secret 값이 포함될 수 있습니다. 클립보드에 한 번 복사할까요?",
  );
  if (!confirmed) return;
  try {
    const revealed = await buildRevealedCurl(req, environment);
    await navigator.clipboard.writeText(revealed);
  } catch {
    setError("원문 cURL을 안전하게 만들거나 복사하지 못했습니다.");
  }
}
