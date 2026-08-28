import type {
  AuthConfig,
  GraphqlRequest,
  KeyValue,
  MultipartPart,
  PersistedHistoryRequest,
  RequestCookie,
  RequestHeader,
} from "../types";
import {
  type CollectionEntry,
  type CollectionStore,
  COLLECTION_VERSION,
} from "./collections";
import type { Environment, EnvironmentStore, EnvVariable } from "./environments";
import { safeMultipartFileName } from "./multipart";
import {
  isSensitiveName,
  REDACTED,
  sanitizeRequestForPersistence,
  toRequestTemplate,
} from "./persistence";
import { isExactVariableReference } from "./references";

export const TRANSFER_VERSION = 1;
export const COLLECTION_EXPORT_SCHEMA = "devbox.api-playground.collection-export";
export const ENVIRONMENT_EXPORT_SCHEMA = "devbox.api-playground.environment-export";
export const MAX_TRANSFER_BYTES = 1 * 1024 * 1024;
export const MAX_EXPORTED_COLLECTIONS = 256;
export const MAX_EXPORTED_ENVIRONMENTS = 64;
export const MAX_EXPORTED_VARIABLES = 256;
export const MAX_TRANSFER_NAME_CHARS = 120;
export const MAX_TRANSFER_ID_CHARS = 256;
export const MAX_TRANSFER_FIELD_BYTES = 64 * 1024;

export interface CollectionExportDocument {
  schema: typeof COLLECTION_EXPORT_SCHEMA;
  schema_version: 1;
  collections: CollectionEntry[];
}

export interface ExportEnvironmentVariable {
  key: string;
  reference: string;
  secret: boolean;
  value?: string;
}

export interface ExportEnvironment {
  id: string;
  name: string;
  variables: ExportEnvironmentVariable[];
}

export interface EnvironmentExportDocument {
  schema: typeof ENVIRONMENT_EXPORT_SCHEMA;
  schema_version: 1;
  environments: ExportEnvironment[];
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

function onlyKeys(value: Record<string, unknown>, required: readonly string[], optional: readonly string[] = []): boolean {
  const allowed = new Set([...required, ...optional]);
  return required.every((key) => Object.prototype.hasOwnProperty.call(value, key))
    && Object.keys(value).every((key) => allowed.has(key));
}

function boundedString(value: unknown, maxBytes = MAX_TRANSFER_FIELD_BYTES): value is string {
  return typeof value === "string" && new TextEncoder().encode(value).byteLength <= maxBytes;
}

function safeMetadata(value: unknown, maxBytes = MAX_TRANSFER_FIELD_BYTES): value is string {
  return boundedString(value, maxBytes)
    && !hasUnsafeMetadataChars(value)
    && !looksLikeSecret(value);
}

function protectedValue(value: unknown): value is string {
  return typeof value === "string"
    && !hasKnownSecret(value)
    && (value === "" || value === REDACTED || value === "REDACTED" || isExactVariableReference(value));
}

function hasKnownSecret(value: string): boolean {
  return value.includes("-----BEGIN") && value.includes("PRIVATE KEY-----") || looksLikeSecret(value);
}

function boundedName(value: unknown): value is string {
  return typeof value === "string" && value.length <= MAX_TRANSFER_NAME_CHARS
    && !hasUnsafeMetadataChars(value)
    && new TextEncoder().encode(value).byteLength <= MAX_TRANSFER_FIELD_BYTES;
}

function hasUnsafeMetadataChars(value: string): boolean {
  // Keep metadata single-line and visible. This includes C0/C1 controls and
  // Unicode line separators which can otherwise split a displayed/exported
  // record without being represented as a normal newline.
  return /[\u0000-\u001f\u007f-\u009f\u2028\u2029]/u.test(value);
}

function redactKnownMetadataSecrets(value: string): string {
  let redacted = value.replace(
    /(?:sk[_-]|ghp_|github_pat_|glpat-|xox[bprsa]-)[A-Za-z0-9_.-]{12,}|AKIA[A-Z0-9]{16}|eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}/gu,
    "[REDACTED]",
  );
  try {
    const url = new URL(redacted);
    let changed = redacted !== value;
    if (url.username) {
      url.username = "[REDACTED]";
      changed = true;
    }
    if (url.password) {
      url.password = "[REDACTED]";
      changed = true;
    }
    for (const key of [...url.searchParams.keys()]) {
      if (isSensitiveName(key) && url.searchParams.get(key) !== "[REDACTED]") {
        url.searchParams.set(key, "[REDACTED]");
        changed = true;
      }
    }
    if (changed) redacted = url.toString();
  } catch {
    // Arbitrary user labels are not URLs; token-shaped literals above are
    // still redacted without attempting URL normalization.
  }
  return redacted;
}

