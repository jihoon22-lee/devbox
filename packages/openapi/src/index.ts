import { visit as visitJson, type ParseError } from "jsonc-parser";
import { parseDocument } from "yaml";

export type OpenApiDocumentFormat = "json" | "yaml";

export const OPENAPI_DOCUMENT_LIMITS = Object.freeze({
  maxBytes: 4 * 1024 * 1024,
  maxDepth: 40,
  maxNodes: 50_000,
  maxStringLength: 16_384,
  maxAliases: 50,
});

export type OpenApiDocumentErrorCode =
  | "EMPTY_SOURCE"
  | "SOURCE_TOO_LARGE"
  | "PARSER_ERROR"
  | "UNSUPPORTED_GRAPH"
  | "NODE_LIMIT"
  | "DEPTH_LIMIT"
  | "STRING_LIMIT"
  | "DANGEROUS_KEY";

export interface OpenApiDocumentError {
  code: OpenApiDocumentErrorCode;
}

export type OpenApiDocumentResult =
  | { ok: true; value: unknown }
  | { ok: false; error: OpenApiDocumentError };

const DANGEROUS_KEYS = new Set(["__proto__", "prototype", "constructor"]);

function fail(code: OpenApiDocumentErrorCode): never {
  throw { code } satisfies OpenApiDocumentError;
}

function isFailure(value: unknown): value is OpenApiDocumentError {
  if (value === null || typeof value !== "object") return false;
  const code = (value as { code?: unknown }).code;
  return typeof code === "string" && [
    "EMPTY_SOURCE",
    "SOURCE_TOO_LARGE",
    "PARSER_ERROR",
    "UNSUPPORTED_GRAPH",
    "NODE_LIMIT",
    "DEPTH_LIMIT",
    "STRING_LIMIT",
    "DANGEROUS_KEY",
  ].includes(code);
}

function byteLength(value: string): number {
  return new TextEncoder().encode(value).byteLength;
}

function isPlainRecord(value: unknown): value is Record<string, unknown> {
  if (value === null || typeof value !== "object" || Array.isArray(value)) return false;
  const prototype = Object.getPrototypeOf(value);
  return prototype === Object.prototype || prototype === null;
}

/** Clone into null-prototype records while checking aliases, cycles and all bounds. */
function normalizeGraph(value: unknown): unknown {
  const active = new WeakSet<object>();
  let nodes = 0;

  const visit = (current: unknown, depth: number): unknown => {
    if (depth > OPENAPI_DOCUMENT_LIMITS.maxDepth) fail("DEPTH_LIMIT");
    nodes += 1;
    if (nodes > OPENAPI_DOCUMENT_LIMITS.maxNodes) fail("NODE_LIMIT");
    if (typeof current === "string") {
      if (current.length > OPENAPI_DOCUMENT_LIMITS.maxStringLength) fail("STRING_LIMIT");
      return current;
    }
    if (typeof current === "number") {
      if (!Number.isFinite(current) || (Number.isInteger(current) && !Number.isSafeInteger(current))) {
        fail("UNSUPPORTED_GRAPH");
      }
      return current;
    }
    if (typeof current === "bigint") {
      if (current > BigInt(Number.MAX_SAFE_INTEGER) || current < BigInt(Number.MIN_SAFE_INTEGER)) {
        fail("UNSUPPORTED_GRAPH");
      }
      return Number(current);
    }
    if (current === null || typeof current === "boolean") return current;
    if (typeof current !== "object") fail("UNSUPPORTED_GRAPH");
    if (active.has(current)) fail("UNSUPPORTED_GRAPH");
    active.add(current);
    try {
      if (Array.isArray(current)) return current.map((entry) => visit(entry, depth + 1));
      if (!isPlainRecord(current)) fail("UNSUPPORTED_GRAPH");
      const result = Object.create(null) as Record<string, unknown>;
      for (const key of Object.keys(current)) {
        nodes += 1;
        if (nodes > OPENAPI_DOCUMENT_LIMITS.maxNodes) fail("NODE_LIMIT");
        if (key.length > OPENAPI_DOCUMENT_LIMITS.maxStringLength) fail("STRING_LIMIT");
        if (DANGEROUS_KEYS.has(key)) fail("DANGEROUS_KEY");
        result[key] = visit(current[key], depth + 1);
      }
      return result;
    } finally {
      active.delete(current);
    }
  };

  return visit(value, 0);
}

function parseDocumentGraph(text: string, format: OpenApiDocumentFormat): unknown {
  if (!text.trim()) fail("EMPTY_SOURCE");
  if (byteLength(text) > OPENAPI_DOCUMENT_LIMITS.maxBytes) fail("SOURCE_TOO_LARGE");

  if (format === "json") {
    const errors: ParseError[] = [];
    let unsafeNumber = false;
    let parseDepth = 0;
    let parseNodes = 0;
    const countNode = () => {
      parseNodes += 1;
      if (parseNodes > OPENAPI_DOCUMENT_LIMITS.maxNodes) fail("NODE_LIMIT");
    };
    const beginContainer = () => {
      countNode();
      parseDepth += 1;
      if (parseDepth > OPENAPI_DOCUMENT_LIMITS.maxDepth) fail("DEPTH_LIMIT");
    };
    try {
      visitJson(text, {
        onObjectBegin: beginContainer,
        onObjectProperty: countNode,
        onObjectEnd: () => { parseDepth -= 1; },
        onArrayBegin: beginContainer,
        onArrayEnd: () => { parseDepth -= 1; },
        onLiteralValue: (value) => {
          countNode();
          if (typeof value === "number" && (!Number.isFinite(value)
            || (Number.isInteger(value) && !Number.isSafeInteger(value)))) {
            unsafeNumber = true;
          }
        },
        onError: (error, offset, length) => errors.push({ error, offset, length }),
      }, {
        allowEmptyContent: false,
        allowTrailingComma: false,
        disallowComments: true,
      });
    } catch (cause) {
      if (isFailure(cause)) throw cause;
      fail("PARSER_ERROR");
    }
    if (errors.length > 0 || unsafeNumber) fail("PARSER_ERROR");
  }

  let document: ReturnType<typeof parseDocument>;
  try {
    document = parseDocument(text, {
      intAsBigInt: true,
      merge: false,
      prettyErrors: false,
      resolveKnownTags: false,
      schema: format === "json" ? "json" : "core",
      strict: true,
      stringKeys: true,
      uniqueKeys: true,
      version: "1.2",
    });
  } catch {
    fail("PARSER_ERROR");
  }
  if (document.errors.length > 0 || document.warnings.length > 0) fail("PARSER_ERROR");
  try {
    return normalizeGraph(document.toJS({ maxAliasCount: OPENAPI_DOCUMENT_LIMITS.maxAliases }));
  } catch (cause) {
    if (isFailure(cause)) throw cause;
    fail("UNSUPPORTED_GRAPH");
  }
}

export function parseBoundedOpenApiDocument(
  text: string,
  format: OpenApiDocumentFormat,
): OpenApiDocumentResult {
  try {
    return { ok: true, value: parseDocumentGraph(text, format) };
  } catch (cause) {
    return { ok: false, error: isFailure(cause) ? cause : { code: "PARSER_ERROR" } };
  }
}
