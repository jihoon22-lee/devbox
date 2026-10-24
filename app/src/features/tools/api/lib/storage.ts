/**
 * 저장 정화: v0.9.0 `persistence.ts`의 `sanitizeRequestForPersistence` 계열만 옮겼다(localStorage 영속·이전 형식 읽기는 이식하지 않음).
 * 코드 생성·가져오기 미리보기는 `sanitizeRequestForPersistence`를, 파일 저장은 같은 규칙 위의 `sanitizeForStorage`를 쓴다.
 */

import { visit } from "jsonc-parser";
import type { Auth, CollectionRoot } from "../../../../rpc/gen/rpc";
import type { StoredBody, StoredPart, StoredRequest, StoredRow } from "../model/stored";
import { normalizeCookies } from "./cookies";
import { maskGraphqlQueryLiterals } from "./graphql/mask";
import { isHeaderEnabled, normalizeHeaders } from "./headers";
import { MAX_MULTIPART_PARTS, normalizeMultipartParts, safeMultipartFileName } from "./multipart";
import { isExactVariableReference } from "./references";
import { isSensitiveName, redactKnownTokenPatterns } from "./secretPatterns";
import type {
  AuthConfig,
  GraphqlRequest,
  KeyValue,
  MultipartPart,
  OAuth2Config,
  PersistedRequest,
  RequestTemplate,
  RequestTls,
} from "./template";

export { isSensitiveName } from "./secretPatterns";
export const REDACTED = "[REDACTED]";

export function normalizeRequestTls(value: unknown): RequestTls | undefined {
  if (!value || typeof value !== "object") return undefined;
  const tls = value as Partial<RequestTls>;
  return {
    credentialId:
      typeof tls.credentialId === "string" && /^[a-f0-9]{32}$/.test(tls.credentialId) ? tls.credentialId : null,
    verify: tls.verify !== false,
  };
}

export function hasCustomTls(tls: RequestTls | null | undefined): boolean {
  return Boolean(tls?.credentialId) || tls?.verify === false;
}

export const emptyOAuth2: OAuth2Config = {
  grantType: "authorizationCode",
  authorizationUrl: "",
  tokenUrl: "",
  clientId: "",
  clientSecret: "",
  scopes: "",
};
function bounded(value: unknown, max: number): string {
  if (typeof value !== "string") return "";
  let result = "",
    bytes = 0;
  const encoder = new TextEncoder();
  for (const character of value) {
    bytes += encoder.encode(character).length;
    if (bytes > max) break;
    result += character;
  }
  return result;
}
export function normalizeOAuth2(value: unknown): OAuth2Config | undefined {
  if (!value || typeof value !== "object") return undefined;
  const config = value as Partial<OAuth2Config>;
  if (config.grantType !== "authorizationCode" && config.grantType !== "clientCredentials") return undefined;
  return {
    grantType: config.grantType,
    authorizationUrl: bounded(config.authorizationUrl, 8192),
    tokenUrl: bounded(config.tokenUrl, 8192),
    clientId: bounded(config.clientId, 8192),
    clientSecret: bounded(config.clientSecret, 65536),
    scopes: bounded(config.scopes, 32768),
  };
}

export function sanitizeRequestForPersistence(request: RequestTemplate): PersistedRequest {
  let requiresSecretReview = false;
  const mark = (original: string, sanitized: string) => {
    if (original !== sanitized) requiresSecretReview = true;
    return sanitized;
  };

  const headers = normalizeHeaders(request.headers).map((header) => ({
    ...sanitizePair(header, mark),
    enabled: isHeaderEnabled(header),
  }));
  const cookies = normalizeCookies(request.cookies).map((cookie) => ({
    ...cookie,
    value: mark(
      cookie.value,
      cookie.value && !isExactVariableReference(cookie.value) ? REDACTED : redactKnownTokenPatterns(cookie.value),
    ),
  }));
  const params = request.params.map((param) => sanitizePair(param, mark));
  const url = mark(request.url, sanitizeUrl(request.url));
  const body = mark(
    request.body,
    ["multipart", "graphql"].includes(request.body_kind) ? "" : sanitizeBody(request.body, request.body_kind),
  );
  const sourceMultipart = request.multipart ?? [];
  if (sourceMultipart.length > MAX_MULTIPART_PARTS) requiresSecretReview = true;
  const multipart = normalizeMultipartParts(sourceMultipart).map((part, index) =>
    sanitizeMultipartPart(part, sourceMultipart[index] ?? part, mark),
  );
  const auth = request.auth ? sanitizeAuth(request.auth, mark) : null;
  const graphql =
    request.body_kind === "graphql" && request.graphql ? sanitizeGraphqlRequest(request.graphql, mark) : undefined;

  return {
    method: request.method,
    url,
    headers,
    cookies,
    multipart,
    params,
    body_kind: request.body_kind,
    body,
    auth,
    timeout_ms: request.timeout_ms,
    ...(graphql ? { graphql } : {}),
    requiresSecretReview,
    ...(request.tls ? { tls: normalizeRequestTls(request.tls) } : {}),
  };
}

