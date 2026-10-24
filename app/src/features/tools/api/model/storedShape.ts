/** 읽은 파일의 `request`를 `StoredRequest`로 검사한다(저장소 내용은 신뢰하지 않음). 모양이 틀리면 거절한다. */
import type { Auth, TlsOptions } from "../../../../rpc/gen/rpc";
import type { StoredBody, StoredPart, StoredRequest, StoredRow } from "./stored";

export class StoredRequestError extends Error {}

const MAX_ROWS = 1000;
const MAX_PATH_BYTES = 4096;
type Obj = Record<string, unknown>;

function fail(): never {
  throw new StoredRequestError("요청 파일 형식이 올바르지 않습니다");
}
function obj(value: unknown): Obj {
  return value && typeof value === "object" && !Array.isArray(value) ? (value as Obj) : fail();
}
function str(value: unknown): string {
  return typeof value === "string" ? value : fail();
}
function optStr(value: unknown): string | null {
  return value === undefined || value === null ? null : str(value);
}
function bool(value: unknown, fallback: boolean): boolean {
  return value === undefined ? fallback : typeof value === "boolean" ? value : fail();
}
function list<T>(value: unknown, item: (v: unknown) => T): T[] {
  if (value === undefined) return [];
  if (!Array.isArray(value) || value.length > MAX_ROWS) fail();
  return value.map(item);
}
function storedRow(value: unknown): StoredRow {
  const r = obj(value);
  return { name: str(r.name), value: str(r.value), enabled: bool(r.enabled, true) };
}
function part(value: unknown): StoredPart {
  const p = obj(value);
  const common = { name: str(p.name), contentType: optStr(p.contentType), enabled: bool(p.enabled, true) };
  if (p.kind === "text") return { kind: "text", ...common, value: str(p.value) };
  if (p.kind === "file") return { kind: "file", ...common, path: str(p.path ?? ""), fileName: optStr(p.fileName) };
  return fail();
}

function body(value: unknown): StoredBody {
  if (value === undefined) return { kind: "none" };
  const b = obj(value);
  switch (b.kind) {
    case "none":
      return { kind: "none" };
    case "raw":
      return {
        kind: "raw",
        text: str(b.text),
        contentType: optStr(b.contentType),
        ...(b.encoding === "base64" ? { encoding: "base64" as const } : {}),
      };
    case "json":
      return { kind: "json", text: str(b.text) };
    case "form":
      return { kind: "form", fields: list(b.fields, storedRow) };
    case "multipart":
      return { kind: "multipart", parts: list(b.parts, part) };
    case "binary":
      return { kind: "binary", path: str(b.path ?? "") };
    case "graphql":
      return {
        kind: "graphql",
        query: str(b.query),
        variables: str(b.variables ?? ""),
        operationName: optStr(b.operationName),
        method: b.method === "get" ? "get" : b.method === undefined || b.method === "post" ? "post" : fail(),
      };
    default:
      return fail();
  }
}

function auth(value: unknown): Auth {
  if (value === undefined) return { kind: "none" };
  const a = obj(value);
  switch (a.kind) {
    case "none":
      return { kind: "none" };
    case "basic":
      return { kind: "basic", username: str(a.username), password: str(a.password) };
    case "bearer":
      return { kind: "bearer", token: str(a.token) };
    case "apiKey":
      if (a.placement !== "header" && a.placement !== "query") fail();
      return { kind: "apiKey", name: str(a.name), value: str(a.value), placement: a.placement };
    case "oauth2ClientCredentials":
      return {
        kind: "oauth2ClientCredentials",
        tokenUrl: str(a.tokenUrl),
        clientId: str(a.clientId),
        clientSecret: str(a.clientSecret),
        scope: optStr(a.scope),
      };
    case "oauth2AuthCode":
      return {
        kind: "oauth2AuthCode",
        authorizationUrl: str(a.authorizationUrl),
        tokenUrl: str(a.tokenUrl),
        clientId: str(a.clientId),
        clientSecret: optStr(a.clientSecret),
        scope: optStr(a.scope),
      };
    default:
      return fail();
  }
}

/** WSL 절대 경로만 둔다. 상대 경로·`..`·제어 문자·너무 긴 경로는 지운다(v0.9.0 잘못된 인증서 참조 정리 적응). */
function tlsPath(value: unknown): string | null {
  if (typeof value !== "string" || !value.startsWith("/")) return null;
  if (new TextEncoder().encode(value).length > MAX_PATH_BYTES) return null;
  if (/[\u0000-\u001f\u007f]/.test(value) || value.split("/").includes("..")) return null;
  return value;
}

export function normalizeTls(value: unknown): TlsOptions {
  const t = value && typeof value === "object" ? (value as Obj) : {};
  return {
    verify: t.verify !== false,
    caPath: tlsPath(t.caPath),
    clientCertPath: tlsPath(t.clientCertPath),
    clientKeyPath: tlsPath(t.clientKeyPath),
  };
}

export function normalizeStoredRequest(value: unknown): StoredRequest {
  const r = obj(value);
  const timeout = r.timeoutMs === undefined ? 30_000 : r.timeoutMs;
  if (typeof timeout !== "number" || !Number.isFinite(timeout)) fail();
  return {
    method: str(r.method),
    url: str(r.url),
    query: list(r.query, storedRow),
    headers: list(r.headers, storedRow),
    cookies: list(r.cookies, storedRow),
    body: body(r.body),
    auth: auth(r.auth),
    tls: normalizeTls(r.tls),
    timeoutMs: timeout,
    followRedirects: bool(r.followRedirects, true),
  };
}
