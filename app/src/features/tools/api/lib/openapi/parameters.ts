import { MAX_REQUEST_COOKIE_ROWS } from "../cookies";
import { MAX_REQUEST_HEADER_ROWS } from "../headers";
import type { KeyValue, RequestHeader, RequestTemplate } from "../template";
import { OPENAPI_LIMITS } from "./limits";
import {
  compareText,
  ExampleValue,
  exampleFrom,
  hasRef,
  isRecord,
  isSensitiveName,
  isSensitiveValue,
  issue,
  OpenApiIssue,
  OpenApiIssueCode,
  OpenApiParameterLocation,
  OpenApiParameterPreview,
  own,
  safeScalar,
  validParameterName,
} from "./shared";

export function parameterValue(
  parameter: Record<string, unknown>,
  name: string,
): { value: string; source: ExampleValue["source"]; redacted: boolean; warning: OpenApiIssueCode | null } {
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

export interface ParameterResult {
  previews: OpenApiParameterPreview[];
  headers: RequestHeader[];
  cookies: RequestTemplate["cookies"];
  params: KeyValue[];
  warnings: OpenApiIssue[];
  errors: OpenApiIssue[];
  /** Raw declarations are counted as well as valid rows for the document bound. */
  declaredCount: number;
}

export function parametersFor(
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
      if (
        (location !== "path" && location !== "query" && location !== "header" && location !== "cookie") ||
        typeof name !== "string" ||
        !validParameterName(name, location)
      ) {
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
    return (
      rank(leftLocation) - rank(rightLocation) || compareText(String(own(left, "name")), String(own(right, "name")))
    );
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
    headers.length > MAX_REQUEST_HEADER_ROWS ||
    cookies.length > MAX_REQUEST_COOKIE_ROWS ||
    params.length > OPENAPI_LIMITS.maxRequestRows
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

export function applyPathParameterExamples(url: string, parameters: OpenApiParameterPreview[]): string {
  let resolved = url;
  for (const parameter of parameters) {
    if (parameter.location !== "path" || parameter.redacted || !parameter.value) continue;
    const encoded = encodeURIComponent(parameter.value);
    resolved = resolved.split(`{${parameter.name}}`).join(encoded);
  }
  return resolved;
}

export function pathTemplateParameters(path: string): string[] {
  const names: string[] = [];
  const matcher = /\{([A-Za-z0-9_.-]{1,120})\}/g;
  for (const match of path.matchAll(matcher)) names.push(match[1]!);
  return names;
}
