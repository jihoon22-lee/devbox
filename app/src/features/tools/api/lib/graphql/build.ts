import type { GraphqlRequest } from "../template";
import {
  bytes,
  GRAPHQL_BODY_TOO_LARGE,
  GRAPHQL_CREDENTIAL_QUERY_ERROR,
  GRAPHQL_ENDPOINT_ERROR,
  GRAPHQL_HEADER_BYTES_ERROR,
  GRAPHQL_HEADER_ROWS_ERROR,
  GRAPHQL_INVALID_DOCUMENT,
  GRAPHQL_OPERATION_INVALID,
  GRAPHQL_QUERY_TOO_LARGE,
  GRAPHQL_UNSUPPORTED_INTROSPECTION,
  GRAPHQL_UNSUPPORTED_SUBSCRIPTION,
  GRAPHQL_URL_TOO_LARGE,
  MAX_GRAPHQL_BODY_BYTES,
  MAX_GRAPHQL_OPERATION_NAME_BYTES,
  MAX_GRAPHQL_OPERATIONS,
  MAX_GRAPHQL_QUERY_BYTES,
  MAX_GRAPHQL_REQUEST_HEADER_BYTES,
  MAX_GRAPHQL_REQUEST_HEADERS,
  MAX_GRAPHQL_TOKENS,
  MAX_GRAPHQL_URL_BYTES,
} from "./limits";
import { parseGraphqlVariables, stableJsonStringify } from "./variables";

type Token =
  | { kind: "name"; value: string }
  | { kind: "string" | "number" }
  | { kind: "punct"; value: string }
  | { kind: "spread" };

function isName(value: string): boolean {
  return /^[A-Za-z_][A-Za-z0-9_]*$/.test(value);
}

function lex(query: string): Token[] {
  if (bytes(query) > MAX_GRAPHQL_QUERY_BYTES) throw new Error(GRAPHQL_QUERY_TOO_LARGE);
  const tokens: Token[] = [];
  let index = 0;
  while (index < query.length) {
    const current = query[index]!;
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
      while (index < query.length && /[A-Za-z0-9_]/.test(query[index]!)) index += 1;
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
      while (index < query.length && !/[\s,()[\]{}]/.test(query[index]!)) index += 1;
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
    const token = tokens[index.value]!;
    if (token.kind === "punct" && token.value in closes) stack.push(closes[token.value]!);
    else if (token.kind === "punct" && token.value === stack[stack.length - 1]!) {
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
    const token = tokens[index.value]!;
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
    const token = tokens[index.value]!;
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
  return /(authorization|proxy-authorization|cookie|set-cookie|api[-_]?key|api[-_]?value|access[-_]?token|refresh[-_]?token|token|secret|password|passwd|private[-_]?key|username)/i.test(
    name,
  );
}

export function validateGraphqlParams(params: readonly { key: string; value: string }[]): void {
  for (const param of params) {
    if (param.key && isGraphqlCredentialName(param.key)) {
      throw new Error(GRAPHQL_CREDENTIAL_QUERY_ERROR);
    }
  }
}

export function validateGraphqlHeaders(headers: readonly { key: string; value: string }[]): void {
  if (headers.length > MAX_GRAPHQL_REQUEST_HEADERS) throw new Error(GRAPHQL_HEADER_ROWS_ERROR);
  const total = headers.reduce((sum, header) => sum + bytes(header.key) + bytes(header.value) + 4, 0);
  if (total > MAX_GRAPHQL_REQUEST_HEADER_BYTES) throw new Error(GRAPHQL_HEADER_BYTES_ERROR);
}

export function isGraphqlDerivedHeader(name: string): boolean {
  const normalized = name.trim().toLowerCase().replace(/_/g, "-");
  return ["content-type", "content-length", "transfer-encoding", "trailer", "expect", "digest", "repr-digest"].includes(
    normalized,
  );
}

export function buildGraphqlGetUrl(
  base: string,
  params: readonly { key: string; value: string }[],
  request: GraphqlRequest,
): string {
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

export function resolveGraphqlRequest(request: GraphqlRequest, variables: ReadonlyMap<string, string>): GraphqlRequest {
  const replace = (value: string) =>
    value.replace(
      /\{\{\s*([A-Za-z0-9_.-]+)\s*\}\}|\$\{\s*([A-Za-z0-9_.-]+)\s*\}/g,
      (match, moustache: string, dollar: string) => variables.get(moustache || dollar) ?? match,
    );
  return {
    query: replace(request.query),
    variables: replace(request.variables),
    operation_name: replace(request.operation_name),
  };
}