function safeExportMetadata(
  value: unknown,
  maxChars: number,
  fallback: string,
  maxBytes = MAX_TRANSFER_FIELD_BYTES,
): string {
  const source = typeof value === "string" ? value : fallback;
  const redacted = redactKnownMetadataSecrets(source);
  const normalized = redacted.replace(/[\u0000-\u001f\u007f-\u009f\u2028\u2029]+/gu, " ").trim();
  const characters = Array.from(normalized).slice(0, maxChars);
  while (characters.length > 0 && new TextEncoder().encode(characters.join("")).byteLength > maxBytes) {
    characters.pop();
  }
  const bounded = characters.join("");
  return bounded || fallback;
}

function hasKnownMetadataSecret(value: string): boolean {
  return redactKnownMetadataSecrets(value) !== value;
}

function isKeyValue(value: unknown): value is KeyValue {
  return isRecord(value) && onlyKeys(value, ["key", "value"])
    && safeMetadata(value.key) && boundedString(value.value)
    && (!isSensitiveName(value.key) || protectedValue(value.value))
    && !hasKnownSecret(value.value);
}

function isHeader(value: unknown): value is RequestHeader {
  return isRecord(value) && onlyKeys(value, ["key", "value"], ["enabled"])
    && safeMetadata(value.key, 256) && boundedString(value.value)
    && (!isSensitiveName(value.key) || protectedValue(value.value))
    && !hasKnownSecret(value.value)
    && (value.enabled === undefined || typeof value.enabled === "boolean");
}

function isCookie(value: unknown): value is RequestCookie {
  return isRecord(value) && onlyKeys(value, ["name", "value"], ["enabled"])
    && safeMetadata(value.name, 256) && protectedValue(value.value)
    && (value.enabled === undefined || typeof value.enabled === "boolean");
}

function isMultipart(value: unknown): value is MultipartPart {
  if (!isRecord(value) || !onlyKeys(value, ["kind", "name", "value", "file_path", "file_name", "content_type"], ["enabled"])) return false;
  if (!safeMetadata(value.kind, 16) || !safeMetadata(value.name, 256)
    || !boundedString(value.value) || !boundedString(value.file_path)
    || !safeMetadata(value.file_name) || !safeMetadata(value.content_type, 256)
    || value.enabled !== undefined && typeof value.enabled !== "boolean") return false;
  if (value.kind === "file") {
    return value.value === "" && value.file_path === ""
      && !/[\\/]/u.test(value.file_name) && !hasKnownSecret(value.file_name);
  }
  return value.file_path === "" && value.file_name === ""
    && (!isSensitiveName(value.name) || protectedValue(value.value))
    && !hasKnownSecret(value.value);
}

function isAuth(value: unknown): value is AuthConfig | null {
  if (value === null) return true;
  return isRecord(value) && onlyKeys(value, ["kind", "username", "password", "token", "api_key", "api_value"])
    && safeMetadata(value.kind, 64) && protectedValue(value.username)
    && protectedValue(value.password) && protectedValue(value.token)
    && safeMetadata(value.api_key, 256) && protectedValue(value.api_value);
}

function isGraphql(value: unknown): value is GraphqlRequest | null | undefined {
  if (value === undefined || value === null) return true;
  if (!isRecord(value) || !onlyKeys(value, ["query", "variables", "operation_name"])
    || boundedString(value.query) === false || boundedString(value.variables) === false
    || hasUnsafeMetadataChars(value.query) || hasUnsafeMetadataChars(value.variables)
    || !safeMetadata(value.operation_name, 256) || hasKnownSecret(value.query)) return false;
  if (!value.variables.trim()) return true;
  if (protectedValue(value.variables)) return true;
  try {
    return isSafeJsonValue(JSON.parse(value.variables), "");
  } catch {
    return false;
  }
}

function isSafeJsonValue(value: unknown, key: string): boolean {
  if (isSensitiveName(key)) return typeof value === "string" && protectedValue(value);
  if (typeof value === "string") return boundedString(value) && !hasUnsafeMetadataChars(value) && !hasKnownSecret(value);
  if (Array.isArray(value)) return value.every((item) => isSafeJsonValue(item, ""));
  if (value && typeof value === "object") {
    return Object.entries(value as Record<string, unknown>).every(([childKey, child]) =>
      isSafeJsonValue(child, childKey));
  }
  return true;
}

