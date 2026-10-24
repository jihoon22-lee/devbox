import { type GraphqlError, type GraphqlResponse, toJsonValue } from "../template";
import {
  bytes,
  GRAPHQL_VARIABLES_TOO_COMPLEX,
  MAX_GRAPHQL_ERROR_MESSAGE_BYTES,
  MAX_GRAPHQL_ERROR_PATH_ITEM_BYTES,
  MAX_GRAPHQL_ERROR_PATH_ITEMS,
  MAX_GRAPHQL_RESPONSE_DEPTH,
  MAX_GRAPHQL_RESPONSE_ERRORS,
  MAX_GRAPHQL_RESPONSE_NODES,
  MAX_GRAPHQL_RESPONSE_STRING_BYTES,
} from "./limits";
import { jsonDepthWithin } from "./variables";

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
  if (!("errors" in object))
    return { envelope: "valid", data: toJsonValue(object.data ?? null), errors: [], errors_truncated: false };
  if (!Array.isArray(object.errors)) return { envelope: "invalid", data: null, errors: [], errors_truncated: false };
  const errors = object.errors;
  const projected: GraphqlError[] = [];
  for (const item of errors.slice(0, MAX_GRAPHQL_RESPONSE_ERRORS)) {
    if (!item || typeof item !== "object" || Array.isArray(item))
      return { envelope: "invalid", data: null, errors: [], errors_truncated: false };
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
          return typeof candidate.line === "number" &&
            Number.isSafeInteger(candidate.line) &&
            candidate.line >= 0 &&
            typeof candidate.column === "number" &&
            Number.isSafeInteger(candidate.column) &&
            candidate.column >= 0
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
    data: toJsonValue(object.data ?? null),
    errors: projected,
    errors_truncated: errors.length > MAX_GRAPHQL_RESPONSE_ERRORS,
  };
}

function validateResponseJson(value: unknown, depth: number, state: { nodes: number }): void {
  state.nodes += 1;
  if (state.nodes > MAX_GRAPHQL_RESPONSE_NODES || depth > MAX_GRAPHQL_RESPONSE_DEPTH)
    throw new Error(GRAPHQL_VARIABLES_TOO_COMPLEX);
  if (typeof value === "string" && bytes(value) > MAX_GRAPHQL_RESPONSE_STRING_BYTES)
    throw new Error(GRAPHQL_VARIABLES_TOO_COMPLEX);
  if (Array.isArray(value)) value.forEach((item) => validateResponseJson(item, depth + 1, state));
  else if (value && typeof value === "object") {
    Object.entries(value as Record<string, unknown>).forEach(([key, item]) => {
      if (bytes(key) > MAX_GRAPHQL_RESPONSE_STRING_BYTES) throw new Error(GRAPHQL_VARIABLES_TOO_COMPLEX);
      validateResponseJson(item, depth + 1, state);
    });
  }
}
