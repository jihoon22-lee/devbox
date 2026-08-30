import {
  OPENAPI_DOCUMENT_LIMITS,
  parseBoundedOpenApiDocument,
  type OpenApiDocumentFormat,
} from "@devbox/openapi";
import type { ResponseRule } from "../api";

const MAX_PATHS = 250;
const MAX_OPERATIONS = 1_000;
const MAX_PATH_BYTES = 16_384;
const MAX_SOURCE_NAME_CHARS = 120;
const METHODS = ["get", "post", "put", "patch", "delete", "options", "head", "trace"] as const;

export type OpenApiRuleIssueCode =
  | "DOCUMENT_INVALID"
  | "VERSION_UNSUPPORTED"
  | "ROOT_INVALID"
  | "PATH_LIMIT"
  | "OPERATION_LIMIT";

export type OpenApiRuleSkipReason =
  | "pathParametersUnsupported"
  | "pathUnsupported"
  | "operationInvalid"
  | "referenceUnsupported";

export interface OpenApiRuleOperation {
  id: string;
  method: string;
  path: string;
  status: number;
  applyable: boolean;
  reason: OpenApiRuleSkipReason | null;
}

export interface OpenApiRulePreview {
  sourceName: string;
  version: "3.0" | "3.1";
  operations: OpenApiRuleOperation[];
}

export type OpenApiRulePreviewResult =
  | { ok: true; preview: OpenApiRulePreview }
  | { ok: false; code: OpenApiRuleIssueCode; message: string };

const MESSAGES: Readonly<Record<OpenApiRuleIssueCode, string>> = {
  DOCUMENT_INVALID: "OpenAPI JSON/YAML 문서를 안전하게 읽을 수 없습니다.",
  VERSION_UNSUPPORTED: "OpenAPI 3.0 또는 3.1 문서만 사용할 수 있습니다.",
  ROOT_INVALID: "OpenAPI paths 구조가 올바르지 않습니다.",
  PATH_LIMIT: "OpenAPI path 수가 250개 제한을 초과했습니다.",
  OPERATION_LIMIT: "OpenAPI operation 수가 1,000개 제한을 초과했습니다.",
};

function error(code: OpenApiRuleIssueCode): OpenApiRulePreviewResult {
  return { ok: false, code, message: MESSAGES[code] };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  if (value === null || typeof value !== "object" || Array.isArray(value)) return false;
  const prototype = Object.getPrototypeOf(value);
  return prototype === null || prototype === Object.prototype;
}

function own(record: Record<string, unknown>, key: string): unknown {
  return Object.prototype.hasOwnProperty.call(record, key) ? record[key] : undefined;
}

function versionOf(value: unknown): "3.0" | "3.1" | null {
  if (typeof value !== "string") return null;
  if (/^3\.0(?:\.\d+)?$/.test(value)) return "3.0";
  if (/^3\.1(?:\.\d+)?$/.test(value)) return "3.1";
  return null;
}

function byteLength(value: string): number {
  return new TextEncoder().encode(value).byteLength;
}

function safePath(path: string): boolean {
  return path.startsWith("/")
    && path.length <= OPENAPI_DOCUMENT_LIMITS.maxStringLength
    && byteLength(path) <= MAX_PATH_BYTES
    && /^[\x20-\x7e]+$/u.test(path)
    && !/[\u0000-\u001f\u007f]/.test(path)
    && !path.includes("?")
    && !path.includes("#")
    && !path.includes("*")
    && !path.split("/").some((segment) => segment === "." || segment === "..");
}

function safeSourceName(value: string): string {
  const basename = value.split("\\").join("/").split("/").pop() ?? "openapi.yaml";
  const clean = [...basename]
    .filter((character) => {
      const code = character.codePointAt(0) ?? 0;
      return code >= 0x20 && code !== 0x7f;
    })
    .join("")
    .trim();
  return [...(clean || "openapi.yaml")].slice(0, MAX_SOURCE_NAME_CHARS).join("");
}

function responseStatus(operation: Record<string, unknown>): number {
  const responses = own(operation, "responses");
  if (!isRecord(responses)) return 200;
  const success = Object.keys(responses)
    .filter((key) => /^2\d\d$/.test(key))
    .map(Number)
    .filter((value) => value >= 200 && value <= 299)
    .sort((left, right) => left - right);
  return success[0] ?? 200;
}

export function previewOpenApiRules(
  text: string,
  format: OpenApiDocumentFormat,
  sourceName: string,
): OpenApiRulePreviewResult {
  const parsed = parseBoundedOpenApiDocument(text, format);
  if (!parsed.ok || !isRecord(parsed.value)) return error("DOCUMENT_INVALID");
  const version = versionOf(own(parsed.value, "openapi"));
  if (!version) return error("VERSION_UNSUPPORTED");
  const paths = own(parsed.value, "paths");
  if (!isRecord(paths)) return error("ROOT_INVALID");
  const pathNames = Object.keys(paths).sort();
  if (pathNames.length > MAX_PATHS) return error("PATH_LIMIT");

  const operations: OpenApiRuleOperation[] = [];
  for (const path of pathNames) {
    const pathItem = paths[path];
    const pathSafe = safePath(path);
    const hasParameters = path.includes("{") || path.includes("}");
    if (!isRecord(pathItem)) continue;
    for (const method of METHODS) {
      if (!Object.prototype.hasOwnProperty.call(pathItem, method)) continue;
      if (operations.length >= MAX_OPERATIONS) return error("OPERATION_LIMIT");
      const operation = pathItem[method];
      let reason: OpenApiRuleSkipReason | null = null;
      if (!pathSafe) reason = "pathUnsupported";
      else if (hasParameters) reason = "pathParametersUnsupported";
      else if (!isRecord(operation)) reason = "operationInvalid";
      else if (Object.prototype.hasOwnProperty.call(operation, "$ref")) {
        reason = "referenceUnsupported";
      }
      operations.push({
        id: `${method}:${path}`,
        method: method.toUpperCase(),
        path,
        status: isRecord(operation) ? responseStatus(operation) : 200,
        applyable: reason === null,
        reason,
      });
    }
  }

  return {
    ok: true,
    preview: {
      sourceName: safeSourceName(sourceName),
      version,
      operations,
    },
  };
}

export function openApiOperationToRule(operation: OpenApiRuleOperation): ResponseRule | null {
  if (!operation.applyable) return null;
  return {
    id: "",
    priority: 0,
    method: operation.method,
    path: operation.path,
    status: operation.status,
    headers: [],
    body: "",
    delayMs: 0,
    sequence: [],
  };
}
