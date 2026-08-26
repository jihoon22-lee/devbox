export interface GraphqlRequest {
  query: string;
  variables: string;
  operation_name: string;
}

export interface GraphqlLocation {
  line: number;
  column: number;
}

export interface GraphqlError {
  message: string;
  locations: GraphqlLocation[];
  path: string[];
}

export interface GraphqlResponse {
  envelope: "valid" | "not_json" | "invalid" | "oversized";
  data: unknown | null;
  errors: GraphqlError[];
  errors_truncated: boolean;
}

export const MAX_GRAPHQL_QUERY_BYTES = 512 * 1024;
export const MAX_GRAPHQL_VARIABLES_BYTES = 512 * 1024;
export const MAX_GRAPHQL_OPERATION_NAME_BYTES = 128;
export const MAX_GRAPHQL_OPERATIONS = 100;
export const MAX_GRAPHQL_TOKENS = 100_000;
export const MAX_GRAPHQL_VARIABLE_DEPTH = 32;
export const MAX_GRAPHQL_VARIABLE_NODES = 10_000;
export const MAX_GRAPHQL_VARIABLE_STRING_BYTES = 64 * 1024;
export const MAX_GRAPHQL_BODY_BYTES = 2 * 1024 * 1024;
export const MAX_GRAPHQL_RESPONSE_NODES = 10_000;
export const MAX_GRAPHQL_RESPONSE_DEPTH = 64;
export const MAX_GRAPHQL_RESPONSE_STRING_BYTES = 64 * 1024;
export const MAX_GRAPHQL_RESPONSE_ERRORS = 100;
export const MAX_GRAPHQL_ERROR_MESSAGE_BYTES = 4 * 1024;
export const MAX_GRAPHQL_ERROR_PATH_ITEMS = 20;
export const MAX_GRAPHQL_ERROR_PATH_ITEM_BYTES = 128;
export const MAX_GRAPHQL_URL_BYTES = 8 * 1024;
export const MIN_GRAPHQL_TIMEOUT_MS = 100;
export const MAX_GRAPHQL_TIMEOUT_MS = 120_000;
export const MAX_GRAPHQL_REQUEST_HEADERS = 100;
export const MAX_GRAPHQL_REQUEST_HEADER_BYTES = 128 * 1024;

export const GRAPHQL_INVALID_REQUEST = "GraphQL 요청 구성이 올바르지 않습니다";
export const GRAPHQL_INVALID_DOCUMENT = "GraphQL 문서 형식이 올바르지 않습니다";
export const GRAPHQL_QUERY_TOO_LARGE = "GraphQL query가 허용된 크기를 초과했습니다";
export const GRAPHQL_VARIABLES_TOO_LARGE = "GraphQL variables가 허용된 크기를 초과했습니다";
export const GRAPHQL_OPERATION_INVALID = "GraphQL operation 선택이 올바르지 않습니다";
export const GRAPHQL_VARIABLES_INVALID = "GraphQL variables는 유효한 JSON object여야 합니다";
export const GRAPHQL_VARIABLES_TOO_COMPLEX = "GraphQL variables 구조가 허용된 한계를 초과했습니다";
export const GRAPHQL_BODY_TOO_LARGE = "GraphQL 요청 본문이 허용된 크기를 초과했습니다";
export const GRAPHQL_UNSUPPORTED_INTROSPECTION = "GraphQL introspection 요청은 지원하지 않습니다";
export const GRAPHQL_UNSUPPORTED_SUBSCRIPTION = "GraphQL subscription은 지원하지 않습니다";
export const GRAPHQL_ENDPOINT_ERROR = "GraphQL endpoint URL이 올바르지 않습니다";
export const GRAPHQL_CREDENTIAL_QUERY_ERROR = "GraphQL endpoint query에 credential을 넣을 수 없습니다";
export const GRAPHQL_HEADER_ROWS_ERROR = "GraphQL header 행 수가 허용된 한계를 초과했습니다";
export const GRAPHQL_HEADER_BYTES_ERROR = "GraphQL header 크기가 허용된 한계를 초과했습니다";
export const GRAPHQL_URL_TOO_LARGE = "GraphQL URL이 허용된 크기를 초과했습니다";

