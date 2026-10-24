/**
 * 가져오기(v0.9.0 `RequestTemplate`) → 파일 모양. 미리보기가 이미 가린 값은 `[REDACTED]`로 오므로,
 * 그 위치를 `masked`로 기록해 다시 입력하기 전까지 보내지 않게 한다(검토 권장 3c).
 * 컬렉션 목록의 `reviewed`와 파일 표시를 합쳐 실행기 항목을 만든다.
 */
import type { Auth } from "../../../../rpc/gen/rpc";
import type { PreviewRequest } from "../lib/importers/preview";
import type { RequestTemplate } from "../lib/template";
import type { RunEntry } from "../runner/runner";
import { draftFromFile } from "./fileDraft";
import type { ParsedRequestFile, RequestFile } from "./requestFile";
import type { StoredBody, StoredPart, StoredRequest, StoredRow } from "./stored";
import { normalizeTls } from "./storedShape";

function formFields(body: string): StoredRow[] {
  return body
    .split(/\r?\n/)
    .filter((line) => line.trim() && !line.trimStart().startsWith("#"))
    .map((line) => {
      const at = line.indexOf("=");
      return {
        name: (at < 0 ? line : line.slice(0, at)).trim(),
        value: at < 0 ? "" : line.slice(at + 1),
        enabled: true,
      };
    });
}

function body(t: RequestTemplate): StoredBody {
  switch (t.body_kind) {
    case "json":
      return { kind: "json", text: t.body };
    case "raw":
      return { kind: "raw", text: t.body, contentType: null };
    case "form":
      return { kind: "form", fields: formFields(t.body) };
    case "multipart":
      return {
        kind: "multipart",
        parts: (t.multipart ?? []).map(
          (part): StoredPart =>
            part.kind === "file"
              ? {
                  kind: "file",
                  name: part.name,
                  path: part.file_path,
                  fileName: part.file_name || null,
                  contentType: part.content_type || null,
                  enabled: part.enabled !== false,
                }
              : {
                  kind: "text",
                  name: part.name,
                  value: part.value,
                  contentType: part.content_type || null,
                  enabled: part.enabled !== false,
                },
        ),
      };
    case "graphql":
      return {
        kind: "graphql",
        query: t.graphql?.query ?? "",
        variables: t.graphql?.variables ?? "",
        operationName: t.graphql?.operation_name || null,
        method: t.method === "GET" ? "get" : "post",
      };
    default:
      return { kind: "none" };
  }
}

function auth(t: RequestTemplate): Auth {
  const a = t.auth;
  switch (a?.kind) {
    case "basic":
      return { kind: "basic", username: a.username, password: a.password };
    case "bearer":
      return { kind: "bearer", token: a.token };
    case "apikey":
      return { kind: "apiKey", name: a.api_key, value: a.api_value, placement: a.placement ?? "header" };
    case "oauth2": {
      const o = a.oauth2;
      if (!o) return { kind: "none" };
      return o.grantType === "clientCredentials"
        ? {
            kind: "oauth2ClientCredentials",
            tokenUrl: o.tokenUrl,
            clientId: o.clientId,
            clientSecret: o.clientSecret,
            scope: o.scopes || null,
          }
        : {
            kind: "oauth2AuthCode",
            authorizationUrl: o.authorizationUrl,
            tokenUrl: o.tokenUrl,
            clientId: o.clientId,
            clientSecret: o.clientSecret || null,
            scope: o.scopes || null,
          };
    }
    default:
      return { kind: "none" };
  }
}

export function storedFromTemplate(t: RequestTemplate): StoredRequest {
  return {
    method: t.method,
    url: t.url,
    query: t.params.map((p) => ({ name: p.key, value: p.value, enabled: true })),
    headers: t.headers.map((h) => ({ name: h.key, value: h.value, enabled: h.enabled !== false })),
    cookies: (t.cookies ?? []).map((c) => ({ name: c.name, value: c.value, enabled: c.enabled !== false })),
    body: body(t),
    auth: auth(t),
    tls: normalizeTls({ verify: t.tls?.verify }),
    timeoutMs: t.timeout_ms,
    followRedirects: true,
  };
}

/** 값 안에 가린 표시가 있는 문자열 위치(파일 기준 경로). */
function redactedPaths(value: unknown, prefix: string, out: string[]): string[] {
  if (typeof value === "string") {
    if (value.includes("REDACTED")) out.push(prefix);
  } else if (Array.isArray(value)) value.forEach((item, i) => redactedPaths(item, `${prefix}.${i}`, out));
  else if (value && typeof value === "object")
    for (const [key, child] of Object.entries(value)) redactedPaths(child, `${prefix}.${key}`, out);
  return out;
}

export function importedFile(p: PreviewRequest): RequestFile {
  const request = storedFromTemplate(p.request);
  const masked = [...redactedPaths(request, "request", []), ...redactedPaths(p.assertions, "assertions", [])];
  return {
    version: 1,
    name: p.name,
    request,
    assertions: p.assertions,
    captures: p.captures,
    ...(p.requiresSecretReview ? { requiresSecretReview: true } : {}),
    ...(masked.length ? { masked } : {}),
  };
}

/** 데몬 digest 검토(`reviewed`, 개인 루트는 늘 참)와 파일의 재검토 표시를 합친다. */
export function reviewNeeded(listed: { reviewed: boolean }, file: RequestFile): boolean {
  return !listed.reviewed || file.requiresSecretReview === true;
}

export function runEntryFromFile(path: string, listed: { reviewed: boolean }, parsed: ParsedRequestFile): RunEntry {
  const draft = draftFromFile(parsed);
  return {
    id: path,
    name: parsed.file.name,
    draft,
    reviewNeeded: reviewNeeded(listed, parsed.file),
    produces: [...new Set(draft.captures.filter((c) => c.enabled).map((c) => c.variable))],
  };
}