function isPersistedRequest(value: unknown): value is PersistedHistoryRequest {
  if (!isRecord(value) || !onlyKeys(value,
    ["method", "url", "headers", "cookies", "multipart", "params", "body_kind", "body", "auth", "timeout_ms", "requiresSecretReview"],
    ["graphql"],
  )) return false;
  return safeMetadata(value.method, 32) && value.method.length > 0
    && boundedString(value.url)
    && !hasUnsafeMetadataChars(value.url) && !hasKnownSecret(value.url)
    && Array.isArray(value.headers) && value.headers.length <= 100 && value.headers.every(isHeader)
    && Array.isArray(value.cookies) && value.cookies.length <= 100 && value.cookies.every(isCookie)
    && Array.isArray(value.multipart) && value.multipart.length <= 50 && value.multipart.every(isMultipart)
    && Array.isArray(value.params) && value.params.length <= 100 && value.params.every(isKeyValue)
    && safeMetadata(value.body_kind, 32) && boundedString(value.body)
    && !hasUnsafeMetadataChars(value.body) && !hasKnownSecret(value.body)
    && isAuth(value.auth) && Number.isSafeInteger(value.timeout_ms) && Number(value.timeout_ms) >= 0
    && typeof value.requiresSecretReview === "boolean" && isGraphql(value.graphql);
}

function safeTransferValue(value: unknown, maxBytes = MAX_TRANSFER_FIELD_BYTES, fallback = ""): string {
  const source = typeof value === "string" ? value : fallback;
  const normalized = redactKnownMetadataSecrets(source)
    .replace(/[\u0000-\u001f\u007f-\u009f\u2028\u2029]+/gu, " ");
  const characters = Array.from(normalized);
  while (characters.length > 0 && new TextEncoder().encode(characters.join("")).byteLength > maxBytes) {
    characters.pop();
  }
  return characters.join("") || fallback;
}

function safeTransferMetadata(
  value: unknown,
  maxChars: number,
  maxBytes: number,
  fallback: string,
): string {
  const visible = safeExportMetadata(value, maxChars, fallback);
  return safeTransferValue(visible, maxBytes, fallback);
}