type Token =
  | { kind: "name"; value: string }
  | { kind: "string" | "number" }
  | { kind: "punct"; value: string }
  | { kind: "spread" };

const encoder = new TextEncoder();

function bytes(value: string): number {
  return encoder.encode(value).byteLength;
}

function isName(value: string): boolean {
  return /^[A-Za-z_][A-Za-z0-9_]*$/.test(value);
}

function lex(query: string): Token[] {
  if (bytes(query) > MAX_GRAPHQL_QUERY_BYTES) throw new Error(GRAPHQL_QUERY_TOO_LARGE);
  const tokens: Token[] = [];
  let index = 0;
  while (index < query.length) {
    const current = query[index];
    if (current === " " || current === "\t" || current === "\r" || current === "\n" || current === ",") {
      index += 1;
      continue;
    }
    if (current === "#") {
      const newline = query.indexOf("\n", index);
      index = newline < 0 ? query.length : newline + 1;
      continue;
    }
    if (/[A-Za-z_]/.test(current)) {
      const start = index++;
      while (index < query.length && /[A-Za-z0-9_]/.test(query[index])) index += 1;
      const value = query.slice(start, index);
      if (value === "__schema" || value === "__type") throw new Error(GRAPHQL_UNSUPPORTED_INTROSPECTION);
      tokens.push({ kind: "name", value });
    } else if (current === '"') {
      const { next } = scanString(query, index);
      tokens.push({ kind: "string" });
      index = next;
    } else if (query.slice(index, index + 3) === "...") {
      tokens.push({ kind: "spread" });
      index += 3;
    } else if ("!$&():=@[]{}|".includes(current)) {
      tokens.push({ kind: "punct", value: current });
      index += 1;
    } else if (current === "-" || /[0-9]/.test(current)) {
      index += 1;
      while (
        index < query.length &&
        !/[\s,()[\]{}]/.test(query[index])
      ) index += 1;
      tokens.push({ kind: "number" });
    } else {
      throw new Error(GRAPHQL_INVALID_DOCUMENT);
    }
    if (tokens.length > MAX_GRAPHQL_TOKENS) throw new Error(GRAPHQL_QUERY_TOO_LARGE);
  }
  return tokens;
}

function scanString(query: string, start: number): { next: number; block: boolean } {
  const block = query.slice(start, start + 3) === '"""';
  let index = start + (block ? 3 : 1);
  while (index < query.length) {
    if (block && query.slice(index, index + 3) === '"""') return { next: index + 3, block };
    if (!block && query[index] === '"') return { next: index + 1, block };
    if (query[index] === "\\") index += 2;
    else {
      if (!block && query.charCodeAt(index) < 0x20) throw new Error(GRAPHQL_INVALID_DOCUMENT);
      index += 1;
    }
  }
  throw new Error(GRAPHQL_INVALID_DOCUMENT);
}

function skipGroup(tokens: Token[], index: { value: number }): void {
  const first = tokens[index.value];
  if (first?.kind !== "punct" || !"{[(".includes(first.value)) throw new Error(GRAPHQL_INVALID_DOCUMENT);
  const closes: Record<string, string> = { "{": "}", "[": "]", "(": ")" };
  const stack = [closes[first.value]];
  index.value += 1;
  while (index.value < tokens.length) {
    const token = tokens[index.value];
    if (token.kind === "punct" && token.value in closes) stack.push(closes[token.value]);
    else if (token.kind === "punct" && token.value === stack[stack.length - 1]) {
      stack.pop();
      if (stack.length === 0) {
        index.value += 1;
        return;
      }
    } else if (token.kind === "punct" && ")]}".includes(token.value)) {
      throw new Error(GRAPHQL_INVALID_DOCUMENT);
    }
    index.value += 1;
  }
  throw new Error(GRAPHQL_INVALID_DOCUMENT);
}

