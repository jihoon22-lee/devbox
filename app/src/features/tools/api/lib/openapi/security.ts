import type { AuthConfig, KeyValue, RequestHeader, RequestTemplate } from "../template";
import {
  emptyAuth,
  hasRef,
  isRecord,
  issue,
  OpenApiIssue,
  OpenApiOperationPreview,
  OpenApiSecurityPreview,
  own,
  validParameterName,
} from "./shared";

export interface SecurityResult {
  auth: AuthConfig;
  metadata: OpenApiSecurityPreview | null;
  params: KeyValue[];
  headers: RequestHeader[];
  cookies: RequestTemplate["cookies"];
  warnings: OpenApiIssue[];
  errors: OpenApiIssue[];
}

export interface InternalOperationPreview extends OpenApiOperationPreview {
  declaredParameterCount: number;
}

export function securityFor(
  rawSecurity: unknown,
  schemes: Record<string, unknown>,
  path: string,
  method: string,
): SecurityResult {
  const none: SecurityResult = {
    auth: emptyAuth(),
    metadata: null,
    params: [],
    headers: [],
    cookies: [],
    warnings: [],
    errors: [],
  };
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
      const httpScheme =
        typeof own(scheme, "scheme") === "string" ? (own(scheme, "scheme") as string).toLowerCase() : null;
      if (type === "http" && httpScheme === "basic") {
        resolved.auth = { ...emptyAuth(), kind: "basic" };
        resolved.metadata = { kind: "basic", location: "header", name: "Authorization", valuesInjected: false };
      } else if (type === "http" && httpScheme === "bearer") {
        resolved.auth = { ...emptyAuth(), kind: "bearer" };
        resolved.metadata = { kind: "bearer", location: "header", name: "Authorization", valuesInjected: false };
      } else if (type === "apiKey") {
        const location = own(scheme, "in");
        const key = own(scheme, "name");
        if (
          (location !== "header" && location !== "query" && location !== "cookie") ||
          typeof key !== "string" ||
          !validParameterName(key, location)
        ) {
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
