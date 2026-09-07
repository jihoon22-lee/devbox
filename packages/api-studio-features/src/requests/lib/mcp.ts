const MAX_NAME_BYTES = 1024;
const MAX_URI_BYTES = 8 * 1024;
const MAX_LIST_ITEMS = 10_000;
const MAX_RETAINED_LIST_BYTES = 16 * 1024 * 1024;
const MAX_SCHEMA_DEPTH = 32;
const MAX_ARRAY_ITEMS = 100;
const MAX_STRING_LENGTH = 256 * 1024;
const MAX_DERIVED_PARAMETER_HEADERS = 100;

export type McpListKind = "tools" | "resources" | "resourceTemplates" | "prompts";

export interface McpListProjection {
  items: Record<string, unknown>[];
  identities: string[];
}

export interface McpSchemaAnalysis {
  mode: "form" | "json";
  schema: Record<string, unknown> | null;
  reason: string | null;
}

const IDENTITY_KEYS: Record<McpListKind, string> = {
  tools: "name",
  resources: "uri",
  resourceTemplates: "uriTemplate",
  prompts: "name",
};

const COMMON_SCHEMA_KEYS = new Set([
  "$schema",
  "title",
  "description",
  "type",
  "enum",
  "default",
  "x-mcp-header",
]);
const TYPE_SCHEMA_KEYS: Record<string, ReadonlySet<string>> = {
  object: new Set(["properties", "required", "additionalProperties"]),
  string: new Set(["minLength", "maxLength"]),
  integer: new Set(["minimum", "maximum"]),
  number: new Set(["minimum", "maximum"]),
  boolean: new Set(),
  array: new Set(["items", "minItems", "maxItems"]),
};

export function hasMcpCapability(
  capabilities: Record<string, unknown>,
  capability: "tools" | "resources" | "prompts",
): boolean {
  return isRecord(capabilities[capability]);
}

export function projectMcpListPage(result: unknown, kind: McpListKind): McpListProjection {
  if (!isRecord(result) || !Array.isArray(result[kind]) || result[kind].length > MAX_LIST_ITEMS) {
    throw new Error("mcp_message_invalid");
  }
  const identityKey = IDENTITY_KEYS[kind];
  const seen = new Set<string>();
  const items = result[kind].map((candidate) => {
    if (!isRecord(candidate)) throw new Error("mcp_message_invalid");
    const identity = candidate[identityKey];
    const max = kind === "resources" || kind === "resourceTemplates"
      ? MAX_URI_BYTES
      : MAX_NAME_BYTES;
    if (
      typeof identity !== "string"
      || identity.length === 0
      || utf8Bytes(identity) > max
      || hasControl(identity)
      || seen.has(identity)
    ) {
      throw new Error("mcp_message_invalid");
    }
    seen.add(identity);
    if (kind === "tools" && (!isRecord(candidate.inputSchema) || candidate.inputSchema.type !== "object")) {
      throw new Error("mcp_message_invalid");
    }
    if ((kind === "resources" || kind === "resourceTemplates") && (
      typeof candidate.name !== "string"
      || candidate.name.length === 0
      || utf8Bytes(candidate.name) > MAX_NAME_BYTES
      || hasControl(candidate.name)
    )) {
      throw new Error("mcp_message_invalid");
    }
    if (kind === "prompts") validatePromptArguments(candidate.arguments);
    return candidate;
  });
  return { items, identities: [...seen] };
}

export function appendMcpListPage(
  current: readonly Record<string, unknown>[],
  page: McpListProjection,
  kind: McpListKind,
): Record<string, unknown>[] {
  if (current.length + page.items.length > MAX_LIST_ITEMS) {
    throw new Error("mcp_response_too_large");
  }
  const identityKey = IDENTITY_KEYS[kind];
  const seen = new Set(current.map((item) => item[identityKey]));
  if (page.identities.some((identity) => seen.has(identity))) {
    throw new Error("mcp_message_invalid");
  }
  const combined = [...current, ...page.items];
  if (jsonBytes(combined) > MAX_RETAINED_LIST_BYTES) {
    throw new Error("mcp_response_too_large");
  }
  return combined;
}

export function analyzeMcpToolSchema(value: unknown): McpSchemaAnalysis {
  if (!isRecord(value)) {
    return { mode: "json", schema: null, reason: "inputSchema가 object가 아닙니다." };
  }
  try {
    validateSupportedSchema(value, 0, true);
    return { mode: "form", schema: value, reason: null };
  } catch {
    return {
      mode: "json",
      schema: value,
      reason: "지원하지 않는 JSON Schema 키워드 또는 구조가 있어 호출을 비활성화했습니다.",
    };
  }
}