function skipToSelection(tokens: Token[], index: { value: number }): void {
  let paren = 0;
  let bracket = 0;
  while (index.value < tokens.length) {
    const token = tokens[index.value];
    if (token.kind === "punct" && token.value === "(") paren += 1;
    else if (token.kind === "punct" && token.value === ")" && paren > 0) paren -= 1;
    else if (token.kind === "punct" && token.value === "[") bracket += 1;
    else if (token.kind === "punct" && token.value === "]" && bracket > 0) bracket -= 1;
    else if (token.kind === "punct" && ")]}".includes(token.value)) throw new Error(GRAPHQL_INVALID_DOCUMENT);
    else if (token.kind === "punct" && token.value === "{" && paren === 0 && bracket === 0) {
      skipGroup(tokens, index);
      return;
    }
    index.value += 1;
  }
  throw new Error(GRAPHQL_INVALID_DOCUMENT);
}

function parseDocument(query: string): { operations: number; names: string[] } {
  const tokens = lex(query);
  const index = { value: 0 };
  const names: string[] = [];
  const nameSet = new Set<string>();
  let operations = 0;
  while (index.value < tokens.length) {
    const token = tokens[index.value];
    if (token.kind === "name" && ["query", "mutation", "subscription"].includes(token.value)) {
      if (token.value === "subscription") throw new Error(GRAPHQL_UNSUPPORTED_SUBSCRIPTION);
      operations += 1;
      if (operations > MAX_GRAPHQL_OPERATIONS) throw new Error(GRAPHQL_OPERATION_INVALID);
      index.value += 1;
      const possibleName = tokens[index.value];
      if (possibleName?.kind === "name") {
        index.value += 1;
        if (bytes(possibleName.value) > MAX_GRAPHQL_OPERATION_NAME_BYTES || nameSet.has(possibleName.value)) {
          throw new Error(GRAPHQL_OPERATION_INVALID);
        }
        nameSet.add(possibleName.value);
        names.push(possibleName.value);
      }
      skipToSelection(tokens, index);
    } else if (token.kind === "name" && token.value === "fragment") {
      index.value += 1;
      if (tokens[index.value]?.kind !== "name") throw new Error(GRAPHQL_INVALID_DOCUMENT);
      index.value += 1;
      const on = tokens[index.value];
      if (on?.kind !== "name" || on.value !== "on") throw new Error(GRAPHQL_INVALID_DOCUMENT);
      index.value += 1;
      if (tokens[index.value]?.kind !== "name") throw new Error(GRAPHQL_INVALID_DOCUMENT);
      index.value += 1;
      skipToSelection(tokens, index);
    } else if (token.kind === "punct" && token.value === "{") {
      operations += 1;
      if (operations > MAX_GRAPHQL_OPERATIONS) throw new Error(GRAPHQL_OPERATION_INVALID);
      skipGroup(tokens, index);
    } else {
      throw new Error(GRAPHQL_INVALID_DOCUMENT);
    }
  }
  if (!operations) throw new Error(GRAPHQL_INVALID_DOCUMENT);
  return { operations, names };
}

export function validateGraphqlDocument(query: string, operationName: string): void {
  const info = parseDocument(query);
  const selected = operationName.trim();
  if (bytes(selected) > MAX_GRAPHQL_OPERATION_NAME_BYTES || (selected && !isName(selected))) {
    throw new Error(GRAPHQL_OPERATION_INVALID);
  }
  // A named operation may be selected from a multi-operation document, but
  // the GraphQL grammar permits an anonymous operation only when it is alone.
  if (info.operations > 1 && info.names.length !== info.operations) {
    throw new Error(GRAPHQL_OPERATION_INVALID);
  }
  if (info.operations > 1 && !selected) throw new Error(GRAPHQL_OPERATION_INVALID);
  if (selected && !info.names.includes(selected)) {
    throw new Error(GRAPHQL_OPERATION_INVALID);
  }
}