export function containsReference(value: string): boolean {
  return /\{\{\s*[a-zA-Z0-9_.-]+\s*\}\}|\$\{\s*[a-zA-Z0-9_.-]+\s*\}/.test(value);
}

/** `Bearer {{secret:T}}`처럼 인증 scheme 뒤가 참조 하나뿐인 값(데몬 기록의 "scheme+참조 유지"와 같은 규칙). */
function isSchemeReference(value: string): boolean {
  const match = /^(?:Bearer|Basic|Token|Digest|ApiKey)[ \t]+(.+)$/i.exec(value);
  return !!match && isExactVariableReference(match[1]!);
}

function sanitizePair(pair: KeyValue, mark: (original: string, sanitized: string) => string): KeyValue {
  const sensitive = isSensitiveName(pair.key);
  const value =
    sensitive && pair.value && !isExactVariableReference(pair.value) && !isSchemeReference(pair.value)
      ? REDACTED
      : redactKnownTokenPatterns(pair.value);
  return { key: pair.key, value: mark(pair.value, value) };
}

function sanitizeAuth(auth: AuthConfig, mark: (original: string, sanitized: string) => string): AuthConfig {
  const secretField = (value: string) =>
    mark(value, value && !isExactVariableReference(value) ? REDACTED : redactKnownTokenPatterns(value));
  const config = normalizeOAuth2(auth.oauth2);
  return {
    kind: auth.kind,
    username: secretField(auth.username),
    password: secretField(auth.password),
    token: secretField(auth.token),
    api_key: mark(auth.api_key, redactKnownTokenPatterns(auth.api_key)),
    api_value: secretField(auth.api_value),
    ...(config
      ? {
          oauth2: {
            grantType: config.grantType,
            authorizationUrl: mark(config.authorizationUrl, sanitizeUrl(config.authorizationUrl)),
            tokenUrl: mark(config.tokenUrl, sanitizeUrl(config.tokenUrl)),
            clientId: mark(config.clientId, redactKnownTokenPatterns(config.clientId)),
            clientSecret: secretField(config.clientSecret),
            scopes: mark(config.scopes, redactKnownTokenPatterns(config.scopes)),
          },
        }
      : {}),
  };
}

function sanitizeMultipartPart(
  part: MultipartPart,
  original: MultipartPart,
  mark: (original: string, sanitized: string) => string,
): MultipartPart {
  if (part.kind === "file") {
    const safeName = safeMultipartFileName(original.file_name || original.file_path);
    return {
      ...part,
      value: mark(original.value, ""),
      file_path: mark(original.file_path, ""),
      file_name: mark(original.file_name, redactKnownTokenPatterns(safeName)),
    };
  }
  const safeValue =
    isSensitiveName(part.name) && part.value
      ? isExactVariableReference(part.value)
        ? part.value
        : REDACTED
      : redactKnownTokenPatterns(part.value);
  mark(original.file_path, "");
  mark(original.file_name, "");
  return {
    ...part,
    value: mark(part.value, safeValue),
    file_path: "",
    file_name: "",
  };
}

/**
 * 정화가 아무것도 가리지 않았으면 사용자가 쓴 원문을 돌려준다(형식만 바꾼 재직렬화로 원문을 바꾸지 않음).
 * 단 원문 글자 자체에 토큰 모양이 없고, 중복 키처럼 파서가 버린 부분이 없을 때만이다(보안 0, parser differential).
 */
function unchangedOriginal(original: string, clean: string, normalized: string): boolean {
  return clean === normalized && !hasDuplicateKeys(original) && redactKnownTokenPatterns(original) === original;
}

