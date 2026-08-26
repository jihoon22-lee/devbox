import type {
  AuthConfig,
  HistoryItem,
  KeyValue,
  PersistedHistoryRequest,
  RequestTemplate,
} from "../types";
import {
  isHeaderEnabled,
  isRequestHeader,
  normalizeHeaders,
} from "./headers";
import { isCookieReference, isRequestCookie, normalizeCookies } from "./cookies";

export const REDACTED = "[REDACTED]";
export const HISTORY_V1_LS_KEY = "apip-history";
export const HISTORY_V2_LS_KEY = "apip-history-v2";
export const HISTORY_V1_MARKER_KEY = "apip-history-v1-migrated";
export const HISTORY_VERSION = 2;

export interface HistoryStore {
  version: 2;
  history: HistoryItem[];
}

export interface StorageMigration<T> {
  store: T;
  migrated: boolean;
  failed: boolean;
  removedLegacyEntries: number;
}

export type PersistenceSanitizer = (serialized: string) => Promise<string>;

export function emptyHistoryStore(): HistoryStore {
  return { version: HISTORY_VERSION, history: [] };
}

/**
 * v1 History는 평문 포함 여부를 증명할 수 없으므로 읽거나 변환하지 않는다.
 * 빈 v2 저장소가 read-back 된 뒤에만 raw key를 지우고 marker를 기록한다.
 */
export function migrateHistoryStorage(storage: Storage = localStorage): StorageMigration<HistoryStore> {
  const raw = storage.getItem(HISTORY_V1_LS_KEY);
  const removedLegacyEntries = countLegacyHistoryEntries(raw);
  try {
    let store = parseHistoryStore(storage.getItem(HISTORY_V2_LS_KEY));
    if (!store) {
      store = emptyHistoryStore();
      storage.setItem(HISTORY_V2_LS_KEY, JSON.stringify(store));
      if (!parseHistoryStore(storage.getItem(HISTORY_V2_LS_KEY))) {
        throw new Error("v2 history read-back failed");
      }
    }

    if (raw !== null) {
      storage.removeItem(HISTORY_V1_LS_KEY);
      if (storage.getItem(HISTORY_V1_LS_KEY) !== null) {
        throw new Error("legacy history deletion failed");
      }
    }
    if (storage.getItem(HISTORY_V1_MARKER_KEY) !== "2") {
      storage.setItem(HISTORY_V1_MARKER_KEY, "2");
      if (storage.getItem(HISTORY_V1_MARKER_KEY) !== "2") {
        throw new Error("history marker write failed");
      }
    }
    return { store, migrated: raw !== null, failed: false, removedLegacyEntries };
  } catch {
    // raw v1은 어떤 실패에서도 반환하지 않아 UI·검색·재전송과 격리한다.
    return { store: emptyHistoryStore(), migrated: false, failed: true, removedLegacyEntries: 0 };
  }
}

export async function saveHistoryStore(
  store: HistoryStore,
  sanitize: PersistenceSanitizer,
  storage: Storage = localStorage,
): Promise<HistoryStore> {
  const original = JSON.stringify(store);
  const sanitized = await sanitize(original);
  const parsedCandidate = parseHistoryStore(sanitized);
  const parsed = parsedCandidate && sanitized !== original
    ? {
        ...parsedCandidate,
        history: parsedCandidate.history.map((item) => ({
          ...item,
          request: { ...item.request, requiresSecretReview: true },
        })),
      }
    : parsedCandidate;
  if (!parsed) throw new Error("안전한 History 형식이 아닙니다");
  storage.setItem(HISTORY_V2_LS_KEY, JSON.stringify(parsed));
  const readBack = parseHistoryStore(storage.getItem(HISTORY_V2_LS_KEY));
  if (!readBack) throw new Error("History 안전 저장을 확인할 수 없습니다");
  return readBack;
}

export function parseHistoryStore(raw: string | null): HistoryStore | null {
  try {
    const parsed = JSON.parse(raw ?? "null") as Partial<HistoryStore> | null;
    if (parsed?.version !== HISTORY_VERSION || !Array.isArray(parsed.history)) return null;
    const history = parsed.history
      .filter(isHistoryItem)
      .slice(0, 50)
      .map((item) => ({
        ...item,
        request: normalizePersistedRequest(item.request),
      }));
    return { version: HISTORY_VERSION, history };
  } catch {
    return null;
  }
}

export function sanitizeRequestForPersistence(request: RequestTemplate): PersistedHistoryRequest {
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
      cookie.value && !isCookieReference(cookie.value)
        ? REDACTED
        : redactKnownTokenPatterns(cookie.value),
    ),
  }));
  const params = request.params.map((param) => sanitizePair(param, mark));
  const url = mark(request.url, sanitizeUrl(request.url));
  const body = mark(request.body, sanitizeBody(request.body, request.body_kind));
  const auth = request.auth ? sanitizeAuth(request.auth, mark) : null;

  return {
    method: request.method,
    url,
    headers,
    cookies,
    params,
    body_kind: request.body_kind,
    body,
    auth,
    timeout_ms: request.timeout_ms,
    requiresSecretReview,
  };
}

export function toRequestTemplate(request: PersistedHistoryRequest): RequestTemplate {
  const { requiresSecretReview: _, ...template } = request;
  return {
    ...template,
    headers: normalizeHeaders(template.headers),
    cookies: normalizeCookies(template.cookies),
  };
}

export function normalizePersistedRequest(
  request: PersistedHistoryRequest,
): PersistedHistoryRequest {
  return {
    ...request,
    headers: normalizeHeaders(request.headers),
    cookies: normalizeCookies(request.cookies),
  };
}