function validateJson(value: unknown, depth: number, state: { nodes: number }): void {
  state.nodes += 1;
  if (state.nodes > MAX_GRAPHQL_VARIABLE_NODES || depth > MAX_GRAPHQL_VARIABLE_DEPTH) {
    throw new Error(GRAPHQL_VARIABLES_TOO_COMPLEX);
  }
  if (typeof value === "string" && bytes(value) > MAX_GRAPHQL_VARIABLE_STRING_BYTES) {
    throw new Error(GRAPHQL_VARIABLES_TOO_COMPLEX);
  }
  if (Array.isArray(value)) value.forEach((item) => validateJson(item, depth + 1, state));
  else if (value && typeof value === "object") {
    Object.entries(value as Record<string, unknown>).forEach(([key, item]) => {
      if (bytes(key) > MAX_GRAPHQL_VARIABLE_STRING_BYTES) throw new Error(GRAPHQL_VARIABLES_TOO_COMPLEX);
      validateJson(item, depth + 1, state);
    });
  }
}

export function parseGraphqlVariables(raw: string): Record<string, unknown> {
  if (bytes(raw) > MAX_GRAPHQL_VARIABLES_BYTES) throw new Error(GRAPHQL_VARIABLES_TOO_LARGE);
  if (!raw.trim()) return {};
  // Avoid handing pathological nesting to JSON.parse before the bounded
  // visitor has a chance to reject it. The root object is one structural
  // level above the value depth used by validateJson.
  if (!jsonDepthWithin(raw, MAX_GRAPHQL_VARIABLE_DEPTH + 1)) {
    throw new Error(GRAPHQL_VARIABLES_TOO_COMPLEX);
  }
  let value: unknown;
  try {
    value = JSON.parse(raw);
  } catch {
    throw new Error(GRAPHQL_VARIABLES_INVALID);
  }
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error(GRAPHQL_VARIABLES_INVALID);
  validateJson(value, 0, { nodes: 0 });
  return value as Record<string, unknown>;
}

export function buildGraphqlBody(request: GraphqlRequest): string {
  validateGraphqlDocument(request.query, request.operation_name);
  const variables = parseGraphqlVariables(request.variables);
  if (!request.query.trim()) throw new Error(GRAPHQL_INVALID_DOCUMENT);
  const body = request.operation_name.trim()
    ? { operationName: request.operation_name.trim(), query: request.query.trim(), variables }
    : { query: request.query.trim(), variables };
  const serialized = stableJsonStringify(body);
  if (bytes(serialized) > MAX_GRAPHQL_BODY_BYTES) throw new Error(GRAPHQL_BODY_TOO_LARGE);
  return serialized;
}

export function validateGraphqlEndpoint(urlValue: string): void {
  if (bytes(urlValue) > MAX_GRAPHQL_URL_BYTES || /[\u0000-\u001f\u007f]/.test(urlValue)) {
    throw new Error(GRAPHQL_ENDPOINT_ERROR);
  }
  let url: URL;
  try {
    url = new URL(urlValue);
  } catch {
    throw new Error(GRAPHQL_ENDPOINT_ERROR);
  }
  if (!/^https?:$/.test(url.protocol) || !url.hostname || url.username || url.password || url.hash) {
    throw new Error(GRAPHQL_ENDPOINT_ERROR);
  }
  for (const key of url.searchParams.keys()) {
    if (isGraphqlCredentialName(key)) {
      throw new Error(GRAPHQL_CREDENTIAL_QUERY_ERROR);
    }
  }
}

export function isGraphqlCredentialName(name: string): boolean {
  return /(authorization|proxy-authorization|cookie|set-cookie|api[-_]?key|api[-_]?value|access[-_]?token|refresh[-_]?token|token|secret|password|passwd|private[-_]?key|username)/i.test(name);
}

export function validateGraphqlParams(
  params: readonly { key: string; value: string }[],
): void {
  for (const param of params) {
    if (param.key && isGraphqlCredentialName(param.key)) {
      throw new Error(GRAPHQL_CREDENTIAL_QUERY_ERROR);
    }
  }
}