export function initialMcpArguments(schema: Record<string, unknown>): Record<string, unknown> {
  const value = initialValue(schema, true);
  return isRecord(value) ? value : {};
}

export function initialMcpFieldValue(schema: Record<string, unknown>): unknown {
  return initialValue(schema);
}

export function getMcpValueAtPath(root: Record<string, unknown>, path: readonly string[]): unknown {
  let current: unknown = root;
  for (const segment of path) {
    if (!isRecord(current)) return undefined;
    current = current[segment];
  }
  return current;
}

export function setMcpValueAtPath(
  root: Record<string, unknown>,
  path: readonly string[],
  value: unknown,
): Record<string, unknown> {
  if (path.length === 0) return root;
  const output = structuredClone(root);
  let current = output;
  for (const segment of path.slice(0, -1)) {
    const child = current[segment];
    if (!isRecord(child)) current[segment] = {};
    current = current[segment] as Record<string, unknown>;
  }
  current[path[path.length - 1]] = value;
  return output;
}

export function removeMcpValueAtPath(
  root: Record<string, unknown>,
  path: readonly string[],
): Record<string, unknown> {
  if (path.length === 0) return root;
  const output = structuredClone(root);
  let current: Record<string, unknown> = output;
  for (const segment of path.slice(0, -1)) {
    const child: unknown = current[segment];
    if (!isRecord(child)) return output;
    current = child;
  }
  delete current[path[path.length - 1]];
  return output;
}

export function validateMcpArguments(
  schema: Record<string, unknown>,
  value: unknown,
): string[] {
  const issues: string[] = [];
  try {
    validateSupportedSchema(schema, 0, true);
  } catch {
    return ["지원하지 않는 schema는 호출할 수 없습니다."];
  }
  validateValue(schema, value, "arguments", issues, 0);
  return issues.slice(0, 100);
}

export function parseMcpJsonObject(value: string): Record<string, unknown> {
  if (utf8Bytes(value) > 1024 * 1024) throw new Error("mcp_request_too_large");
  let parsed: unknown;
  try {
    parsed = JSON.parse(value);
  } catch {
    throw new Error("mcp_message_invalid");
  }
  if (!isRecord(parsed)) throw new Error("mcp_message_invalid");
  return parsed;
}