function sanitizeUrl(value: string): string {
  try {
    const url = new URL(value);
    const before = url.toString();
    for (const key of new Set(url.searchParams.keys())) {
      // 같은 키가 여러 번이면 모든 값을 본다(`get`은 첫 값만 돌려준다).
      const values = url.searchParams.getAll(key);
      if (isSensitiveName(key) && values.some((value) => !isExactVariableReference(value)))
        url.searchParams.set(key, REDACTED);
    }
    if (url.username && !isExactVariableReference(url.username)) url.username = "REDACTED";
    if (url.password && !isExactVariableReference(url.password)) url.password = "REDACTED";
    const after = url.toString();
    // 가린 것이 없으면 원문 그대로(정규화만 한 URL로 바꾸지 않음). 원문의 토큰 모양은 여전히 가린다.
    return redactKnownTokenPatterns(after === before ? value : after);
  } catch {
    const withoutCredentials = value.replace(
      /^([a-z][a-z0-9+.-]*:\/\/)([^/@]+)@/i,
      (_match, scheme: string, credentials: string) =>
        isExactVariableReference(credentials) ? `${scheme}${credentials}@` : `${scheme}REDACTED:REDACTED@`,
    );
    const safeQuery = withoutCredentials.replace(
      /([?&])([^=&#]+)=([^&#]*)/g,
      (_match, separator: string, rawKey: string, rawValue: string) => {
        const key = decodeURIComponentSafely(rawKey);
        const value = isSensitiveName(key) && !isExactVariableReference(rawValue) ? REDACTED : rawValue;
        return `${separator}${rawKey}=${value}`;
      },
    );
    return redactKnownTokenPatterns(safeQuery);
  }
}

function sanitizeBody(body: string, kind: string): string {
  if (!body) return body;
  if (kind === "json") {
    try {
      const parsed = JSON.parse(body) as unknown;
      // 키 이름에 있는 토큰도 가린다(값만 보던 v0.9.0 누락).
      const clean = redactKnownTokenPatterns(JSON.stringify(sanitizeJsonValue(parsed)));
      return unchangedOriginal(body, clean, JSON.stringify(parsed)) ? body : clean;
    } catch {
      return redactKnownTokenPatterns(redactMalformedJsonFields(body));
    }
  }
  if (kind === "form") {
    return body
      .split("\n")
      .map((line) => {
        const parts = line.split("=");
        if (parts.length < 2 || !isSensitiveName(parts[0]!.trim())) return redactKnownTokenPatterns(line);
        const raw = parts.slice(1).join("=");
        return `${parts[0]}=${isExactVariableReference(raw) ? raw : REDACTED}`;
      })
      .join("\n");
  }
  return redactKnownTokenPatterns(body);
}

function sanitizeGraphqlRequest(
  request: GraphqlRequest,
  mark: (original: string, sanitized: string) => string,
): GraphqlRequest {
  const query = maskGraphqlQueryLiterals(request.query);
  let variables = request.variables;
  if (variables.trim()) {
    try {
      if (hasDuplicateKeys(variables)) variables = REDACTED;
      else {
        const parsed = JSON.parse(variables) as unknown;
        const clean = JSON.stringify(sanitizeGraphqlVariables(parsed));
        variables = unchangedOriginal(variables, clean, JSON.stringify(parsed)) ? variables : clean;
      }
    } catch {
      variables = REDACTED;
    }
  }
  return {
    query: mark(request.query, redactKnownTokenPatterns(query)),
    variables: mark(request.variables, redactKnownTokenPatterns(variables)),
    operation_name: mark(request.operation_name, redactKnownTokenPatterns(request.operation_name)),
  };
}

function sanitizeGraphqlVariables(value: unknown, key = ""): unknown {
  if (isSensitiveName(key)) {
    return typeof value === "string" && isExactVariableReference(value) ? value : REDACTED;
  }
  if (Array.isArray(value)) return value.map((item) => sanitizeGraphqlVariables(item));
  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value as Record<string, unknown>).map(([childKey, child]) => [
        childKey,
        sanitizeGraphqlVariables(child, childKey),
      ]),
    );
  }
  return typeof value === "string" ? redactKnownTokenPatterns(value) : value;
}

function sanitizeJsonValue(value: unknown, key = ""): unknown {
  if (isSensitiveName(key)) {
    return typeof value === "string" && isExactVariableReference(value) ? value : REDACTED;
  }
  if (Array.isArray(value)) return value.map((item) => sanitizeJsonValue(item));
  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value as Record<string, unknown>).map(([childKey, child]) => [
        childKey,
        sanitizeJsonValue(child, childKey),
      ]),
    );
  }
  return typeof value === "string" ? redactKnownTokenPatterns(value) : value;
}