export function validateGraphqlHeaders(
  headers: readonly { key: string; value: string }[],
): void {
  if (headers.length > MAX_GRAPHQL_REQUEST_HEADERS) throw new Error(GRAPHQL_HEADER_ROWS_ERROR);
  const total = headers.reduce(
    (sum, header) => sum + bytes(header.key) + bytes(header.value) + 4,
    0,
  );
  if (total > MAX_GRAPHQL_REQUEST_HEADER_BYTES) throw new Error(GRAPHQL_HEADER_BYTES_ERROR);
}

export function isGraphqlDerivedHeader(name: string): boolean {
  const normalized = name.trim().toLowerCase().replace(/_/g, "-");
  return [
    "content-type",
    "content-length",
    "transfer-encoding",
    "trailer",
    "expect",
    "digest",
    "repr-digest",
  ].includes(normalized);
}

export function buildGraphqlGetUrl(base: string, params: readonly { key: string; value: string }[], request: GraphqlRequest): string {
  validateGraphqlEndpoint(base);
  const url = new URL(base);
  validateGraphqlParams(params);
  for (const param of params) {
    if (!param.key) continue;
    url.searchParams.append(param.key, param.value);
  }
  const variables = stableJsonStringify(parseGraphqlVariables(request.variables));
  url.searchParams.append("query", request.query.trim());
  url.searchParams.append("variables", variables);
  if (request.operation_name.trim()) url.searchParams.append("operationName", request.operation_name.trim());
  if (bytes(url.toString()) > MAX_GRAPHQL_URL_BYTES) throw new Error(GRAPHQL_URL_TOO_LARGE);
  return url.toString();
}

export function projectGraphqlResponse(body: string): GraphqlResponse {
  // The response envelope and `data` wrapper add two levels to the projected
  // data depth. This preflight keeps deeply nested JSON out of JSON.parse.
  if (!jsonDepthWithin(body, MAX_GRAPHQL_RESPONSE_DEPTH + 2)) {
    return { envelope: "oversized", data: null, errors: [], errors_truncated: false };
  }
  let value: unknown;
  try {
    value = JSON.parse(body);
  } catch {
    return { envelope: "not_json", data: null, errors: [], errors_truncated: false };
  }
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return { envelope: "invalid", data: null, errors: [], errors_truncated: false };
  }
  const object = value as Record<string, unknown>;
  if (!("data" in object) && !("errors" in object)) {
    return { envelope: "invalid", data: null, errors: [], errors_truncated: false };
  }
  if ("data" in object) {
    try {
      validateResponseJson(object.data, 0, { nodes: 0 });
    } catch {
      return { envelope: "oversized", data: null, errors: [], errors_truncated: false };
    }
  }
  if (!("errors" in object)) return { envelope: "valid", data: object.data ?? null, errors: [], errors_truncated: false };
  if (!Array.isArray(object.errors)) return { envelope: "invalid", data: null, errors: [], errors_truncated: false };
  const errors = object.errors;
  const projected: GraphqlError[] = [];
  for (const item of errors.slice(0, MAX_GRAPHQL_RESPONSE_ERRORS)) {
    if (!item || typeof item !== "object" || Array.isArray(item)) return { envelope: "invalid", data: null, errors: [], errors_truncated: false };
    const record = item as Record<string, unknown>;
    if (typeof record.message !== "string") {
      return { envelope: "invalid", data: null, errors: [], errors_truncated: false };
    }
    if (bytes(record.message) > MAX_GRAPHQL_ERROR_MESSAGE_BYTES) {
      return { envelope: "oversized", data: null, errors: [], errors_truncated: false };
    }
    const locations = Array.isArray(record.locations)
      ? record.locations.slice(0, 20).flatMap((location) => {
        if (!location || typeof location !== "object" || Array.isArray(location)) return [];
        const candidate = location as Record<string, unknown>;
        return typeof candidate.line === "number" && Number.isSafeInteger(candidate.line) && candidate.line >= 0
          && typeof candidate.column === "number" && Number.isSafeInteger(candidate.column) && candidate.column >= 0
          ? [{ line: candidate.line, column: candidate.column }]
          : [];
      })
      : [];
    const path = Array.isArray(record.path)
      ? record.path.slice(0, MAX_GRAPHQL_ERROR_PATH_ITEMS).flatMap((part) => {
        if (typeof part === "number" && Number.isSafeInteger(part) && part >= 0) return [String(part)];
        return typeof part === "string" && bytes(part) <= MAX_GRAPHQL_ERROR_PATH_ITEM_BYTES ? [part] : [];
      })
      : [];
    projected.push({ message: record.message, locations, path });
  }
  return {
    envelope: "valid",
    data: object.data ?? null,
    errors: projected,
    errors_truncated: errors.length > MAX_GRAPHQL_RESPONSE_ERRORS,
  };
}

