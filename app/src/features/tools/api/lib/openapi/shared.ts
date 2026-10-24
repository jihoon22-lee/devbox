import type { AuthConfig, RequestTemplate } from "../template";
import { parseBoundedOpenApiDocument } from "./document";
import { OPENAPI_LIMITS } from "./limits";

/**
 * OpenAPI parsing stays a local, bounded transform. Native URL retrieval is a
 * separate command; neither source path resolves a $ref or turns an example
 * into a secret-bearing request. Keep these limits in one place so every
 * source enforces the same parser contract.
 */

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

export type OpenApiImportResult = { ok: true; preview: OpenApiImportPreview } | { ok: false; error: OpenApiIssue };

export type OpenApiSource =
  | { kind: "file"; name: string; format?: OpenApiFormat; text: string }
  | { kind: "url"; format: OpenApiFormat; text: string };

export const METHOD_ORDER = ["get", "post", "put", "patch", "delete"] as const;
export const METHOD_SET = new Set<string>(METHOD_ORDER);
export const PATH_ITEM_METADATA = new Set(["parameters", "$ref", "summary", "description", "servers"]);
export const HTTP_TOKEN = /^[!#$%&'*+.^_`|~0-9A-Za-z-]+$/;
export const SENSITIVE_NAME =
  /(authorization|proxy-authorization|cookie|set-cookie|api[-_]?key|access[-_]?(?:key|token)|client[-_]?key|refresh[-_]?token|token|secret|password|passwd|credential|private[-_]?key|user[-_]?name|(?:^|[-_.])key(?:$|[-_.]))/i;
export const KNOWN_CREDENTIAL =
  /(?:sk-|ghp_|github_pat_|glpat-|xox[baprs]-)[A-Za-z0-9_.-]{12,}|AKIA[A-Z0-9]{16}|(?:[A-Za-z0-9_-]{10,}\.){2}[A-Za-z0-9_-]{10,}/;
export const SECRET_REFERENCE = /\{\{\s*[A-Za-z0-9_.-]+\s*\}\}|\$\{\s*[A-Za-z0-9_.-]+\s*\}/;
export const AUTH_VALUE = /^(?:bearer|basic)\s+\S+$/i;

export const ISSUE_MESSAGES: Readonly<Record<OpenApiIssueCode, string>> = {
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

export function issue(code: OpenApiIssueCode, scope: OpenApiIssueScope, path?: string, method?: string): OpenApiIssue {
  return { code, message: ISSUE_MESSAGES[code], scope, ...(path ? { path } : {}), ...(method ? { method } : {}) };
}

export function byteLength(value: string): number {
  return new TextEncoder().encode(value).byteLength;
}

export function compareText(left: string, right: string): number {
  return left < right ? -1 : left > right ? 1 : 0;
}

export function isRecord(value: unknown): value is Record<string, unknown> {
  if (value === null || typeof value !== "object" || Array.isArray(value)) return false;
  const prototype = Object.getPrototypeOf(value);
  return prototype === Object.prototype || prototype === null;
}

export function own(value: Record<string, unknown>, key: string): unknown {
  return Object.prototype.hasOwnProperty.call(value, key) ? value[key] : undefined;
}

export function safeFileName(value: string): string {
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

export function parseSource(text: string, format: OpenApiFormat): unknown {
  const parsed = parseBoundedOpenApiDocument(text, format);
  if (parsed.ok) return parsed.value;
  throw issue(parsed.error.code, "document");
}

export function isOpenApiIssue(value: unknown): value is OpenApiIssue {
  return (
    isRecord(value) &&
    typeof value.code === "string" &&
    typeof value.message === "string" &&
    (value.scope === "document" || value.scope === "operation")
  );
}

export function versionOf(value: unknown): "3.0" | "3.1" | null {
  if (typeof value !== "string") return null;
  if (/^3\.0(?:\.\d+)?$/.test(value)) return "3.0";
  if (/^3\.1(?:\.\d+)?$/.test(value)) return "3.1";
  return null;
}

export function isSafePath(value: string): boolean {
  if (!value.startsWith("/") || value.length > OPENAPI_LIMITS.maxStringLength || isSensitiveValue(value)) return false;
  const segments = value.split("/");
  if (segments.some((segment) => segment === "." || segment === "..")) return false;
  try {
    const decoded = decodeURIComponent(value);
    if (
      isSensitiveValue(decoded) ||
      decoded.includes("\\") ||
      decoded.includes("?") ||
      decoded.includes("#") ||
      /[\u0000-\u001f\u007f]/.test(decoded) ||
      decoded.split("/").some((segment) => segment === "." || segment === "..")
    )
      return false;
  } catch {
    return false;
  }
  for (let index = 0; index < value.length; index += 1) {
    const character = value[index]!;
    if (character === "\\" || character === "?" || character === "#" || /[\u0000-\u001f\u007f]/.test(character))
      return false;
    if (character === "}") return false;
    if (character === "{") {
      const end = value.indexOf("}", index + 1);
      if (end < 0 || !/^[A-Za-z0-9_.-]{1,120}$/.test(value.slice(index + 1, end))) return false;
      index = end;
    }
  }
  return true;
}

export function isSensitiveName(value: string): boolean {
  return SENSITIVE_NAME.test(value);
}

export function containsRawCredential(value: string): boolean {
  return KNOWN_CREDENTIAL.test(value);
}

export function isSensitiveValue(value: string): boolean {
  return containsRawCredential(value) || SECRET_REFERENCE.test(value) || AUTH_VALUE.test(value);
}

export function validParameterName(value: string, location: OpenApiParameterLocation): boolean {
  if (value.length === 0 || value.length > OPENAPI_LIMITS.maxParameterNameLength || /[\u0000-\u001f\u007f]/.test(value))
    return false;
  return location === "query" || HTTP_TOKEN.test(value);
}

export function safeScalar(value: unknown): string | null {
  if (typeof value === "string")
    return value.length <= OPENAPI_LIMITS.maxStringLength &&
      !/[\u0000-\u001f\u007f]/.test(value) &&
      !isSensitiveValue(value)
      ? value
      : null;
  if (typeof value === "number" && Number.isFinite(value)) return String(value);
  if (typeof value === "boolean") return String(value);
  if (value === null) return "null";
  return null;
}

export interface ExampleValue {
  found: boolean;
  value: unknown;
  source: "example" | "default" | "enum" | "empty";
}

export function exampleFrom(value: Record<string, unknown>): ExampleValue {
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

export function mediaType(value: string): boolean {
  return (
    value.length <= 256 &&
    /^[A-Za-z0-9!#$&^_.+-]+\/[A-Za-z0-9!#$&^_.+-]+(?:\s*;\s*[A-Za-z0-9!#$&^_.+-]+=[A-Za-z0-9!#$&^_.+-]+)*$/.test(value)
  );
}

export function hasRef(value: unknown): boolean {
  if (Array.isArray(value)) return value.some(hasRef);
  if (!isRecord(value)) return false;
  // A malformed non-string `$ref` is not safe to ignore either. Treat any
  // explicit reference member as unsupported at the owning operation.
  if (Object.prototype.hasOwnProperty.call(value, "$ref")) return true;
  return Object.keys(value).some((key) => hasRef(value[key]));
}

export function emptyAuth(): AuthConfig {
  return { kind: "none", username: "", password: "", token: "", api_key: "", api_value: "" };
}

export function emptyRequest(method: string, url: string): RequestTemplate {
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

export function serverUrl(value: unknown): string | null {
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

export function serverUrlWithVariables(value: unknown): string | null {
  if (!isRecord(value)) return null;
  const raw = own(value, "url");
  if (typeof raw !== "string" || raw.length === 0 || raw.length > OPENAPI_LIMITS.maxStringLength) return null;
  if (/[\u0000-\u0020\u007f]/.test(raw)) return null;
  const variableRecord = own(value, "variables");
  const variables = isRecord(variableRecord) ? variableRecord : (Object.create(null) as Record<string, unknown>);
  const substituted = raw.replace(/\{([A-Za-z0-9_.-]{1,120})\}/g, (_match, name: string) => {
    const variable = variables[name];
    if (!isRecord(variable) || typeof variable.default !== "string" || isSensitiveName(name))
      throw issue("SERVER_VARIABLE_INVALID", "document");
    if (
      variable.default.length > OPENAPI_LIMITS.maxStringLength ||
      /[\u0000-\u0020\u007f]/.test(variable.default) ||
      isSensitiveValue(variable.default)
    )
      throw issue("SERVER_VARIABLE_INVALID", "document");
    return variable.default;
  });
  return serverUrl(substituted);
}

export function joinServerPath(server: string, path: string): string | null {
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

export function sortedMethods(pathItem: Record<string, unknown>): string[] {
  return METHOD_ORDER.filter((method) => Object.prototype.hasOwnProperty.call(pathItem, method));
}
