import type { RequestTemplate } from "../../types";
export interface ImportedRequest {
  name: string;
  folder: string;
  request: RequestTemplate;
}
export interface ImportedEnvironment {
  name: string;
  variables: { key: string; value: string }[];
}
export interface ImportBundle {
  requests: ImportedRequest[];
  environments: ImportedEnvironment[];
  warnings: string[];
}
export type ImportFormat = "curl" | "devbox" | "postman" | "insomnia" | "har" | "bruno";
export class ImportError extends Error {}
export const IMPORT_METHODS = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] as const;
export function emptyRequest(): RequestTemplate {
  return {
    method: "GET",
    url: "",
    headers: [],
    cookies: [],
    multipart: [],
    params: [],
    body_kind: "none",
    body: "",
    auth: { kind: "none", username: "", password: "", token: "", api_key: "", api_value: "" },
    timeout_ms: 30000,
  };
}
export function normalizeBodyHeaders(request: RequestTemplate): void {
  if (["json", "form", "multipart"].includes(request.body_kind))
    request.headers = request.headers.filter((h) => h.key.toLowerCase() !== "content-type");
}
export function requestName(request: RequestTemplate): string {
  let pathname = "/";
  try {
    pathname = new URL(request.url).pathname || "/";
  } catch {
    pathname = request.url.replace(/^\{\{[^}]+\}\}/, "").split(/[?#]/)[0] || "/";
  }
  return `${request.method} ${pathname}`;
}
