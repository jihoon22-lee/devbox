/**
 * 파일에 저장하는 요청 모양(활성 종류 하나만). 데몬 재검토 판정(`crates/api/src/collections/reviews.rs`)이
 * 읽는 키(`kind: "file"`, `{ kind: "binary", path }`, `caPath`·`clientCertPath`·`clientKeyPath`)를 그대로 쓴다.
 */
import type { Auth, GraphqlMethod, TlsOptions } from "../../../../rpc/gen/rpc";
import type { Assertion } from "../lib/assertions";
import type { Capture } from "../lib/captures";
import {
  type AuthDrafts,
  activeAuth,
  activeBody,
  type BodyDrafts,
  emptyDraft,
  type PartRow,
  type RequestDraft,
  type Row,
  row,
  rowId,
} from "./draft";
import { normalizeStoredRequest, StoredRequestError } from "./storedShape";

export { normalizeStoredRequest, StoredRequestError };

export interface StoredRow {
  name: string;
  value: string;
  enabled: boolean;
}

export type StoredPart =
  | { kind: "text"; name: string; value: string; contentType: string | null; enabled: boolean }
  | { kind: "file"; name: string; path: string; fileName: string | null; contentType: string | null; enabled: boolean };

export type StoredBody =
  | { kind: "none" }
  | { kind: "raw"; text: string; contentType: string | null; encoding?: "base64" }
  | { kind: "json"; text: string }
  | { kind: "form"; fields: StoredRow[] }
  | { kind: "multipart"; parts: StoredPart[] }
  | { kind: "binary"; path: string }
  | { kind: "graphql"; query: string; variables: string; operationName: string | null; method: GraphqlMethod };

export interface StoredRequest {
  method: string;
  url: string;
  query: StoredRow[];
  headers: StoredRow[];
  cookies: StoredRow[];
  body: StoredBody;
  auth: Auth;
  tls: TlsOptions;
  timeoutMs: number;
  followRedirects: boolean;
}

const toRow = (r: Row): StoredRow => ({ name: r.name, value: r.value, enabled: r.enabled });

function storedBody(draft: RequestDraft): StoredBody {
  const kind = draft.bodyKind;
  if (kind === "form") return { kind, fields: (draft.bodies.form?.fields ?? []).map(toRow) };
  if (kind === "multipart") {
    const parts = (draft.bodies.multipart?.parts ?? []).map(
      (part): StoredPart =>
        part.kind === "file"
          ? {
              kind: "file",
              name: part.name,
              path: part.path,
              fileName: part.fileName || null,
              contentType: part.contentType || null,
              enabled: part.enabled,
            }
          : {
              kind: "text",
              name: part.name,
              value: part.value,
              contentType: part.contentType || null,
              enabled: part.enabled,
            },
    );
    return { kind, parts };
  }
  return activeBody(draft) as StoredBody;
}

/** 활성 종류 하나만 남긴 파일 모양. 행은 비활성 행도 남긴다. 정화는 `sanitizeForStorage`가 한다. */
export function toStored(draft: RequestDraft): StoredRequest {
  return {
    method: draft.method,
    url: draft.url,
    query: draft.query.map(toRow),
    headers: draft.headers.map(toRow),
    cookies: draft.cookies.map(toRow),
    body: storedBody(draft),
    auth: activeAuth(draft),
    tls: { ...draft.tls },
    timeoutMs: draft.timeoutMs,
    followRedirects: draft.followRedirects,
  };
}

const fromRow = (r: StoredRow): Row => ({ ...row(r.name, r.value, r.enabled) });

function bodyDrafts(body: StoredBody): Partial<BodyDrafts> {
  switch (body.kind) {
    case "none":
      return {};
    case "raw":
      return {
        raw: {
          text: body.text,
          contentType: body.contentType ?? "",
          ...(body.encoding ? { encoding: body.encoding } : {}),
        },
      };
    case "json":
      return { json: { text: body.text } };
    case "form":
      return { form: { fields: body.fields.map(fromRow) } };
    case "binary":
      return { binary: { path: body.path } };
    case "graphql":
      return {
        graphql: {
          query: body.query,
          variables: body.variables,
          operationName: body.operationName ?? "",
          method: body.method,
        },
      };
    case "multipart":
      return {
        multipart: {
          parts: body.parts.map(
            (part): PartRow => ({
              id: rowId(),
              enabled: part.enabled,
              kind: part.kind,
              name: part.name,
              value: part.kind === "text" ? part.value : "",
              path: part.kind === "file" ? part.path : "",
              fileName: part.kind === "file" ? (part.fileName ?? "") : "",
              contentType: part.contentType ?? "",
            }),
          ),
        },
      };
  }
}

function authDrafts(auth: Auth): Partial<AuthDrafts> {
  switch (auth.kind) {
    case "none":
      return {};
    case "basic":
      return { basic: { username: auth.username, password: auth.password } };
    case "bearer":
      return { bearer: { token: auth.token } };
    case "apiKey":
      return { apiKey: { name: auth.name, value: auth.value, placement: auth.placement } };
    case "oauth2ClientCredentials":
      return {
        oauth2ClientCredentials: {
          tokenUrl: auth.tokenUrl,
          clientId: auth.clientId,
          clientSecret: auth.clientSecret,
          scope: auth.scope ?? "",
        },
      };
    case "oauth2AuthCode":
      return {
        oauth2AuthCode: {
          authorizationUrl: auth.authorizationUrl,
          tokenUrl: auth.tokenUrl,
          clientId: auth.clientId,
          clientSecret: auth.clientSecret ?? "",
          scope: auth.scope ?? "",
        },
      };
  }
}

export function fromStored(
  stored: StoredRequest,
  checks: { assertions: Assertion[]; captures: Capture[] },
): RequestDraft {
  return {
    ...emptyDraft(),
    method: stored.method,
    url: stored.url,
    query: stored.query.map(fromRow),
    headers: stored.headers.map(fromRow),
    cookies: stored.cookies.map(fromRow),
    bodyKind: stored.body.kind,
    bodies: bodyDrafts(stored.body),
    authKind: stored.auth.kind,
    auths: authDrafts(stored.auth),
    tls: { ...stored.tls },
    timeoutMs: stored.timeoutMs,
    followRedirects: stored.followRedirects,
    assertions: checks.assertions,
    captures: checks.captures,
  };
}