function exportMethod(value: unknown): string {
  const method = safeTransferMetadata(value, 32, 32, "GET").toUpperCase();
  return /^[A-Z][A-Z0-9!#$%&'*+.^_`|~-]{0,31}$/u.test(method) ? method : "GET";
}

function exportBodyKind(value: unknown): string {
  const kind = safeTransferMetadata(value, 32, 32, "none");
  return ["none", "json", "form", "multipart", "raw", "graphql"].includes(kind) ? kind : "none";
}

function exportAuthKind(value: unknown): string {
  const kind = safeTransferMetadata(value, 64, 64, "none");
  return ["none", "basic", "bearer", "apikey"].includes(kind) ? kind : "none";
}

function cleanPersistedRequest(request: PersistedHistoryRequest): PersistedHistoryRequest {
  let safe: PersistedHistoryRequest;
  try {
    safe = sanitizeRequestForPersistence(toRequestTemplate(request));
  } catch {
    throw new Error("Collection request를 안전하게 내보낼 수 없습니다");
  }
  const bodyKind = exportBodyKind(safe.body_kind);
  const headers = safe.headers.map((header) => ({
    key: safeTransferMetadata(header.key, 256, 256, "X-Header"),
    value: safeTransferValue(header.value),
    enabled: header.enabled !== false,
  }));
  const cookies = safe.cookies.map((cookie) => ({
    name: safeTransferMetadata(cookie.name, 256, 256, "cookie"),
    value: safeTransferValue(cookie.value),
    enabled: cookie.enabled !== false,
  }));
  const multipart = safe.multipart.map((part) => {
    const kind = part.kind === "file" ? "file" : "text";
    const fileName = kind === "file"
      ? safeTransferMetadata(safeMultipartFileName(part.file_name), 255, MAX_TRANSFER_FIELD_BYTES, "file")
      : "";
    return {
      kind,
      name: safeTransferMetadata(part.name, 256, 256, ""),
      value: kind === "text" ? safeTransferValue(part.value) : "",
      file_path: "",
      file_name: fileName,
      content_type: safeTransferMetadata(part.content_type, 256, 256, ""),
      enabled: part.enabled !== false,
    } satisfies MultipartPart;
  });
  const params = safe.params.map((param) => ({
    key: safeTransferMetadata(param.key, MAX_TRANSFER_FIELD_BYTES, MAX_TRANSFER_FIELD_BYTES, ""),
    value: safeTransferValue(param.value),
  }));
  const auth = safe.auth ? {
    kind: exportAuthKind(safe.auth.kind),
    username: safeTransferValue(safe.auth.username, MAX_TRANSFER_FIELD_BYTES, ""),
    password: safeTransferValue(safe.auth.password, MAX_TRANSFER_FIELD_BYTES, ""),
    token: safeTransferValue(safe.auth.token, MAX_TRANSFER_FIELD_BYTES, ""),
    api_key: safeTransferMetadata(safe.auth.api_key, 256, 256, ""),
    api_value: safeTransferValue(safe.auth.api_value, MAX_TRANSFER_FIELD_BYTES, ""),
  } : null;
  const graphql = bodyKind === "graphql" && safe.graphql ? {
    query: safeTransferValue(safe.graphql.query),
    variables: safeTransferValue(safe.graphql.variables),
    operation_name: safeTransferMetadata(safe.graphql.operation_name, 256, 256, ""),
  } : undefined;
  return {
    method: exportMethod(safe.method),
    url: safeTransferValue(safe.url),
    headers,
    cookies,
    multipart,
    params,
    body_kind: bodyKind,
    body: bodyKind === "graphql" || bodyKind === "multipart" ? "" : safeTransferValue(safe.body),
    auth,
    timeout_ms: Number.isSafeInteger(safe.timeout_ms) && safe.timeout_ms >= 0 ? safe.timeout_ms : 0,
    ...(graphql ? { graphql } : {}),
    requiresSecretReview: Boolean(request.requiresSecretReview || safe.requiresSecretReview),
  };
}

function cleanCollectionEntry(value: unknown): CollectionEntry | null {
  if (!isRecord(value) || !onlyKeys(value, ["id", "name", "folder", "saved_at", "request", "requiresSecretReview"])) return null;
  const request = value.request;
  if (!boundedString(value.id, MAX_TRANSFER_ID_CHARS) || !boundedName(value.name) || !boundedName(value.folder)
    || hasUnsafeMetadataChars(value.id) || hasKnownMetadataSecret(value.id)
    || hasKnownMetadataSecret(value.name) || hasKnownMetadataSecret(value.folder)
    || !Number.isSafeInteger(value.saved_at) || Number(value.saved_at) < 0
    || typeof value.requiresSecretReview !== "boolean" || !isPersistedRequest(request)) return null;
  let safeRequest: PersistedHistoryRequest;
  try {
    safeRequest = cleanPersistedRequest(request);
  } catch {
    return null;
  }
  return {
    id: value.id,
    name: value.name.trim() || "untitled",
    folder: value.folder.trim(),
    saved_at: Number(value.saved_at),
    request: safeRequest,
    requiresSecretReview: Boolean(value.requiresSecretReview) || request.requiresSecretReview,
  };
}

function parseDocument(raw: string): unknown | null {
  if (new TextEncoder().encode(raw).byteLength > MAX_TRANSFER_BYTES) return null;
  try {
    return JSON.parse(raw) as unknown;
  } catch {
    return null;
  }
}

export function serializeCollectionExport(store: CollectionStore): string {
  if (store.collections.length > MAX_EXPORTED_COLLECTIONS) {
    throw new Error("Collection export 항목 수가 허용된 크기를 초과했습니다");
  }
  const collections = store.collections.map((entry, index) => ({
    id: safeExportMetadata(entry.id, MAX_TRANSFER_ID_CHARS, `collection-${index + 1}`, MAX_TRANSFER_ID_CHARS),
    name: safeExportMetadata(entry.name, MAX_TRANSFER_NAME_CHARS, "untitled"),
    folder: safeExportMetadata(entry.folder, MAX_TRANSFER_NAME_CHARS, ""),
    saved_at: Number.isSafeInteger(entry.saved_at) && entry.saved_at >= 0 ? entry.saved_at : 0,
    request: cleanPersistedRequest(entry.request),
    requiresSecretReview: Boolean(entry.requiresSecretReview || entry.request.requiresSecretReview),
  } satisfies CollectionEntry));
  const raw = JSON.stringify({ schema: COLLECTION_EXPORT_SCHEMA, schema_version: TRANSFER_VERSION, collections });
  if (!parseCollectionExport(raw)) throw new Error("Collection export metadata가 안전하지 않습니다");
  return raw;
}

export function parseCollectionExport(raw: string): CollectionStore | null {
  const value = parseDocument(raw);
  if (!isRecord(value) || !onlyKeys(value, ["schema", "schema_version", "collections"])
    || value.schema !== COLLECTION_EXPORT_SCHEMA || value.schema_version !== TRANSFER_VERSION
    || !Array.isArray(value.collections) || value.collections.length > MAX_EXPORTED_COLLECTIONS) return null;
  const collections = value.collections.map(cleanCollectionEntry);
  if (collections.some((entry) => entry === null)) return null;
  return { version: COLLECTION_VERSION, collections: collections as CollectionEntry[] };
}

function looksLikeSecret(value: string): boolean {
  return /(?:sk[_-]|ghp_|github_pat_|glpat-|xox[bprsa]-)[A-Za-z0-9_.-]{12,}/u.test(value)
    || /^AKIA[A-Z0-9]{16}$/u.test(value);
}

function isEnvironmentKey(value: string): boolean {
  return value.length > 0 && value.length <= 128 && /^[A-Za-z0-9_.-]+$/u.test(value)
    && !hasKnownSecret(value);
}

function exportEnvironmentVariable(variable: EnvVariable): ExportEnvironmentVariable | null {
  if (!isEnvironmentKey(variable.key) || !boundedString(variable.value)
    || hasUnsafeMetadataChars(variable.value)) return null;
  const secret = variable.secret || isSensitiveName(variable.key) || looksLikeSecret(variable.value);
  return secret
    ? { key: variable.key, reference: `\${${variable.key}}`, secret: true }
    : { key: variable.key, reference: `\${${variable.key}}`, secret: false, value: variable.value };
}

export function serializeEnvironmentExport(store: EnvironmentStore): string {
  if (store.environments.length > MAX_EXPORTED_ENVIRONMENTS) {
    throw new Error("Environment export 항목 수가 허용된 크기를 초과했습니다");
  }
  const environments = store.environments.map((environment, index) => {
    if (environment.variables.length > MAX_EXPORTED_VARIABLES) {
      throw new Error("Environment export 변수 수가 허용된 크기를 초과했습니다");
    }
    const variables = environment.variables.map(exportEnvironmentVariable);
    if (variables.some((variable) => variable === null)) {
      throw new Error("Environment export metadata가 안전하지 않습니다");
    }
    return {
    id: safeExportMetadata(environment.id, MAX_TRANSFER_ID_CHARS, `environment-${index + 1}`, MAX_TRANSFER_ID_CHARS),
    name: safeExportMetadata(environment.name, MAX_TRANSFER_NAME_CHARS, "새 환경"),
    variables: variables as ExportEnvironmentVariable[],
    };
  });
  const raw = JSON.stringify({ schema: ENVIRONMENT_EXPORT_SCHEMA, schema_version: TRANSFER_VERSION, environments });
  if (!parseEnvironmentExport(raw)) throw new Error("Environment export metadata가 안전하지 않습니다");
  return raw;
}

function cleanEnvironment(value: unknown): ExportEnvironment | null {
  if (!isRecord(value) || !onlyKeys(value, ["id", "name", "variables"]) || !boundedString(value.id, MAX_TRANSFER_ID_CHARS)
    || !boundedName(value.name) || hasUnsafeMetadataChars(value.id)
    || hasKnownMetadataSecret(value.id) || hasKnownMetadataSecret(value.name)
    || !Array.isArray(value.variables) || value.variables.length > MAX_EXPORTED_VARIABLES) return null;
  const variables = value.variables.map((candidate): ExportEnvironmentVariable | null => {
    if (!isRecord(candidate) || !onlyKeys(candidate, ["key", "reference", "secret"], ["value"])
      || !boundedString(candidate.key, 128) || !/^[A-Za-z0-9_.-]+$/u.test(candidate.key)
      || candidate.reference !== `\${${candidate.key}}` || typeof candidate.secret !== "boolean") return null;
    if (candidate.secret) {
      if (Object.prototype.hasOwnProperty.call(candidate, "value")) return null;
      return { key: candidate.key, reference: candidate.reference, secret: true };
    }
    if (!boundedString(candidate.value) || hasUnsafeMetadataChars(candidate.value)) return null;
    // A hand-written export must not bypass the same policy used by the
    // exporter. Sensitive names and recognizable token-shaped values are
    // reference-only even when the incoming document lies about `secret`.
    if (isSensitiveName(candidate.key) || looksLikeSecret(candidate.value)) return null;
    return { key: candidate.key, reference: candidate.reference, secret: false, value: candidate.value };
  });
  if (variables.some((variable) => variable === null)) return null;
  const keys = variables.map((variable) => variable?.key);
  if (new Set(keys).size !== keys.length) return null;
  return { id: value.id, name: value.name.trim() || "새 환경", variables: variables as ExportEnvironmentVariable[] };
}

export function parseEnvironmentExport(raw: string): EnvironmentExportDocument | null {
  const value = parseDocument(raw);
  if (!isRecord(value) || !onlyKeys(value, ["schema", "schema_version", "environments"])
    || value.schema !== ENVIRONMENT_EXPORT_SCHEMA || value.schema_version !== TRANSFER_VERSION
    || !Array.isArray(value.environments) || value.environments.length > MAX_EXPORTED_ENVIRONMENTS) return null;
  const environments = value.environments.map(cleanEnvironment);
  if (environments.some((environment) => environment === null)) return null;
  return { schema: ENVIRONMENT_EXPORT_SCHEMA, schema_version: TRANSFER_VERSION, environments: environments as ExportEnvironment[] };
}

export function mergeImportedCollections(
  current: CollectionStore,
  imported: CollectionStore,
  makeId: () => string,
): CollectionStore | null {
  if (current.collections.length > MAX_EXPORTED_COLLECTIONS
    || imported.collections.length > MAX_EXPORTED_COLLECTIONS
    || current.collections.length + imported.collections.length > MAX_EXPORTED_COLLECTIONS) return null;
  const used = new Set(current.collections.map((entry) => entry.id));
  const additions: CollectionEntry[] = [];
  for (const entry of imported.collections) {
    const id = nextUniqueId(makeId, used);
    if (!id) return null;
    used.add(id);
    additions.push({ ...entry, id, request: cleanPersistedRequest(entry.request) });
  }
  return { ...current, collections: [...additions, ...current.collections] };
}

export function mergeImportedEnvironments(
  current: EnvironmentStore,
  imported: EnvironmentExportDocument,
  makeId: () => string,
): EnvironmentStore | null {
  if (current.environments.length > MAX_EXPORTED_ENVIRONMENTS
    || imported.environments.length > MAX_EXPORTED_ENVIRONMENTS
    || current.environments.length + imported.environments.length > MAX_EXPORTED_ENVIRONMENTS) return null;
  const used = new Set(current.environments.map((environment) => environment.id));
  const additions: Environment[] = [];
  for (const incoming of imported.environments) {
    const id = nextUniqueId(makeId, used);
    if (!id) return null;
    used.add(id);
    additions.push({
      id,
      name: incoming.name,
      variables: incoming.variables.slice(0, MAX_EXPORTED_VARIABLES).map((variable) => ({
        key: variable.key,
        value: variable.secret ? "" : variable.value ?? "",
        secret: variable.secret,
      })),
    });
  }
  return { ...current, environments: [...additions, ...current.environments] };
}

function nextUniqueId(makeId: () => string, used: Set<string>): string | null {
  for (let attempt = 0; attempt < 1024; attempt += 1) {
    let candidate: string;
    try {
      candidate = makeId();
    } catch {
      return null;
    }
    if (boundedString(candidate, MAX_TRANSFER_ID_CHARS)
      && candidate.length > 0
      && !hasUnsafeMetadataChars(candidate)
      && !used.has(candidate)) return candidate;
  }
  return null;
}

/** Decode a browser-selected transfer file without silently replacing invalid UTF-8. */
export function decodeTransferBytes(bytes: Uint8Array): string {
  if (bytes.byteLength > MAX_TRANSFER_BYTES) throw new Error("transfer too large");
  try {
    return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  } catch {
    throw new Error("transfer is not valid UTF-8");
  }
}

/** Read only a browser File whose declared and actual sizes stay within the transfer bound. */
export async function readTransferFile(file: Pick<File, "size" | "arrayBuffer">): Promise<string> {
  if (!Number.isSafeInteger(file.size) || file.size < 0 || file.size > MAX_TRANSFER_BYTES) {
    throw new Error("transfer too large");
  }
  const bytes = new Uint8Array(await file.arrayBuffer());
  return decodeTransferBytes(bytes);
}