function redactMalformedJsonFields(value: string): string {
  return value.replace(/"([^"]+)"\s*:\s*"([^"]*)"/g, (match, key: string, raw: string) =>
    isSensitiveName(key) && !isExactVariableReference(raw) ? match.replace(raw, REDACTED) : match,
  );
}

function decodeURIComponentSafely(value: string): string {
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
}

/** 파일 저장 정화 결과. `masked`는 가린 필드 위치(`StoredRequest` 기준 경로, 예 `headers.0.value`). */
export interface StorageResult {
  stored: StoredRequest;
  masked: string[];
}

/** 형식만 다른 값(정규화한 URL·다시 쓴 JSON)은 가린 것이 아니다. */
function normalizedUrl(value: string): string | undefined {
  try {
    return new URL(value).toString();
  } catch {
    return undefined;
  }
}
function normalizedJson(value: string): string | undefined {
  if (hasDuplicateKeys(value)) return undefined;
  try {
    return JSON.stringify(JSON.parse(value));
  } catch {
    return undefined;
  }
}

/** `JSON.parse`는 중복 키의 마지막 값만 본다. 정화가 보지 못한 앞 값이 원문에 남지 않게 따로 찾는다. */
export function hasDuplicateKeys(text: string): boolean {
  const stack: Set<string>[] = [];
  let duplicate = false;
  visit(text, {
    onObjectBegin: () => {
      stack.push(new Set());
    },
    onObjectEnd: () => {
      stack.pop();
    },
    onObjectProperty: (property) => {
      const keys = stack.at(-1);
      if (!keys) return;
      if (keys.has(property)) duplicate = true;
      keys.add(property);
    },
  });
  return duplicate;
}

/**
 * 모든 파일 저장(저장·새 요청으로 저장·가져오기·웹훅에서 만든 요청)이 지나는 정화(01 §9.1-14).
 * 직접 입력한 민감 값만 `[REDACTED]`로 바꾸고 `{{secret:NAME}}`·`{{name}}` 참조는 둔다.
 * 정화로 바뀐 필드는 모두 가린 위치로 기록한다. 형식만 바뀐 필드는 사용자가 쓴 글자 그대로 둔다.
 * 프로젝트 루트는 multipart 파일·binary 본문의 로컬 경로를 지운다(가림 수에는 넣지 않음).
 */
export function sanitizeForStorage(stored: StoredRequest, root: CollectionRoot): StorageResult {
  const masked: string[] = [];
  const mark: Mark = (path, original, sanitized, normalize) => {
    if (sanitized === original) return original;
    // 형식만 다르면 원문을 둔다. 단 원문 글자 자체에 토큰 모양이 없고 정규화가 원문 전체를 본 경우만
    // (중복 키처럼 파서가 버린 부분이 있으면 `normalize`가 undefined다).
    if (normalize && redactKnownTokenPatterns(original) === original && sanitized === normalize(original))
      return original;
    masked.push(path);
    return sanitized;
  };
  const secretField = (path: string, value: string) =>
    mark(path, value, value && !isExactVariableReference(value) ? REDACTED : redactKnownTokenPatterns(value));
  const pair = (path: string) => (r: StoredRow, index: number) => ({
    ...r,
    value: mark(`${path}.${index}.value`, r.value, sanitizePair({ key: r.name, value: r.value }, keep).value),
  });
  const url = mark("url", stored.url, sanitizeUrl(stored.url), normalizedUrl);
  const query = stored.query.map(pair("query"));
  const headers = stored.headers.map(pair("headers"));
  const cookies = stored.cookies.map((r, index) => ({ ...r, value: secretField(`cookies.${index}.value`, r.value) }));
  const shared = root.kind === "project";
  const body = sanitizeStoredBody(stored.body, shared, mark, pair);
  const auth = sanitizeStoredAuth(stored.auth, mark, secretField);
  return {
    stored: { ...stored, url, query, headers, cookies, body, auth, tls: { ...stored.tls } },
    masked,
  };
}

type Mark = (
  path: string,
  original: string,
  sanitized: string,
  normalize?: (value: string) => string | undefined,
) => string;
const keep = (_original: string, sanitized: string) => sanitized;

/** Keep received wire text intact until storage, then apply the matching structured-body sanitizer. */
function sanitizeRawText(text: string, contentType: string | null): string {
  const mime = contentType?.split(";", 1)[0]?.trim().toLowerCase() ?? "";
  if (mime.endsWith("/json") || mime.endsWith("+json")) return sanitizeBody(text, "json");
  if (mime !== "application/x-www-form-urlencoded") return redactKnownTokenPatterns(text);
  return text
    .split("&")
    .map((field) => {
      const entry = new URLSearchParams(field).entries().next().value;
      if (!entry) return field;
      const [name, value] = entry;
      const safe =
        isSensitiveName(name) && value && !isExactVariableReference(value) ? REDACTED : redactKnownTokenPatterns(value);
      return safe === value ? field : `${field.split("=", 1)[0]}=${encodeURIComponent(safe)}`;
    })
    .join("&");
}

