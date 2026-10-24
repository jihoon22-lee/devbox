import {
  bytes,
  GRAPHQL_INVALID_REQUEST,
  GRAPHQL_VARIABLES_INVALID,
  GRAPHQL_VARIABLES_TOO_COMPLEX,
  GRAPHQL_VARIABLES_TOO_LARGE,
  MAX_GRAPHQL_VARIABLE_DEPTH,
  MAX_GRAPHQL_VARIABLE_NODES,
  MAX_GRAPHQL_VARIABLE_STRING_BYTES,
  MAX_GRAPHQL_VARIABLES_BYTES,
} from "./limits";

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

export function jsonDepthWithin(raw: string, maxDepth: number): boolean {
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

export function stableJsonStringify(value: unknown): string {
  if (Array.isArray(value)) return `[${value.map(stableJsonStringify).join(",")}]`;
  if (value && typeof value === "object") {
    const object = value as Record<string, unknown>;
    return `{${Object.keys(object)
      .sort()
      .map((key) => `${JSON.stringify(key)}:${stableJsonStringify(object[key])}`)
      .join(",")}}`;
  }
  const serialized = JSON.stringify(value);
  if (serialized === undefined) throw new Error(GRAPHQL_INVALID_REQUEST);
  return serialized;
}
