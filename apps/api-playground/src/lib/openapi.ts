import {
  OPENAPI_DOCUMENT_LIMITS,
  parseBoundedOpenApiDocument,
} from "@devbox/openapi";
import type {
  AuthConfig,
  KeyValue,
  MultipartPart,
  RequestHeader,
  RequestTemplate,
} from "../types";
import { MAX_REQUEST_COOKIE_ROWS } from "./cookies";
import { MAX_REQUEST_HEADER_ROWS } from "./headers";
import { MAX_MULTIPART_PARTS } from "./multipart";

/**
 * OpenAPI parsing stays a local, bounded transform. Native URL retrieval is a
 * separate command; neither source path resolves a $ref or turns an example
 * into a secret-bearing request. Keep these limits in one place so every
 * source enforces the same parser contract.
 */
export const OPENAPI_LIMITS = Object.freeze({
  ...OPENAPI_DOCUMENT_LIMITS,
  maxPaths: 250,
  maxOperations: 1_000,
  maxServers: 20,
  maxParameters: 2_000,
  maxSecuritySchemes: 100,
  maxMediaTypes: 50,
  maxBodyBytes: 512 * 1024,
  maxRequestRows: 100,
  maxParameterNameLength: 256,
  maxFileNameLength: 120,
  maxCollectionNameLength: 120,
});

export type OpenApiFormat = "json" | "yaml";
export type OpenApiIssueScope = "document" | "operation";

export type OpenApiIssueCode =
  | "EMPTY_SOURCE"
  | "SOURCE_TOO_LARGE"
  | "PARSER_ERROR"
  | "UNSUPPORTED_GRAPH"
  | "NODE_LIMIT"
  | "DEPTH_LIMIT"
  | "STRING_LIMIT"
  | "DANGEROUS_KEY"
  | "ROOT_INVALID"
  | "VERSION_UNSUPPORTED"
  | "PATH_LIMIT"
  | "OPERATION_LIMIT"
  | "SERVER_LIMIT"
  | "PARAMETER_LIMIT"
  | "SECURITY_SCHEME_LIMIT"
  | "MEDIA_TYPE_LIMIT"
  | "REQUEST_ROW_LIMIT"
  | "UNSUPPORTED_REF"
  | "PATH_INVALID"
  | "METHOD_UNSUPPORTED"
  | "SERVER_INVALID"
  | "SERVER_VARIABLE_INVALID"
  | "SERVER_OVERRIDE_UNSUPPORTED"
  | "NO_SERVER"
  | "OPERATION_INVALID"
  | "DUPLICATE_PARAMETER"
  | "PARAMETER_INVALID"
  | "PARAMETER_EXAMPLE_OMITTED"
  | "REQUEST_BODY_INVALID"
  | "BODY_EXAMPLE_OMITTED"
  | "BODY_TOO_LARGE"
  | "SECURITY_UNSUPPORTED"
  | "SECURITY_INVALID";

export interface OpenApiIssue {
  code: OpenApiIssueCode;
  message: string;
  scope: OpenApiIssueScope;
  /** Validated OpenAPI route only; never a local filesystem path or parser text. */
  path?: string;
  method?: string;
}

export type OpenApiParameterLocation = "path" | "query" | "header" | "cookie";

export interface OpenApiParameterPreview {
  name: string;
  location: OpenApiParameterLocation;
  value: string;
  redacted: boolean;
  source: "example" | "default" | "enum" | "empty";
}

export type OpenApiSecurityKind = "none" | "basic" | "bearer" | "apikey";

export interface OpenApiSecurityPreview {
  kind: OpenApiSecurityKind;
  location: "header" | "query" | "cookie" | null;
  name: string;
  valuesInjected: false;
}

export interface OpenApiRequestBodyPreview {
  mediaType: string;
  exampleIncluded: boolean;
  redacted: boolean;
}

export interface OpenApiServerPreview {
  index: number;
  url: string;
}

export interface OpenApiOperationPreview {
  id: string;
  path: string;
  method: string;
  label: string;
  serverIndex: number | null;
  request: RequestTemplate;
  parameters: OpenApiParameterPreview[];
  requestBody: OpenApiRequestBodyPreview | null;
  security: OpenApiSecurityPreview | null;
  warnings: OpenApiIssue[];
  errors: OpenApiIssue[];
  applyable: boolean;
}

export interface OpenApiImportPreview {
  format: OpenApiFormat;
  version: "3.0" | "3.1";
  servers: OpenApiServerPreview[];
  operations: OpenApiOperationPreview[];
  errors: OpenApiIssue[];
  sourceName?: string;
}

export type OpenApiImportResult =
  | { ok: true; preview: OpenApiImportPreview }
  | { ok: false; error: OpenApiIssue };

export type OpenApiSource =
  | { kind: "file"; name: string; format?: OpenApiFormat; text: string }
  | { kind: "url"; format: OpenApiFormat; text: string };