export function isSensitiveName(name: string): boolean {
  return /(authorization|proxy-authorization|cookie|set-cookie|api[-_]?key|access[-_]?token|refresh[-_]?token|token|secret|password|passwd|private[-_]?key)/i.test(
    name,
  );
}

export function containsReference(value: string): boolean {
  return /\{\{\s*[a-zA-Z0-9_.-]+\s*\}\}|\$\{\s*[a-zA-Z0-9_.-]+\s*\}/.test(value);
}

function sanitizePair(
  pair: KeyValue,
  mark: (original: string, sanitized: string) => string,
): KeyValue {
  const sensitive = isSensitiveName(pair.key);
  const value = sensitive && pair.value && !containsReference(pair.value)
    ? REDACTED
    : redactKnownTokenPatterns(pair.value);
  return { key: pair.key, value: mark(pair.value, value) };
}

function sanitizeAuth(
  auth: AuthConfig,
  mark: (original: string, sanitized: string) => string,
): AuthConfig {
  const secretField = (value: string) =>
    mark(value, value && !containsReference(value) ? REDACTED : redactKnownTokenPatterns(value));
  return {
    kind: auth.kind,
    username: secretField(auth.username),
    password: secretField(auth.password),
    token: secretField(auth.token),
    api_key: mark(auth.api_key, redactKnownTokenPatterns(auth.api_key)),
    api_value: secretField(auth.api_value),
  };
}

function sanitizeUrl(value: string): string {
  try {
    const url = new URL(value);
    for (const key of [...url.searchParams.keys()]) {
      const current = url.searchParams.get(key) ?? "";
      if (isSensitiveName(key) && !containsReference(current)) url.searchParams.set(key, REDACTED);
    }
    if (url.username && !containsReference(url.username)) url.username = "REDACTED";
    if (url.password && !containsReference(url.password)) url.password = "REDACTED";
    return redactKnownTokenPatterns(url.toString());
  } catch {
    const withoutCredentials = value.replace(
      /^([a-z][a-z0-9+.-]*:\/\/)([^/@]+)@/i,
      (_match, scheme: string, credentials: string) =>
        containsReference(credentials) ? `${scheme}${credentials}@` : `${scheme}REDACTED:REDACTED@`,
    );
    const safeQuery = withoutCredentials.replace(
      /([?&])([^=&#]+)=([^&#]*)/g,
      (_match, separator: string, rawKey: string, rawValue: string) => {
        const key = decodeURIComponentSafely(rawKey);
        const value = isSensitiveName(key) && !containsReference(rawValue) ? REDACTED : rawValue;
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
        if (parts.length < 2 || !isSensitiveName(parts[0].trim())) return redactKnownTokenPatterns(line);
        const raw = parts.slice(1).join("=");
        return `${parts[0]}=${containsReference(raw) ? raw : REDACTED}`;
      })
      .join("\n");
  }
  return redactKnownTokenPatterns(body);
}

function sanitizeJsonValue(value: unknown, key = ""): unknown {
  if (isSensitiveName(key)) {
    return typeof value === "string" && containsReference(value) ? value : REDACTED;
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
    if (looksLikeSecret(candidate)) output = output.split(candidate).join(REDACTED);
  }
  return output;
}

function redactMalformedJsonFields(value: string): string {
  return value.replace(
    /"([^"]+)"\s*:\s*"([^"]*)"/g,
    (match, key: string, raw: string) =>
      isSensitiveName(key) && !containsReference(raw) ? match.replace(raw, REDACTED) : match,
  );
}

function decodeURIComponentSafely(value: string): string {
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
}

function looksLikeSecret(value: string): boolean {
  if (/^(?:sk-|ghp_|github_pat_|glpat-|xox[baprs]-)[A-Za-z0-9_.-]{12,}$/.test(value)) return true;
  if (/^AKIA[A-Z0-9]{16}$/.test(value)) return true;
  const jwt = value.split(".");
  return jwt.length === 3 && jwt.every((part) => part.length >= 10 && /^[A-Za-z0-9_-]+$/.test(part));
}

function isHistoryItem(value: unknown): value is HistoryItem {
  if (!value || typeof value !== "object") return false;
  const item = value as Partial<HistoryItem>;
  return (
    typeof item.id === "string" &&
    (item.name === undefined || typeof item.name === "string") &&
    typeof item.saved_at === "number" &&
    isPersistedRequest(item.request)
  );
}

function isPersistedRequest(value: unknown): value is PersistedHistoryRequest {
  if (!value || typeof value !== "object") return false;
  const request = value as Partial<PersistedHistoryRequest>;
  return (
    typeof request.method === "string" &&
    typeof request.url === "string" &&
    Array.isArray(request.headers) &&
    request.headers.every(isRequestHeader) &&
    (request.cookies === undefined ||
      (Array.isArray(request.cookies) && request.cookies.every(isRequestCookie))) &&
    Array.isArray(request.params) &&
    request.params.every(isKeyValue) &&
    typeof request.body_kind === "string" &&
    typeof request.body === "string" &&
    typeof request.timeout_ms === "number" &&
    typeof request.requiresSecretReview === "boolean"
  );
}

function isKeyValue(value: unknown): value is KeyValue {
  if (!value || typeof value !== "object") return false;
  const pair = value as Partial<KeyValue>;
  return typeof pair.key === "string" && typeof pair.value === "string";
}

function countLegacyHistoryEntries(raw: string | null): number {
  try {
    const parsed = JSON.parse(raw ?? "null") as unknown;
    return Array.isArray(parsed) ? parsed.length : raw === null ? 0 : 1;
  } catch {
    return raw === null ? 0 : 1;
  }
}