function sanitizeStoredBody(
  body: StoredBody,
  shared: boolean,
  mark: Mark,
  pair: (path: string) => (r: StoredRow, index: number) => StoredRow,
): StoredBody {
  switch (body.kind) {
    case "none":
      return body;
    case "raw":
      if (body.encoding === "base64")
        return { ...body, text: mark("body.text", body.text, btoa(redactKnownTokenPatterns(atob(body.text)))) };
      return { ...body, text: mark("body.text", body.text, sanitizeRawText(body.text, body.contentType)) };
    case "json":
      return { ...body, text: mark("body.text", body.text, sanitizeBody(body.text, "json"), normalizedJson) };
    case "form":
      return { ...body, fields: body.fields.map(pair("body.fields")) };
    case "binary":
      return { ...body, path: shared ? "" : body.path };
    case "graphql": {
      const clean = sanitizeGraphqlRequest(
        { query: body.query, variables: body.variables, operation_name: body.operationName ?? "" },
        keep,
      );
      return {
        ...body,
        query: mark("body.query", body.query, clean.query),
        variables: mark("body.variables", body.variables, clean.variables, normalizedJson),
        operationName:
          body.operationName === null ? null : mark("body.operationName", body.operationName, clean.operation_name),
      };
    }
    case "multipart":
      return {
        ...body,
        parts: body.parts.map((part, index): StoredPart => {
          if (part.kind === "file") {
            if (!shared) return { ...part };
            return { ...part, path: "", fileName: safeMultipartFileName(part.fileName || part.path) || null };
          }
          const value = sanitizePair({ key: part.name, value: part.value }, keep).value;
          return { ...part, value: mark(`body.parts.${index}.value`, part.value, value) };
        }),
      };
  }
}

function sanitizeStoredAuth(auth: Auth, mark: Mark, secretField: (path: string, value: string) => string): Auth {
  const tokens = (path: string, value: string) => mark(path, value, redactKnownTokenPatterns(value));
  const url = (path: string, value: string) => mark(path, value, sanitizeUrl(value), normalizedUrl);
  switch (auth.kind) {
    case "none":
      return auth;
    case "basic":
      return {
        ...auth,
        username: secretField("auth.username", auth.username),
        password: secretField("auth.password", auth.password),
      };
    case "bearer":
      return { ...auth, token: secretField("auth.token", auth.token) };
    case "apiKey":
      return { ...auth, name: tokens("auth.name", auth.name), value: secretField("auth.value", auth.value) };
    case "oauth2ClientCredentials":
      return {
        ...auth,
        tokenUrl: url("auth.tokenUrl", auth.tokenUrl),
        clientId: tokens("auth.clientId", auth.clientId),
        clientSecret: secretField("auth.clientSecret", auth.clientSecret),
        scope: auth.scope === null ? null : tokens("auth.scope", auth.scope),
      };
    case "oauth2AuthCode":
      return {
        ...auth,
        authorizationUrl: url("auth.authorizationUrl", auth.authorizationUrl),
        tokenUrl: url("auth.tokenUrl", auth.tokenUrl),
        clientId: tokens("auth.clientId", auth.clientId),
        clientSecret: auth.clientSecret === null ? null : secretField("auth.clientSecret", auth.clientSecret),
        scope: auth.scope === null ? null : tokens("auth.scope", auth.scope),
      };
  }
}

/** 경로(`headers.0.value`)의 문자열 값. 없으면 `undefined`. */
export function valueAt(root: unknown, path: string): string | undefined {
  let node = root;
  for (const key of path.split(".")) {
    if (!node || typeof node !== "object" || !Object.hasOwn(node, key)) return undefined;
    node = (node as Record<string, unknown>)[key];
  }
  return typeof node === "string" ? node : undefined;
}

/** 기록한 가린 위치 중 아직 가린 값이 남은 것. 사용자가 다른 곳에 쓴 `[REDACTED]`는 보지 않는다. */
export function stillMasked(stored: StoredRequest, masked: readonly string[]): string[] {
  return masked.filter((path) => valueAt(stored, path)?.includes("REDACTED"));
}