function validateResponseJson(value: unknown, depth: number, state: { nodes: number }): void {
  state.nodes += 1;
  if (state.nodes > MAX_GRAPHQL_RESPONSE_NODES || depth > MAX_GRAPHQL_RESPONSE_DEPTH) throw new Error(GRAPHQL_VARIABLES_TOO_COMPLEX);
  if (typeof value === "string" && bytes(value) > MAX_GRAPHQL_RESPONSE_STRING_BYTES) throw new Error(GRAPHQL_VARIABLES_TOO_COMPLEX);
  if (Array.isArray(value)) value.forEach((item) => validateResponseJson(item, depth + 1, state));
  else if (value && typeof value === "object") {
    Object.entries(value as Record<string, unknown>).forEach(([key, item]) => {
      if (bytes(key) > MAX_GRAPHQL_RESPONSE_STRING_BYTES) throw new Error(GRAPHQL_VARIABLES_TOO_COMPLEX);
      validateResponseJson(item, depth + 1, state);
    });
  }
}

function jsonDepthWithin(raw: string, maxDepth: number): boolean {
  let depth = 0;
  let inString = false;
  let escaped = false;
  for (const character of raw) {
    if (inString) {
      if (escaped) escaped = false;
      else if (character === "\\") escaped = true;
      else if (character === '"') inString = false;
      continue;
    }
    if (character === '"') inString = true;
    else if (character === "{" || character === "[") {
      depth += 1;
      if (depth > maxDepth) return false;
    } else if (character === "}" || character === "]") {
      depth = Math.max(0, depth - 1);
    }
  }
  return true;
}

function stableJsonStringify(value: unknown): string {
  if (Array.isArray(value)) return `[${value.map(stableJsonStringify).join(",")}]`;
  if (value && typeof value === "object") {
    const object = value as Record<string, unknown>;
    return `{${Object.keys(object).sort().map((key) => `${JSON.stringify(key)}:${stableJsonStringify(object[key])}`).join(",")}}`;
  }
  const serialized = JSON.stringify(value);
  if (serialized === undefined) throw new Error(GRAPHQL_INVALID_REQUEST);
  return serialized;
}

function isExactVariableReference(value: string): boolean {
  return /^(?:\{\{\s*[A-Za-z0-9_.-]+\s*\}\}|\$\{\s*[A-Za-z0-9_.-]+\s*\})$/.test(value);
}

/** Replace GraphQL string literals before persistence/display redaction. */
export function maskGraphqlQueryLiterals(query: string): string {
  let output = "";
  let index = 0;
  while (index < query.length) {
    if (query[index] !== '"') {
      output += query[index];
      index += 1;
      continue;
    }
    const block = query.slice(index, index + 3) === '"""';
    const opening = block ? 3 : 1;
    const closing = block ? '"""' : '"';
    const contentStart = index + opening;
    let cursor = contentStart;
    let closed = false;
    while (cursor < query.length) {
      if (query.slice(cursor, cursor + closing.length) === closing) {
        closed = true;
        break;
      }
      if (!block && query[cursor] === "\\") cursor += 2;
      else cursor += 1;
    }
    const inner = query.slice(contentStart, cursor);
    if (closed && isExactVariableReference(inner)) {
      output += query.slice(index, cursor + closing.length);
    } else {
      output += block ? '"""[REDACTED]"""' : '"[REDACTED]"';
      if (!closed) return output;
    }
    index = closed ? cursor + closing.length : query.length;
  }
  return output;
}

