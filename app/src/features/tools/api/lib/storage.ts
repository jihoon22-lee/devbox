/**
 * 저장 정화: v0.9.0 `persistence.ts`의 `sanitizeRequestForPersistence` 계열만 옮겼다(localStorage 영속·이전 형식 읽기는 이식하지 않음).
 * 코드 생성·가져오기 미리보기가 쓴다. 파일 저장용 `sanitizeForStorage(stored, root)`는 S5-3 3c에서 이 위에 더한다.
 */

import { normalizeCookies } from "./cookies";
import { maskGraphqlQueryLiterals } from "./graphql/mask";
import { isHeaderEnabled, normalizeHeaders } from "./headers";
import { MAX_MULTIPART_PARTS, normalizeMultipartParts, safeMultipartFileName } from "./multipart";
import { isExactVariableReference } from "./references";
import { isSensitiveName } from "./secretPatterns";
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

function sanitizePair(pair: KeyValue, mark: (original: string, sanitized: string) => string): KeyValue {
  const sensitive = isSensitiveName(pair.key);
  const value =
    sensitive && pair.value && !isExactVariableReference(pair.value) ? REDACTED : redactKnownTokenPatterns(pair.value);
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

function sanitizeUrl(value: string): string {
  try {
    const url = new URL(value);
    for (const key of [...url.searchParams.keys()]) {
      const current = url.searchParams.get(key) ?? "";
      if (isSensitiveName(key) && !isExactVariableReference(current)) url.searchParams.set(key, REDACTED);
    }
    if (url.username && !isExactVariableReference(url.username)) url.username = "REDACTED";
    if (url.password && !isExactVariableReference(url.password)) url.password = "REDACTED";
    return redactKnownTokenPatterns(url.toString());
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
      return JSON.stringify(sanitizeJsonValue(parsed));
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
      variables = JSON.stringify(sanitizeGraphqlVariables(JSON.parse(variables) as unknown));
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

function redactKnownTokenPatterns(value: string): string {
  if (!value) return value;
  if (/-----BEGIN [A-Z ]*PRIVATE KEY-----/.test(value)) return REDACTED;
  let output = value;
  const candidates = value.split(/[\s"'=:,&]+/).filter(Boolean);
  for (const candidate of candidates) {
    if (looksLikeStoredSecret(candidate)) output = output.split(candidate).join(REDACTED);
  }
  return output;
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

function looksLikeStoredSecret(value: string): boolean {
  if (/^(?:sk[_-]|ghp_|github_pat_|glpat-|xox[baprs]-)[A-Za-z0-9_.-]{12,}$/.test(value)) return true;
  if (/^AKIA[A-Z0-9]{16}$/.test(value)) return true;
  const jwt = value.split(".");
  return jwt.length === 3 && jwt.every((part) => part.length >= 10 && /^[A-Za-z0-9_-]+$/.test(part));
}