function validateSupportedSchema(
  schema: Record<string, unknown>,
  depth: number,
  root = false,
  headerReachable = false,
  headerNames = new Set<string>(),
): void {
  if (depth > MAX_SCHEMA_DEPTH) throw new Error("unsupported");
  const type = schema.type;
  if (typeof type !== "string" || !TYPE_SCHEMA_KEYS[type]) throw new Error("unsupported");
  if (root && type !== "object") throw new Error("unsupported");
  if (schema.$schema !== undefined && (!root || typeof schema.$schema !== "string")) {
    throw new Error("unsupported");
  }
  validateDisplay(schema.$schema);
  for (const key of Object.keys(schema)) {
    if (!COMMON_SCHEMA_KEYS.has(key) && !TYPE_SCHEMA_KEYS[type].has(key)) {
      throw new Error("unsupported");
    }
  }
  validateDisplay(schema.title);
  validateDisplay(schema.description);
  validateEnum(schema.enum, type);
  if (schema["x-mcp-header"] !== undefined) {
    const header = schema["x-mcp-header"];
    const normalized = typeof header === "string" ? header.toLowerCase() : "";
    if (!headerReachable
      || !["string", "integer", "boolean"].includes(type)
      || typeof header !== "string"
      || !isHeaderToken(header)
      || headerNames.has(normalized)
      || headerNames.size >= MAX_DERIVED_PARAMETER_HEADERS) {
      throw new Error("unsupported");
    }
    headerNames.add(normalized);
  }
  if (type === "object") {
    if (schema.additionalProperties !== undefined && schema.additionalProperties !== false) {
      throw new Error("unsupported");
    }
    const properties = schema.properties ?? {};
    if (!isRecord(properties) || Object.keys(properties).length > 2_000) throw new Error("unsupported");
    const required = schema.required ?? [];
    if (!Array.isArray(required) || required.some((item) => typeof item !== "string")) {
      throw new Error("unsupported");
    }
    const names = new Set(Object.keys(properties));
    if (new Set(required).size !== required.length || required.some((name) => !names.has(name))) {
      throw new Error("unsupported");
    }
    for (const [name, child] of Object.entries(properties)) {
      if (!name || utf8Bytes(name) > 4 * 1024 || hasControl(name) || !isRecord(child)) {
        throw new Error("unsupported");
      }
      validateSupportedSchema(
        child,
        depth + 1,
        false,
        root || headerReachable,
        headerNames,
      );
    }
  } else if (type === "array") {
    if (!isRecord(schema.items)) throw new Error("unsupported");
    validateNonNegativeInteger(schema.minItems);
    validateNonNegativeInteger(schema.maxItems);
    if ((schema.minItems as number | undefined) !== undefined
      && (schema.minItems as number) > MAX_ARRAY_ITEMS) throw new Error("unsupported");
    if ((schema.maxItems as number | undefined) !== undefined
      && (schema.maxItems as number) > MAX_ARRAY_ITEMS) throw new Error("unsupported");
    validateOrderedRange(schema.minItems, schema.maxItems);
    validateSupportedSchema(schema.items, depth + 1, false, false, headerNames);
  } else if (type === "string") {
    validateNonNegativeInteger(schema.minLength);
    validateNonNegativeInteger(schema.maxLength);
    if ((schema.minLength as number | undefined) !== undefined
      && (schema.minLength as number) > MAX_STRING_LENGTH) throw new Error("unsupported");
    if ((schema.maxLength as number | undefined) !== undefined
      && (schema.maxLength as number) > MAX_STRING_LENGTH) throw new Error("unsupported");
    validateOrderedRange(schema.minLength, schema.maxLength);
  } else if (type === "integer" || type === "number") {
    validateFiniteNumber(schema.minimum);
    validateFiniteNumber(schema.maximum);
    validateOrderedRange(schema.minimum, schema.maximum);
  }
  if (schema.default !== undefined) {
    const issues: string[] = [];
    validateValue(schema, schema.default, "default", issues, depth + 1);
    if (issues.length > 0) throw new Error("unsupported");
  }
}

function validateValue(
  schema: Record<string, unknown>,
  value: unknown,
  path: string,
  issues: string[],
  depth: number,
): void {
  if (issues.length >= 100 || depth > MAX_SCHEMA_DEPTH) return;
  const type = schema.type;
  if (schema.enum !== undefined && !(schema.enum as unknown[]).some((item) => Object.is(item, value))) {
    issues.push(`${path}: 허용된 enum 값이 아닙니다.`);
    return;
  }
  if (type === "object") {
    if (!isRecord(value)) {
      issues.push(`${path}: object가 필요합니다.`);
      return;
    }
    const properties = (schema.properties ?? {}) as Record<string, Record<string, unknown>>;
    const required = new Set(schema.required as string[] | undefined);
    for (const name of required) {
      if (!(name in value)) issues.push(`${path}.${name}: 필수 값입니다.`);
    }
    for (const name of Object.keys(value)) {
      const child = properties[name];
      if (!child) {
        issues.push(`${path}.${name}: schema에 없는 값입니다.`);
      } else {
        validateValue(child, value[name], `${path}.${name}`, issues, depth + 1);
      }
    }
  } else if (type === "string") {
    if (typeof value !== "string") issues.push(`${path}: 문자열이 필요합니다.`);
    else {
      if (typeof schema.minLength === "number" && [...value].length < schema.minLength) {
        issues.push(`${path}: 너무 짧습니다.`);
      }
      if (typeof schema.maxLength === "number" && [...value].length > schema.maxLength) {
        issues.push(`${path}: 너무 깁니다.`);
      }
    }
  } else if (type === "integer") {
    if (!Number.isSafeInteger(value)) issues.push(`${path}: 정수가 필요합니다.`);
    else validateNumberRange(schema, value as number, path, issues);
  } else if (type === "number") {
    if (typeof value !== "number" || !Number.isFinite(value)) issues.push(`${path}: 숫자가 필요합니다.`);
    else validateNumberRange(schema, value, path, issues);
  } else if (type === "boolean") {
    if (typeof value !== "boolean") issues.push(`${path}: boolean이 필요합니다.`);
  } else if (type === "array") {
    if (!Array.isArray(value)) {
      issues.push(`${path}: 배열이 필요합니다.`);
      return;
    }
    if (value.length > MAX_ARRAY_ITEMS) issues.push(`${path}: 배열 항목이 너무 많습니다.`);
    if (typeof schema.minItems === "number" && value.length < schema.minItems) {
      issues.push(`${path}: 배열 항목이 부족합니다.`);
    }
    if (typeof schema.maxItems === "number" && value.length > schema.maxItems) {
      issues.push(`${path}: 배열 항목이 너무 많습니다.`);
    }
    const itemSchema = schema.items as Record<string, unknown>;
    value.slice(0, MAX_ARRAY_ITEMS).forEach((item, index) => {
      validateValue(itemSchema, item, `${path}[${index}]`, issues, depth + 1);
    });
  }
}