interface GraphqlStringLiteral {
  start: number;
  value: string;
}

/** Extract decoded-ish string literal values for response redaction. */
export function extractGraphqlStringLiterals(query: string): string[] {
  return scanGraphqlStringLiterals(query).map((literal) => literal.value);
}

/** Extract only literals assigned to credential-shaped GraphQL arguments. */
export function extractGraphqlCredentialLiterals(query: string): string[] {
  const values: string[] = [];
  let index = 0;
  while (index < query.length) {
    const current = query[index];
    if (current === " " || current === "\t" || current === "\r" || current === "\n" || current === ",") {
      index += 1;
      continue;
    }
    if (current === "#") {
      const newline = query.indexOf("\n", index);
      index = newline < 0 ? query.length : newline + 1;
      continue;
    }
    if (/[A-Za-z_]/.test(current)) {
      const start = index++;
      while (index < query.length && /[A-Za-z0-9_]/.test(query[index])) index += 1;
      const name = query.slice(start, index);
      const afterName = skipGraphqlIgnored(query, index);
      if (query[afterName] !== ":") continue;
      const valueStart = skipGraphqlIgnored(query, afterName + 1);
      if (query[valueStart] !== '"') continue;
      const literal = scanGraphqlLiteral(query, valueStart);
      if (isGraphqlCredentialName(name) && literal.value) values.push(literal.value);
      index = literal.next;
      continue;
    }
    if (current === '"') {
      index = scanGraphqlLiteral(query, index).next;
      continue;
    }
    index += 1;
  }
  return values;
}

function scanGraphqlStringLiterals(query: string): GraphqlStringLiteral[] {
  const values: GraphqlStringLiteral[] = [];
  let index = 0;
  while (index < query.length) {
    if (query[index] !== '"') {
      index += 1;
      continue;
    }
    const literal = scanGraphqlLiteral(query, index);
    if (literal.value) values.push({ start: index, value: literal.value });
    if (!literal.closed) break;
    index = literal.next;
  }
  return values;
}

function skipGraphqlIgnored(query: string, start: number): number {
  let index = start;
  while (index < query.length) {
    if (query[index] === " " || query[index] === "\t" || query[index] === "\r" || query[index] === "\n" || query[index] === ",") {
      index += 1;
    } else if (query[index] === "#") {
      const newline = query.indexOf("\n", index);
      index = newline < 0 ? query.length : newline + 1;
    } else {
      break;
    }
  }
  return index;
}

function scanGraphqlLiteral(query: string, start: number): {
  next: number;
  closed: boolean;
  value: string;
} {
  const block = query.slice(start, start + 3) === '"""';
  const opening = block ? 3 : 1;
  const closing = block ? '"""' : '"';
  const contentStart = start + opening;
  let cursor = contentStart;
  while (cursor < query.length) {
    if (query.slice(cursor, cursor + closing.length) === closing) {
      const raw = query.slice(contentStart, cursor);
      if (!raw) return { next: cursor + closing.length, closed: true, value: "" };
      if (block) return { next: cursor + closing.length, closed: true, value: raw };
      try {
        return {
          next: cursor + closing.length,
          closed: true,
          value: JSON.parse(query.slice(start, cursor + closing.length)) as string,
        };
      } catch {
        return { next: cursor + closing.length, closed: true, value: raw };
      }
    }
    if (!block && query[cursor] === "\\") cursor += 2;
    else cursor += 1;
  }
  return { next: query.length, closed: false, value: query.slice(contentStart) };
}

export function resolveGraphqlRequest(request: GraphqlRequest, variables: ReadonlyMap<string, string>): GraphqlRequest {
  const replace = (value: string) => value.replace(/\{\{\s*([A-Za-z0-9_.-]+)\s*\}\}|\$\{\s*([A-Za-z0-9_.-]+)\s*\}/g, (match, moustache: string, dollar: string) => variables.get(moustache || dollar) ?? match);
  return {
    query: replace(request.query),
    variables: replace(request.variables),
    operation_name: replace(request.operation_name),
  };
}