const METHOD_ORDER = ["get", "post", "put", "patch", "delete"] as const;
const METHOD_SET = new Set<string>(METHOD_ORDER);
const PATH_ITEM_METADATA = new Set(["parameters", "$ref", "summary", "description", "servers"]);
const HTTP_TOKEN = /^[!#$%&'*+.^_`|~0-9A-Za-z-]+$/;
const SENSITIVE_NAME = /(authorization|proxy-authorization|cookie|set-cookie|api[-_]?key|access[-_]?(?:key|token)|client[-_]?key|refresh[-_]?token|token|secret|password|passwd|credential|private[-_]?key|user[-_]?name|(?:^|[-_.])key(?:$|[-_.]))/i;
const KNOWN_CREDENTIAL = /(?:sk-|ghp_|github_pat_|glpat-|xox[baprs]-)[A-Za-z0-9_.-]{12,}|AKIA[A-Z0-9]{16}|(?:[A-Za-z0-9_-]{10,}\.){2}[A-Za-z0-9_-]{10,}/;
const SECRET_REFERENCE = /\{\{\s*[A-Za-z0-9_.-]+\s*\}\}|\$\{\s*[A-Za-z0-9_.-]+\s*\}/;
const AUTH_VALUE = /^(?:bearer|basic)\s+\S+$/i;

const ISSUE_MESSAGES: Readonly<Record<OpenApiIssueCode, string>> = {
  EMPTY_SOURCE: "OpenAPI 파일이 비어 있습니다.",
  SOURCE_TOO_LARGE: "OpenAPI 파일은 4 MiB 이하만 가져올 수 있습니다.",
  PARSER_ERROR: "OpenAPI JSON/YAML 구문을 안전하게 해석할 수 없습니다.",
  UNSUPPORTED_GRAPH: "순환하거나 과도하게 확장되는 OpenAPI 구조는 가져올 수 없습니다.",
  NODE_LIMIT: "OpenAPI 구조가 안전한 항목 수 제한을 초과했습니다.",
  DEPTH_LIMIT: "OpenAPI 중첩 깊이가 안전한 제한을 초과했습니다.",
  STRING_LIMIT: "OpenAPI 문자열이 안전한 길이 제한을 초과했습니다.",
  DANGEROUS_KEY: "안전하지 않은 객체 키가 포함되어 OpenAPI 가져오기를 중단했습니다.",
  ROOT_INVALID: "OpenAPI 3 문서의 기본 구조가 올바르지 않습니다.",
  VERSION_UNSUPPORTED: "OpenAPI 3.0 또는 3.1 문서만 가져올 수 있습니다.",
  PATH_LIMIT: "OpenAPI path 수가 250개 제한을 초과했습니다.",
  OPERATION_LIMIT: "OpenAPI operation 수가 1,000개 제한을 초과했습니다.",
  SERVER_LIMIT: "OpenAPI server 수가 20개 제한을 초과했습니다.",
  PARAMETER_LIMIT: "OpenAPI parameter 수가 2,000개 제한을 초과했습니다.",
  SECURITY_SCHEME_LIMIT: "OpenAPI security scheme 수가 100개 제한을 초과했습니다.",
  MEDIA_TYPE_LIMIT: "OpenAPI request body media type 수가 50개 제한을 초과했습니다.",
  REQUEST_ROW_LIMIT: "request draft의 parameter/header/cookie 행이 100개 제한을 초과했습니다.",
  UNSUPPORTED_REF: "$ref는 자동 해석하지 않으며 이 operation을 적용할 수 없습니다.",
  PATH_INVALID: "안전하지 않거나 올바르지 않은 OpenAPI path입니다.",
  METHOD_UNSUPPORTED: "현재 request draft가 지원하지 않는 HTTP method입니다.",
  SERVER_INVALID: "사용할 수 없는 HTTP(S) server를 건너뛰었습니다.",
  SERVER_VARIABLE_INVALID: "server variable을 안전하게 해석할 수 없습니다.",
  SERVER_OVERRIDE_UNSUPPORTED: "path 또는 operation server override는 현재 request draft로 표현할 수 없습니다.",
  NO_SERVER: "적용할 HTTP(S) server가 없어 request draft를 만들 수 없습니다.",
  OPERATION_INVALID: "OpenAPI operation 구조가 올바르지 않습니다.",
  DUPLICATE_PARAMETER: "operation 안에 중복 parameter가 있어 적용할 수 없습니다.",
  PARAMETER_INVALID: "지원하지 않거나 올바르지 않은 parameter입니다.",
  PARAMETER_EXAMPLE_OMITTED: "안전하게 문자열화할 수 없는 parameter example은 비워 두었습니다.",
  REQUEST_BODY_INVALID: "request body 구조가 올바르지 않습니다.",
  BODY_EXAMPLE_OMITTED: "안전하게 확인할 수 없는 request body example은 비워 두었습니다.",
  BODY_TOO_LARGE: "request body example이 512 KiB 제한을 초과해 비워 두었습니다.",
  SECURITY_UNSUPPORTED: "지원하지 않는 security scheme은 operation 단위로 격리했습니다.",
  SECURITY_INVALID: "security 요구사항 구조가 올바르지 않습니다.",
};

function issue(
  code: OpenApiIssueCode,
  scope: OpenApiIssueScope,
  path?: string,
  method?: string,
): OpenApiIssue {
  return { code, message: ISSUE_MESSAGES[code], scope, ...(path ? { path } : {}), ...(method ? { method } : {}) };
}

function byteLength(value: string): number {
  return new TextEncoder().encode(value).byteLength;
}

function compareText(left: string, right: string): number {
  return left < right ? -1 : left > right ? 1 : 0;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  if (value === null || typeof value !== "object" || Array.isArray(value)) return false;
  const prototype = Object.getPrototypeOf(value);
  return prototype === Object.prototype || prototype === null;
}

function own(value: Record<string, unknown>, key: string): unknown {
  return Object.prototype.hasOwnProperty.call(value, key) ? value[key] : undefined;
}

function safeFileName(value: string): string {
  const basename = value.split("\\").join("/").split("/").pop() ?? "openapi.yaml";
  const cleaned = [...basename]
    .filter((character) => {
      const code = character.codePointAt(0) ?? 0;
      return code >= 0x20 && code !== 0x7f;
    })
    .join("")
    .trim();
  const bounded = (cleaned || "openapi.yaml").slice(0, OPENAPI_LIMITS.maxFileNameLength);
  return isSensitiveValue(bounded) ? "openapi.yaml" : bounded;
}

export function displayOpenApiFileName(value: string): string {
  return safeFileName(value);
}

export function detectOpenApiFormat(fileName: string): OpenApiFormat {
  return fileName.trim().toLowerCase().endsWith(".json") ? "json" : "yaml";
}

function parseSource(text: string, format: OpenApiFormat): unknown {
  const parsed = parseBoundedOpenApiDocument(text, format);
  if (parsed.ok) return parsed.value;
  throw issue(parsed.error.code, "document");
}

function isOpenApiIssue(value: unknown): value is OpenApiIssue {
  return isRecord(value)
    && typeof value.code === "string"
    && typeof value.message === "string"
    && (value.scope === "document" || value.scope === "operation");
}

function versionOf(value: unknown): "3.0" | "3.1" | null {
  if (typeof value !== "string") return null;
  if (/^3\.0(?:\.\d+)?$/.test(value)) return "3.0";
  if (/^3\.1(?:\.\d+)?$/.test(value)) return "3.1";
  return null;
}

function isSafePath(value: string): boolean {
  if (!value.startsWith("/") || value.length > OPENAPI_LIMITS.maxStringLength || isSensitiveValue(value)) return false;
  const segments = value.split("/");
  if (segments.some((segment) => segment === "." || segment === "..")) return false;
  try {
    const decoded = decodeURIComponent(value);
    if (isSensitiveValue(decoded) || decoded.includes("\\") || decoded.includes("?") || decoded.includes("#") || /[\u0000-\u001f\u007f]/.test(decoded) || decoded.split("/").some((segment) => segment === "." || segment === "..")) return false;
  } catch {
    return false;
  }
  for (let index = 0; index < value.length; index += 1) {
    const character = value[index];
    if (character === "\\" || character === "?" || character === "#" || /[\u0000-\u001f\u007f]/.test(character)) return false;
    if (character === "}") return false;
    if (character === "{") {
      const end = value.indexOf("}", index + 1);
      if (end < 0 || !/^[A-Za-z0-9_.-]{1,120}$/.test(value.slice(index + 1, end))) return false;
      index = end;
    }
  }
  return true;
}

function isSensitiveName(value: string): boolean {
  return SENSITIVE_NAME.test(value);
}

function containsRawCredential(value: string): boolean {
  return KNOWN_CREDENTIAL.test(value);
}

function isSensitiveValue(value: string): boolean {
  return containsRawCredential(value) || SECRET_REFERENCE.test(value) || AUTH_VALUE.test(value);
}

function validParameterName(value: string, location: OpenApiParameterLocation): boolean {
  if (value.length === 0 || value.length > OPENAPI_LIMITS.maxParameterNameLength || /[\u0000-\u001f\u007f]/.test(value)) return false;
  return location === "query" || HTTP_TOKEN.test(value);
}

function safeScalar(value: unknown): string | null {
  if (typeof value === "string") return value.length <= OPENAPI_LIMITS.maxStringLength && !/[\u0000-\u001f\u007f]/.test(value) && !isSensitiveValue(value) ? value : null;
  if (typeof value === "number" && Number.isFinite(value)) return String(value);
  if (typeof value === "boolean") return String(value);
  if (value === null) return "null";
  return null;
}

interface ExampleValue {
  found: boolean;
  value: unknown;
  source: "example" | "default" | "enum" | "empty";
}

function exampleFrom(value: Record<string, unknown>): ExampleValue {
  if (Object.prototype.hasOwnProperty.call(value, "example")) {
    return { found: true, value: own(value, "example"), source: "example" };
  }
  const examples = own(value, "examples");
  if (isRecord(examples)) {
    for (const key of Object.keys(examples).sort()) {
      const example = examples[key];
      if (isRecord(example) && Object.prototype.hasOwnProperty.call(example, "value")) {
        return { found: true, value: own(example, "value"), source: "example" };
      }
    }
  }
  const schema = own(value, "schema");
  if (isRecord(schema)) {
    if (Object.prototype.hasOwnProperty.call(schema, "example")) {
      return { found: true, value: own(schema, "example"), source: "example" };
    }
    if (Object.prototype.hasOwnProperty.call(schema, "default")) {
      return { found: true, value: own(schema, "default"), source: "default" };
    }
    const enumValues = own(schema, "enum");
    if (Array.isArray(enumValues) && enumValues.length > 0) {
      return { found: true, value: enumValues[0], source: "enum" };
    }
  }
  return { found: false, value: "", source: "empty" };
}

function mediaType(value: string): boolean {
  return value.length <= 256
    && /^[A-Za-z0-9!#$&^_.+-]+\/[A-Za-z0-9!#$&^_.+-]+(?:\s*;\s*[A-Za-z0-9!#$&^_.+-]+=[A-Za-z0-9!#$&^_.+-]+)*$/.test(value);
}

function hasRef(value: unknown): boolean {
  if (Array.isArray(value)) return value.some(hasRef);
  if (!isRecord(value)) return false;
  // A malformed non-string `$ref` is not safe to ignore either. Treat any
  // explicit reference member as unsupported at the owning operation.
  if (Object.prototype.hasOwnProperty.call(value, "$ref")) return true;
  return Object.keys(value).some((key) => hasRef(value[key]));
}

function emptyAuth(): AuthConfig {
  return { kind: "none", username: "", password: "", token: "", api_key: "", api_value: "" };
}

function emptyRequest(method: string, url: string): RequestTemplate {
  return {
    method,
    url,
    headers: [],
    cookies: [],
    multipart: [],
    params: [],
    body_kind: "none",
    body: "",
    auth: emptyAuth(),
    timeout_ms: 10_000,
  };
}

function serverUrl(value: unknown): string | null {
  if (typeof value !== "string" || value.length === 0 || value.length > OPENAPI_LIMITS.maxStringLength) return null;
  if (/[\u0000-\u0020\u007f]/.test(value) || isSensitiveValue(value)) return null;
  if (/%(?:2e|2f|5c|23|3f|00)/i.test(value)) return null;
  if (/(?:^|\/)\.\.?(?:\/|$)/.test(value)) return null;
  const variables = [...value.matchAll(/\{([A-Za-z0-9_.-]{1,120})\}/g)];
  const withoutVariables = value.replace(/\{[A-Za-z0-9_.-]{1,120}\}/g, "example.invalid");
  if (withoutVariables.includes("{")) return null;
  let parsed: URL;
  try {
    parsed = new URL(withoutVariables);
  } catch {
    return null;
  }
  try {
    if (isSensitiveValue(decodeURIComponent(value))) return null;
  } catch {
    return null;
  }
  if (parsed.protocol !== "http:" && parsed.protocol !== "https:") return null;
  if (parsed.username || parsed.password) return null;
  if (parsed.search || parsed.hash) return null;
  if (parsed.pathname.split("/").some((segment) => segment === "." || segment === "..")) return null;
  if (variables.length > 0) return null;
  return value;
}

function serverUrlWithVariables(value: unknown): string | null {
  if (!isRecord(value)) return null;
  const raw = own(value, "url");
  if (typeof raw !== "string" || raw.length === 0 || raw.length > OPENAPI_LIMITS.maxStringLength) return null;
  if (/[\u0000-\u0020\u007f]/.test(raw)) return null;
  const variableRecord = own(value, "variables");
  const variables = isRecord(variableRecord) ? variableRecord : Object.create(null) as Record<string, unknown>;
  const substituted = raw.replace(/\{([A-Za-z0-9_.-]{1,120})\}/g, (_match, name: string) => {
    const variable = variables[name];
    if (!isRecord(variable) || typeof variable.default !== "string" || isSensitiveName(name)) throw issue("SERVER_VARIABLE_INVALID", "document");
    if (variable.default.length > OPENAPI_LIMITS.maxStringLength || /[\u0000-\u0020\u007f]/.test(variable.default) || isSensitiveValue(variable.default)) throw issue("SERVER_VARIABLE_INVALID", "document");
    return variable.default;
  });
  return serverUrl(substituted);
}

function joinServerPath(server: string, path: string): string | null {
  const base = server.endsWith("/") ? server.slice(0, -1) : server;
  const joined = `${base}${path}`;
  try {
    const parsed = new URL(joined);
    if (parsed.protocol !== "http:" && parsed.protocol !== "https:") return null;
    if (parsed.username || parsed.password) return null;
    for (const key of parsed.searchParams.keys()) if (isSensitiveName(key)) return null;
    return joined;
  } catch {
    return null;
  }
}

function sortedMethods(pathItem: Record<string, unknown>): string[] {
  return METHOD_ORDER.filter((method) => Object.prototype.hasOwnProperty.call(pathItem, method));
}

function sanitizeBodyValue(value: unknown, propertyName?: string): { value: unknown; redacted: boolean; safe: boolean } {
  if (propertyName && isSensitiveName(propertyName)) return { value: "", redacted: true, safe: true };
  if (typeof value === "string") {
    if (isSensitiveValue(value)) return { value: "", redacted: true, safe: true };
    if (/[\u0000-\u001f\u007f]/.test(value)) return { value: "", redacted: false, safe: false };
    return value.length <= OPENAPI_LIMITS.maxStringLength
      ? { value, redacted: false, safe: true }
      : { value: "", redacted: false, safe: false };
  }
  if (typeof value === "number" || typeof value === "boolean" || value === null) {
    return typeof value === "number" && !Number.isFinite(value)
      ? { value: "", redacted: false, safe: false }
      : { value, redacted: false, safe: true };
  }
  if (Array.isArray(value)) {
    const result: unknown[] = [];
    let redacted = false;
    for (const entry of value) {
      const safe = sanitizeBodyValue(entry);
      if (!safe.safe) return { value: [], redacted: false, safe: false };
      result.push(safe.value);
      redacted ||= safe.redacted;
    }
    return { value: result, redacted, safe: true };
  }
  if (!isRecord(value)) return { value: "", redacted: false, safe: false };
  const result = Object.create(null) as Record<string, unknown>;
  let redacted = false;
  for (const key of Object.keys(value).sort()) {
    if (key.length > OPENAPI_LIMITS.maxStringLength || /[\u0000-\u001f\u007f]/.test(key)) return { value: Object.create(null), redacted: false, safe: false };
    const safe = sanitizeBodyValue(value[key], key);
    if (!safe.safe) return { value: Object.create(null), redacted: false, safe: false };
    result[key] = safe.value;
    redacted ||= safe.redacted;
  }
  return { value: result, redacted, safe: true };
}

function schemaExample(schema: Record<string, unknown>, depth = 0): unknown {
  if (depth > 12) return undefined;
  if (Object.prototype.hasOwnProperty.call(schema, "example")) return own(schema, "example");
  if (Object.prototype.hasOwnProperty.call(schema, "default")) return own(schema, "default");
  const enumValues = own(schema, "enum");
  if (Array.isArray(enumValues) && enumValues.length > 0) return enumValues[0];
  const type = own(schema, "type");
  if (type === "object" || isRecord(own(schema, "properties"))) {
    const properties = own(schema, "properties");
    if (!isRecord(properties)) return {};
    const result = Object.create(null) as Record<string, unknown>;
    for (const key of Object.keys(properties).sort()) {
      const property = properties[key];
      if (!isRecord(property)) continue;
      const example = schemaExample(property, depth + 1);
      if (example !== undefined) result[key] = example;
    }
    return result;
  }
  if (type === "array") {
    const items = own(schema, "items");
    return isRecord(items) ? [schemaExample(items, depth + 1)].filter((entry) => entry !== undefined) : [];
  }
  if (type === "boolean") return false;
  if (type === "integer" || type === "number") return 0;
  return type === "string" ? "" : undefined;
}

function bodySample(content: Record<string, unknown>): { value: unknown; found: boolean } {
  const direct = exampleFrom(content);
  if (direct.found) return { value: direct.value, found: true };
  const schema = own(content, "schema");
  if (isRecord(schema)) {
    const sample = schemaExample(schema);
    return { value: sample, found: sample !== undefined };
  }
  return { value: "", found: false };
}

function sortedObjectEntries(value: Record<string, unknown>): [string, unknown][] {
  return Object.keys(value).sort().map((key) => [key, value[key]]);
}

function bodyForMedia(
  media: string,
  content: Record<string, unknown>,
): { kind: string; body: string; multipart: MultipartPart[]; exampleIncluded: boolean; redacted: boolean; warning: OpenApiIssueCode | null } {
  const normalized = media.split(";", 1)[0].trim().toLowerCase();
  const sample = bodySample(content);
  if (!sample.found) {
    return { kind: normalized === "multipart/form-data" ? "multipart" : normalized === "application/x-www-form-urlencoded" ? "form" : normalized.includes("json") ? "json" : "raw", body: "", multipart: [], exampleIncluded: false, redacted: false, warning: null };
  }
  // A scalar document-level example has no property name to classify. Treat a
  // non-empty string as opaque so a bearer/password literal cannot be copied
  // into a draft merely because it was documented as an example.
  if ((typeof sample.value === "string" && sample.value.length > 0) || Array.isArray(sample.value)) {
    return { kind: "none", body: "", multipart: [], exampleIncluded: false, redacted: false, warning: "BODY_EXAMPLE_OMITTED" };
  }
  const sanitized = sanitizeBodyValue(sample.value);
  if (!sanitized.safe) {
    return { kind: "none", body: "", multipart: [], exampleIncluded: false, redacted: false, warning: "BODY_EXAMPLE_OMITTED" };
  }
  if (normalized === "multipart/form-data") {
    const multipart: MultipartPart[] = [];
    let multipartBytes = 0;
    if (isRecord(sanitized.value)) {
      for (const [name, value] of sortedObjectEntries(sanitized.value)) {
        const scalar = safeScalar(value);
        if (scalar === null || name.length > 120 || !HTTP_TOKEN.test(name)) {
          return { kind: "multipart", body: "", multipart: [], exampleIncluded: false, redacted: sanitized.redacted, warning: "BODY_EXAMPLE_OMITTED" };
        }
        multipartBytes += byteLength(name) + byteLength(scalar);
        if (multipart.length >= MAX_MULTIPART_PARTS || multipartBytes > OPENAPI_LIMITS.maxBodyBytes) {
          return { kind: "multipart", body: "", multipart: [], exampleIncluded: false, redacted: sanitized.redacted, warning: "BODY_TOO_LARGE" };
        }
        multipart.push({ kind: "text", name, value: scalar, file_path: "", file_name: "", content_type: "", enabled: true });
      }
    }
    return { kind: "multipart", body: "", multipart, exampleIncluded: true, redacted: sanitized.redacted, warning: null };
  }
  if (normalized === "application/x-www-form-urlencoded") {
    if (!isRecord(sanitized.value)) return { kind: "form", body: "", multipart: [], exampleIncluded: false, redacted: sanitized.redacted, warning: "BODY_EXAMPLE_OMITTED" };
    const form = new URLSearchParams();
    for (const [name, value] of sortedObjectEntries(sanitized.value)) {
      const scalar = safeScalar(value);
      if (scalar !== null) form.set(name, scalar);
    }
    const body = form.toString();
    return byteLength(body) > OPENAPI_LIMITS.maxBodyBytes
      ? { kind: "form", body: "", multipart: [], exampleIncluded: false, redacted: sanitized.redacted, warning: "BODY_TOO_LARGE" }
      : { kind: "form", body, multipart: [], exampleIncluded: true, redacted: sanitized.redacted, warning: null };
  }
  if (normalized === "application/json" || normalized.endsWith("+json")) {
    let body: string;
    try {
      body = JSON.stringify(sanitized.value, null, 2);
    } catch {
      return { kind: "json", body: "", multipart: [], exampleIncluded: false, redacted: sanitized.redacted, warning: "BODY_EXAMPLE_OMITTED" };
    }
    if (byteLength(body) > OPENAPI_LIMITS.maxBodyBytes) {
      return { kind: "json", body: "", multipart: [], exampleIncluded: false, redacted: sanitized.redacted, warning: "BODY_TOO_LARGE" };
    }
    return { kind: "json", body, multipart: [], exampleIncluded: true, redacted: sanitized.redacted, warning: null };
  }
  // Non-JSON raw examples can be legitimate documentation, but may also be a
  // bearer/password value. Leave them out unless the source was structured.
  return { kind: "raw", body: "", multipart: [], exampleIncluded: false, redacted: sanitized.redacted, warning: "BODY_EXAMPLE_OMITTED" };
}

function chooseMedia(content: Record<string, unknown>): string | null {
  const keys = Object.keys(content).filter((key) => mediaType(key)).sort();
  if (keys.length === 0) return null;
  const json = keys.find((key) => key.toLowerCase().split(";", 1)[0] === "application/json");
  return json ?? keys.find((key) => key.toLowerCase().split(";", 1)[0].endsWith("+json"))
    ?? keys.find((key) => key.toLowerCase().split(";", 1)[0] === "application/x-www-form-urlencoded")
    ?? keys.find((key) => key.toLowerCase().split(";", 1)[0] === "multipart/form-data")
    ?? keys[0];
}

function parameterValue(parameter: Record<string, unknown>, name: string): { value: string; source: ExampleValue["source"]; redacted: boolean; warning: OpenApiIssueCode | null } {
  if (isSensitiveName(name)) return { value: "", source: "empty", redacted: true, warning: null };
  const example = exampleFrom(parameter);
  if (!example.found) return { value: "", source: "empty", redacted: false, warning: null };
  if (typeof example.value === "string" && isSensitiveValue(example.value)) {
    return { value: "", source: "empty", redacted: true, warning: null };
  }
  const value = safeScalar(example.value);
  if (value === null) return { value: "", source: "empty", redacted: false, warning: "PARAMETER_EXAMPLE_OMITTED" };
  return { value, source: example.source, redacted: false, warning: null };
}

interface ParameterResult {
  previews: OpenApiParameterPreview[];
  headers: RequestHeader[];
  cookies: RequestTemplate["cookies"];
  params: KeyValue[];
  warnings: OpenApiIssue[];
  errors: OpenApiIssue[];
  /** Raw declarations are counted as well as valid rows for the document bound. */
  declaredCount: number;
}

function parametersFor(
  pathItem: Record<string, unknown>,
  operation: Record<string, unknown>,
  path: string,
  method: string,
): ParameterResult {
  const warnings: OpenApiIssue[] = [];
  const errors: OpenApiIssue[] = [];
  let declaredCount = 0;
  const merged = new Map<string, Record<string, unknown>>();
  const seenOperation = new Set<string>();
  const scopes = [own(pathItem, "parameters"), own(operation, "parameters")];
  for (let scopeIndex = 0; scopeIndex < scopes.length; scopeIndex += 1) {
    const list = scopes[scopeIndex];
    if (list === undefined) continue;
    if (!Array.isArray(list)) {
      errors.push(issue("PARAMETER_INVALID", "operation", path, method));
      continue;
    }
    declaredCount += list.length;
    for (const raw of list) {
      if (!isRecord(raw) || hasRef(raw)) {
        errors.push(issue(hasRef(raw) ? "UNSUPPORTED_REF" : "PARAMETER_INVALID", "operation", path, method));
        continue;
      }
      const location = own(raw, "in");
      const name = own(raw, "name");
      if ((location !== "path" && location !== "query" && location !== "header" && location !== "cookie") || typeof name !== "string" || !validParameterName(name, location)) {
        errors.push(issue("PARAMETER_INVALID", "operation", path, method));
        continue;
      }
      if (location === "path" && !path.includes(`{${name}}`)) {
        errors.push(issue("PARAMETER_INVALID", "operation", path, method));
        continue;
      }
      if (location === "path" && own(raw, "required") !== true) {
        errors.push(issue("PARAMETER_INVALID", "operation", path, method));
        continue;
      }
      const identity = `${location}\u0000${location === "header" ? name.toLowerCase() : name}`;
      if (scopeIndex === 1 && seenOperation.has(identity)) {
        errors.push(issue("DUPLICATE_PARAMETER", "operation", path, method));
        continue;
      }
      if (scopeIndex === 0 && merged.has(identity)) {
        errors.push(issue("DUPLICATE_PARAMETER", "operation", path, method));
        continue;
      }
      if (scopeIndex === 1) seenOperation.add(identity);
      merged.set(identity, raw);
    }
  }

  const entries = [...merged.values()].sort((left, right) => {
    const leftLocation = String(own(left, "in"));
    const rightLocation = String(own(right, "in"));
    const rank = (location: string) => ["path", "query", "header", "cookie"].indexOf(location);
    return rank(leftLocation) - rank(rightLocation) || compareText(String(own(left, "name")), String(own(right, "name")));
  });
  const previews: OpenApiParameterPreview[] = [];
  const headers: RequestHeader[] = [];
  const cookies: RequestTemplate["cookies"] = [];
  const params: KeyValue[] = [];
  for (const parameter of entries) {
    const location = own(parameter, "in") as OpenApiParameterLocation;
    const name = own(parameter, "name") as string;
    const resolved = parameterValue(parameter, name);
    previews.push({ name, location, value: resolved.value, redacted: resolved.redacted, source: resolved.source });
    if (resolved.warning) warnings.push(issue(resolved.warning, "operation", path, method));
    if (location === "path") continue;
    if (location === "header") headers.push({ key: name, value: resolved.value, enabled: true });
    else if (location === "cookie") cookies.push({ name, value: resolved.value, enabled: true });
    else params.push({ key: name, value: resolved.value });
  }
  if (
    headers.length > MAX_REQUEST_HEADER_ROWS
    || cookies.length > MAX_REQUEST_COOKIE_ROWS
    || params.length > OPENAPI_LIMITS.maxRequestRows
  ) {
    errors.push(issue("REQUEST_ROW_LIMIT", "operation", path, method));
  }
  return {
    previews,
    headers: headers.slice(0, MAX_REQUEST_HEADER_ROWS),
    cookies: cookies.slice(0, MAX_REQUEST_COOKIE_ROWS),
    params: params.slice(0, OPENAPI_LIMITS.maxRequestRows),
    warnings,
    errors,
    declaredCount,
  };
}

function applyPathParameterExamples(url: string, parameters: OpenApiParameterPreview[]): string {
  let resolved = url;
  for (const parameter of parameters) {
    if (parameter.location !== "path" || parameter.redacted || !parameter.value) continue;
    const encoded = encodeURIComponent(parameter.value);
    resolved = resolved.split(`{${parameter.name}}`).join(encoded);
  }
  return resolved;
}

function pathTemplateParameters(path: string): string[] {
  const names: string[] = [];
  const matcher = /\{([A-Za-z0-9_.-]{1,120})\}/g;
  for (const match of path.matchAll(matcher)) names.push(match[1]);
  return names;
}

interface SecurityResult {
  auth: AuthConfig;
  metadata: OpenApiSecurityPreview | null;
  params: KeyValue[];
  headers: RequestHeader[];
  cookies: RequestTemplate["cookies"];
  warnings: OpenApiIssue[];
  errors: OpenApiIssue[];
}

interface InternalOperationPreview extends OpenApiOperationPreview {
  declaredParameterCount: number;
}

function securityFor(
  rawSecurity: unknown,
  schemes: Record<string, unknown>,
  path: string,
  method: string,
): SecurityResult {
  const none: SecurityResult = { auth: emptyAuth(), metadata: null, params: [], headers: [], cookies: [], warnings: [], errors: [] };
  if (rawSecurity === undefined) return none;
  if (!Array.isArray(rawSecurity)) return { ...none, errors: [issue("SECURITY_INVALID", "operation", path, method)] };
  if (rawSecurity.length === 0) return none;
  let unsupported = false;
  for (const requirement of rawSecurity) {
    if (!isRecord(requirement)) {
      unsupported = true;
      continue;
    }
    const names = Object.keys(requirement).sort();
    if (names.length === 0) return none;
    // RequestTemplate has one auth slot; silently dropping an AND requirement
    // would create a materially different request, so isolate it instead.
    if (names.length > 1) {
      unsupported = true;
      continue;
    }
    const resolved: SecurityResult = { ...none, params: [], headers: [], cookies: [], warnings: [], errors: [] };
    let valid = true;
    for (const name of names) {
      const scheme = schemes[name];
      if (!isRecord(scheme) || hasRef(scheme)) {
        valid = false;
        unsupported = true;
        break;
      }
      const type = own(scheme, "type");
      const httpScheme = typeof own(scheme, "scheme") === "string"
        ? (own(scheme, "scheme") as string).toLowerCase()
        : null;
      if (type === "http" && httpScheme === "basic") {
        resolved.auth = { ...emptyAuth(), kind: "basic" };
        resolved.metadata = { kind: "basic", location: "header", name: "Authorization", valuesInjected: false };
      } else if (type === "http" && httpScheme === "bearer") {
        resolved.auth = { ...emptyAuth(), kind: "bearer" };
        resolved.metadata = { kind: "bearer", location: "header", name: "Authorization", valuesInjected: false };
      } else if (type === "apiKey") {
        const location = own(scheme, "in");
        const key = own(scheme, "name");
        if ((location !== "header" && location !== "query" && location !== "cookie") || typeof key !== "string" || !validParameterName(key, location)) {
          valid = false;
          unsupported = true;
          break;
        }
        resolved.metadata = { kind: "apikey", location, name: key, valuesInjected: false };
        if (location === "header") resolved.auth = { ...emptyAuth(), kind: "apikey", api_key: key };
        else if (location === "query") resolved.params.push({ key, value: "" });
        else resolved.cookies.push({ name: key, value: "", enabled: true });
      } else {
        valid = false;
        unsupported = true;
        break;
      }
    }
    if (valid) {
      if (unsupported) resolved.warnings.push(issue("SECURITY_UNSUPPORTED", "operation", path, method));
      return resolved;
    }
  }
  return { ...none, errors: [issue("SECURITY_UNSUPPORTED", "operation", path, method)] };
}

function operationPreview(
  path: string,
  method: string,
  pathItem: Record<string, unknown>,
  operation: unknown,
  servers: OpenApiServerPreview[],
  securitySchemes: Record<string, unknown>,
  rootSecurity: unknown,
  operationIndex: number,
): InternalOperationPreview {
  const errors: OpenApiIssue[] = [];
  const warnings: OpenApiIssue[] = [];
  const server = servers[0];
  const url = server ? joinServerPath(server.url, path) ?? path : path;
  const request = emptyRequest(method.toUpperCase(), url);
  // Inspect only the path item's own `$ref`. Recursing through the full path
  // item would see references inside sibling methods and break operation-level
  // error isolation.
  const pathItemHasRef = Object.prototype.hasOwnProperty.call(pathItem, "$ref");
  if (!isRecord(operation) || hasRef(operation) || pathItemHasRef) {
    errors.push(issue(hasRef(operation) || pathItemHasRef ? "UNSUPPORTED_REF" : "OPERATION_INVALID", "operation", path, method));
  }
  const operationRecord = isRecord(operation) ? operation : Object.create(null) as Record<string, unknown>;
  if (!server) errors.push(issue("NO_SERVER", "operation", path, method));
  if (server && !joinServerPath(server.url, path)) errors.push(issue("SERVER_INVALID", "operation", path, method));
  // Existing RequestTemplate has one document-wide server selector. Applying
  // a path/operation override without representing its precedence would make
  // the preview point at a different endpoint than the source describes.
  if (Object.prototype.hasOwnProperty.call(pathItem, "servers") || Object.prototype.hasOwnProperty.call(operationRecord, "servers")) {
    errors.push(issue("SERVER_OVERRIDE_UNSUPPORTED", "operation", path, method));
  }

  const parameterResult = parametersFor(pathItem, operationRecord, path, method);
  warnings.push(...parameterResult.warnings);
  errors.push(...parameterResult.errors);
  request.headers.push(...parameterResult.headers);
  request.cookies.push(...parameterResult.cookies);
  request.params.push(...parameterResult.params);
  const declaredPathParameters = new Set(
    parameterResult.previews.filter((parameter) => parameter.location === "path").map((parameter) => parameter.name),
  );
  const templateParameters = pathTemplateParameters(path);
  if (new Set(templateParameters).size !== templateParameters.length) {
    errors.push(issue("DUPLICATE_PARAMETER", "operation", path, method));
  }
  for (const name of templateParameters) {
    if (!declaredPathParameters.has(name)) errors.push(issue("PARAMETER_INVALID", "operation", path, method));
  }
  request.url = applyPathParameterExamples(request.url, parameterResult.previews);

  const rawBody = own(operationRecord, "requestBody");
  let requestBody: OpenApiRequestBodyPreview | null = null;
  if (rawBody !== undefined) {
    if (!isRecord(rawBody) || hasRef(rawBody)) {
      errors.push(issue(hasRef(rawBody) ? "UNSUPPORTED_REF" : "REQUEST_BODY_INVALID", "operation", path, method));
    } else {
      const content = own(rawBody, "content");
      if (!isRecord(content)) {
        errors.push(issue("REQUEST_BODY_INVALID", "operation", path, method));
      } else {
        const mediaKeys = Object.keys(content);
        if (mediaKeys.length > OPENAPI_LIMITS.maxMediaTypes) errors.push(issue("MEDIA_TYPE_LIMIT", "operation", path, method));
        const media = chooseMedia(content);
        if (media) {
          const mediaContent = content[media];
          if (!isRecord(mediaContent) || hasRef(mediaContent)) {
            errors.push(issue(hasRef(mediaContent) ? "UNSUPPORTED_REF" : "REQUEST_BODY_INVALID", "operation", path, method));
          } else {
            const body = bodyForMedia(media, mediaContent);
            request.body_kind = body.kind;
            request.body = body.body;
            request.multipart = body.multipart;
            requestBody = { mediaType: media, exampleIncluded: body.exampleIncluded, redacted: body.redacted };
            if (body.warning) warnings.push(issue(body.warning, "operation", path, method));
            if (body.kind !== "none" && body.kind !== "multipart" && !request.headers.some((header) => header.key.toLowerCase() === "content-type")) {
              request.headers.push({ key: "Content-Type", value: media, enabled: true });
            }
          }
        } else {
          errors.push(issue("REQUEST_BODY_INVALID", "operation", path, method));
        }
      }
    }
  }

  const operationSecurity = Object.prototype.hasOwnProperty.call(operationRecord, "security")
    ? own(operationRecord, "security")
    : rootSecurity;
  const security = securityFor(operationSecurity, securitySchemes, path, method);
  request.auth = security.auth;
  request.params.push(...security.params);
  request.headers.push(...security.headers);
  request.cookies.push(...security.cookies);
  warnings.push(...security.warnings);
  errors.push(...security.errors);

  // Body and security metadata can add derived rows after the parameter
  // pass. Apply the cap again at the final request boundary so an otherwise
  // valid document cannot produce an oversized editor or request payload.
  if (
    request.headers.length > MAX_REQUEST_HEADER_ROWS
    || request.cookies.length > MAX_REQUEST_COOKIE_ROWS
    || request.params.length > OPENAPI_LIMITS.maxRequestRows
  ) {
    errors.push(issue("REQUEST_ROW_LIMIT", "operation", path, method));
    request.headers = request.headers.slice(0, MAX_REQUEST_HEADER_ROWS);
    request.cookies = request.cookies.slice(0, MAX_REQUEST_COOKIE_ROWS);
    request.params = request.params.slice(0, OPENAPI_LIMITS.maxRequestRows);
  }

  request.headers.sort((left, right) => compareText(left.key, right.key));
  request.params.sort((left, right) => compareText(left.key, right.key));
  request.cookies.sort((left, right) => compareText(left.name, right.name));
  const uniqueIssues = (entries: OpenApiIssue[]) => entries.filter(
    (entry, index) => entries.findIndex((candidate) => candidate.code === entry.code) === index,
  );
  const uniqueErrors = uniqueIssues(errors);
  const uniqueWarnings = uniqueIssues(warnings.filter((entry) => !uniqueErrors.some((error) => error.code === entry.code)));
  const id = `openapi-${operationIndex + 1}`;
  return {
    id,
    path,
    method: method.toUpperCase(),
    label: `${method.toUpperCase()} ${path}`,
    serverIndex: server?.index ?? null,
    request,
    parameters: parameterResult.previews,
    requestBody,
    security: security.metadata,
    warnings: uniqueWarnings,
    errors: uniqueErrors,
    applyable: uniqueErrors.length === 0,
    declaredParameterCount: parameterResult.declaredCount,
  };
}

function parsePreview(source: unknown, format: OpenApiFormat): OpenApiImportPreview {
  if (!isRecord(source)) throw issue("ROOT_INVALID", "document");
  const version = versionOf(own(source, "openapi"));
  if (!version) throw issue(typeof own(source, "openapi") === "string" ? "VERSION_UNSUPPORTED" : "ROOT_INVALID", "document");
  const info = own(source, "info");
  if (!isRecord(info) || typeof own(info, "title") !== "string" || typeof own(info, "version") !== "string" || !isRecord(own(source, "paths"))) {
    throw issue("ROOT_INVALID", "document");
  }

  const rawServers = own(source, "servers");
  const servers: OpenApiServerPreview[] = [];
  const errors: OpenApiIssue[] = [];
  if (rawServers !== undefined && !Array.isArray(rawServers)) errors.push(issue("SERVER_INVALID", "document"));
  if (Array.isArray(rawServers)) {
    if (rawServers.length > OPENAPI_LIMITS.maxServers) throw issue("SERVER_LIMIT", "document");
    rawServers.forEach((raw, index) => {
      try {
        const url = serverUrlWithVariables(raw);
        if (!url) throw new Error("invalid");
        servers.push({ index, url });
      } catch (cause) {
        errors.push(isOpenApiIssue(cause) ? cause : issue("SERVER_INVALID", "document"));
      }
    });
  }

  const components = own(source, "components");
  const securitySchemes = isRecord(components) && isRecord(own(components, "securitySchemes"))
    ? own(components, "securitySchemes") as Record<string, unknown>
    : Object.create(null) as Record<string, unknown>;
  if (Object.keys(securitySchemes).length > OPENAPI_LIMITS.maxSecuritySchemes) throw issue("SECURITY_SCHEME_LIMIT", "document");
  const paths = own(source, "paths") as Record<string, unknown>;
  const pathKeys = Object.keys(paths).filter((key) => key.startsWith("/") || key === "").sort();
  if (pathKeys.length > OPENAPI_LIMITS.maxPaths) throw issue("PATH_LIMIT", "document");
  const allPathKeys = Object.keys(paths);
  if (allPathKeys.some((key) => !key.startsWith("/") && !key.startsWith("x-"))) errors.push(issue("PATH_INVALID", "document"));

  let operationCount = 0;
  let parameterCount = 0;
  const operations: OpenApiOperationPreview[] = [];
  const rootSecurity = own(source, "security");
  for (const path of pathKeys) {
    if (!isSafePath(path)) {
      errors.push(issue("PATH_INVALID", "document"));
      continue;
    }
    const pathItem = paths[path];
    if (!isRecord(pathItem)) {
      errors.push(issue("PATH_INVALID", "document", path));
      continue;
    }
    const operationKeys = Object.keys(pathItem).filter((key) => !PATH_ITEM_METADATA.has(key) && !key.startsWith("x-"));
    operationCount += operationKeys.length;
    if (operationCount > OPENAPI_LIMITS.maxOperations) throw issue("OPERATION_LIMIT", "document");
    const methods = sortedMethods(pathItem);
    for (const method of methods) {
      const { declaredParameterCount, ...operation } = operationPreview(path, method, pathItem, pathItem[method], servers, securitySchemes, rootSecurity, operations.length);
      parameterCount += declaredParameterCount;
      if (parameterCount > OPENAPI_LIMITS.maxParameters) throw issue("PARAMETER_LIMIT", "document");
      operations.push(operation);
    }
    const unsupportedMethods = Object.keys(pathItem).filter((key) => !METHOD_SET.has(key) && !PATH_ITEM_METADATA.has(key) && !key.startsWith("x-"));
    for (const unsupportedMethod of unsupportedMethods) {
      errors.push(issue("METHOD_UNSUPPORTED", "operation", path, unsupportedMethod.toUpperCase()));
    }
  }
  return { format, version, servers, operations, errors, sourceName: undefined };
}

export function parseOpenApi(text: string, format: OpenApiFormat = "yaml"): OpenApiImportResult {
  try {
    const source = parseSource(text, format);
    return { ok: true, preview: parsePreview(source, format) };
  } catch (cause) {
    return { ok: false, error: isOpenApiIssue(cause) ? cause : issue("PARSER_ERROR", "document") };
  }
}

/** Alias kept explicit for callers that prefer the document-oriented name. */
export const parseOpenApiDocument = parseOpenApi;

export function parseOpenApiSource(source: OpenApiSource): OpenApiImportResult {
  const format = source.kind === "url" ? source.format : source.format ?? detectOpenApiFormat(source.name);
  const result = parseOpenApi(source.text, format);
  return result.ok
    ? {
      ok: true,
      preview: {
        ...result.preview,
        sourceName: source.kind === "file" ? safeFileName(source.name) : `remote-openapi.${format === "json" ? "json" : "yaml"}`,
      },
    }
    : result;
}

export function selectOpenApiServer(preview: OpenApiImportPreview, serverIndex: number): OpenApiImportPreview {
  const server = preview.servers.find((candidate) => candidate.index === serverIndex);
  if (!server) return preview;
  return {
    ...preview,
    operations: preview.operations.map((operation) => {
      const url = joinServerPath(server.url, operation.path);
      if (!url) {
        const errors = [...operation.errors.filter((entry) => entry.code !== "SERVER_INVALID"), issue("SERVER_INVALID", "operation", operation.path, operation.method)];
        return { ...operation, serverIndex, request: { ...operation.request, url: operation.path }, errors, applyable: false };
      }
      const errors = operation.errors.filter((entry) => entry.code !== "NO_SERVER" && entry.code !== "SERVER_INVALID");
      return {
        ...operation,
        serverIndex,
        request: { ...operation.request, url: applyPathParameterExamples(url, operation.parameters) },
        errors,
        applyable: errors.length === 0,
      };
    }),
  };
}