function initialValue(schema: Record<string, unknown>, root = false): unknown {
  if (schema.default !== undefined) return structuredClone(schema.default);
  if (Array.isArray(schema.enum) && schema.enum.length > 0) return schema.enum[0];
  switch (schema.type) {
    case "object": {
      const output: Record<string, unknown> = {};
      const required = new Set(schema.required as string[] | undefined);
      const properties = (schema.properties ?? {}) as Record<string, Record<string, unknown>>;
      for (const [name, child] of Object.entries(properties)) {
        if (required.has(name) || child.default !== undefined) output[name] = initialValue(child);
      }
      return output;
    }
    case "string": return "";
    case "integer":
    case "number": return 0;
    case "boolean": return false;
    case "array": return [];
    default: return root ? {} : null;
  }
}

function validateEnum(value: unknown, type: string): void {
  if (value === undefined) return;
  if (!Array.isArray(value) || value.length === 0 || value.length > 1_000) {
    throw new Error("unsupported");
  }
  const seen = new Set<string>();
  for (const item of value) {
    const valid = type === "string" ? typeof item === "string"
      : type === "integer" ? Number.isSafeInteger(item)
        : type === "number" ? typeof item === "number" && Number.isFinite(item)
          : type === "boolean" ? typeof item === "boolean"
            : false;
    const identity = JSON.stringify(item);
    if (!valid || seen.has(identity)) throw new Error("unsupported");
    seen.add(identity);
  }
}

function validateDisplay(value: unknown): void {
  if (value !== undefined && (
    typeof value !== "string"
    || utf8Bytes(value) > 4 * 1024
    || value.includes("\0")
  )) throw new Error("unsupported");
}

function isHeaderToken(value: string): boolean {
  return value.length > 0
    && value.length <= 128
    && /^[!#$%&'*+\-.^_`|~0-9A-Za-z]+$/u.test(value);
}

function validatePromptArguments(value: unknown): void {
  if (value === undefined) return;
  if (!Array.isArray(value) || value.length > 2_000) {
    throw new Error("mcp_message_invalid");
  }
  const seen = new Set<string>();
  for (const candidate of value) {
    if (!isRecord(candidate)
      || typeof candidate.name !== "string"
      || candidate.name.length === 0
      || utf8Bytes(candidate.name) > MAX_NAME_BYTES
      || hasControl(candidate.name)
      || seen.has(candidate.name)
      || (candidate.required !== undefined && typeof candidate.required !== "boolean")) {
      throw new Error("mcp_message_invalid");
    }
    seen.add(candidate.name);
  }
}

function validateNonNegativeInteger(value: unknown): void {
  if (value !== undefined && (!Number.isSafeInteger(value) || (value as number) < 0)) {
    throw new Error("unsupported");
  }
}

function validateFiniteNumber(value: unknown): void {
  if (value !== undefined && (typeof value !== "number" || !Number.isFinite(value))) {
    throw new Error("unsupported");
  }
}

function validateOrderedRange(minimum: unknown, maximum: unknown): void {
  if (typeof minimum === "number" && typeof maximum === "number" && minimum > maximum) {
    throw new Error("unsupported");
  }
}

function validateNumberRange(
  schema: Record<string, unknown>,
  value: number,
  path: string,
  issues: string[],
): void {
  if (typeof schema.minimum === "number" && value < schema.minimum) issues.push(`${path}: 최솟값보다 작습니다.`);
  if (typeof schema.maximum === "number" && value > schema.maximum) issues.push(`${path}: 최댓값보다 큽니다.`);
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

function hasControl(value: string): boolean {
  return /[\u0000-\u001f\u007f]/u.test(value);
}

function utf8Bytes(value: string): number {
  return new TextEncoder().encode(value).byteLength;
}

function jsonBytes(value: unknown): number {
  try {
    return utf8Bytes(JSON.stringify(value));
  } catch {
    throw new Error("mcp_message_invalid");
  }
}
