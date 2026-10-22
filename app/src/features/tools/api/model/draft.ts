/**
 * 요청 편집 초안(07b S5-3). 본문·인증은 종류마다 초안을 따로 두고 활성 종류만 보내고 저장한다(AR-F06·AU-AS-07).
 */
import type {
  Auth,
  Body,
  GraphqlMethod,
  HeaderRow,
  KeyPlacement,
  Pair,
  Part,
  RequestSnapshot,
  TlsOptions,
} from "../../../../rpc/gen/rpc";
import type { Assertion } from "../lib/assertions";
import type { Capture } from "../lib/captures";

export interface Row {
  id: string;
  enabled: boolean;
  name: string;
  value: string;
}

export interface PartRow {
  id: string;
  enabled: boolean;
  kind: "text" | "file";
  name: string;
  value: string;
  path: string;
  fileName: string;
  contentType: string;
}

export interface BodyDrafts {
  raw: { text: string; contentType: string };
  json: { text: string };
  form: { fields: Row[] };
  multipart: { parts: PartRow[] };
  binary: { path: string };
  graphql: { query: string; variables: string; operationName: string; method: GraphqlMethod };
}
export type BodyKind = "none" | keyof BodyDrafts;

export interface AuthDrafts {
  basic: { username: string; password: string };
  bearer: { token: string };
  apiKey: { name: string; value: string; placement: KeyPlacement };
  oauth2ClientCredentials: { tokenUrl: string; clientId: string; clientSecret: string; scope: string };
  oauth2AuthCode: { authorizationUrl: string; tokenUrl: string; clientId: string; clientSecret: string; scope: string };
}
export type AuthKind = "none" | keyof AuthDrafts;

export interface RequestDraft {
  method: string;
  url: string;
  query: Row[];
  headers: Row[];
  cookies: Row[];
  bodyKind: BodyKind;
  bodies: Partial<BodyDrafts>;
  authKind: AuthKind;
  auths: Partial<AuthDrafts>;
  tls: TlsOptions;
  timeoutMs: number;
  followRedirects: boolean;
  assertions: Assertion[];
  captures: Capture[];
}

export function emptyDraft(): RequestDraft {
  return {
    method: "GET",
    url: "",
    query: [],
    headers: [],
    cookies: [],
    bodyKind: "none",
    bodies: {},
    authKind: "none",
    auths: {},
    tls: { verify: true, caPath: null, clientCertPath: null, clientKeyPath: null },
    timeoutMs: 30_000,
    followRedirects: true,
    assertions: [],
    captures: [],
  };
}

let nextRowId = 0;
export function rowId(): string {
  nextRowId += 1;
  return `r${nextRowId}`;
}

export function row(name = "", value = "", enabled = true): Row {
  return { id: rowId(), enabled, name, value };
}

const BODY_DEFAULTS: { [K in keyof BodyDrafts]: () => BodyDrafts[K] } = {
  raw: () => ({ text: "", contentType: "" }),
  json: () => ({ text: "" }),
  form: () => ({ fields: [] }),
  multipart: () => ({ parts: [] }),
  binary: () => ({ path: "" }),
  graphql: () => ({ query: "", variables: "", operationName: "", method: "post" }),
};

const AUTH_DEFAULTS: { [K in keyof AuthDrafts]: () => AuthDrafts[K] } = {
  basic: () => ({ username: "", password: "" }),
  bearer: () => ({ token: "" }),
  apiKey: () => ({ name: "", value: "", placement: "header" }),
  oauth2ClientCredentials: () => ({ tokenUrl: "", clientId: "", clientSecret: "", scope: "" }),
  oauth2AuthCode: () => ({ authorizationUrl: "", tokenUrl: "", clientId: "", clientSecret: "", scope: "" }),
};

/** 활성 종류만 바꾼다. 다른 종류 초안은 지우지 않는다(AR-F06). */
export function setBodyKind(draft: RequestDraft, kind: BodyKind): RequestDraft {
  if (kind === "none" || draft.bodies[kind]) return { ...draft, bodyKind: kind };
  return { ...draft, bodyKind: kind, bodies: { ...draft.bodies, [kind]: BODY_DEFAULTS[kind]() } };
}

export function updateBody<K extends keyof BodyDrafts>(
  draft: RequestDraft,
  kind: K,
  patch: Partial<BodyDrafts[K]>,
): RequestDraft {
  const current = draft.bodies[kind] ?? BODY_DEFAULTS[kind]();
  return { ...draft, bodies: { ...draft.bodies, [kind]: { ...current, ...patch } } };
}

export function setAuthKind(draft: RequestDraft, kind: AuthKind): RequestDraft {
  if (kind === "none" || draft.auths[kind]) return { ...draft, authKind: kind };
  return { ...draft, authKind: kind, auths: { ...draft.auths, [kind]: AUTH_DEFAULTS[kind]() } };
}

export function updateAuth<K extends keyof AuthDrafts>(
  draft: RequestDraft,
  kind: K,
  patch: Partial<AuthDrafts[K]>,
): RequestDraft {
  const current = draft.auths[kind] ?? AUTH_DEFAULTS[kind]();
  return { ...draft, auths: { ...draft.auths, [kind]: { ...current, ...patch } } };
}

export function activeBody(draft: RequestDraft): Body {
  const kind = draft.bodyKind;
  if (kind === "none") return { kind: "none" };
  switch (kind) {
    case "raw": {
      const body = draft.bodies.raw ?? BODY_DEFAULTS.raw();
      return { kind, text: body.text, contentType: body.contentType || null };
    }
    case "json":
      return { kind, text: (draft.bodies.json ?? BODY_DEFAULTS.json()).text };
    case "form":
      return { kind, fields: enabledPairs((draft.bodies.form ?? BODY_DEFAULTS.form()).fields) };
    case "multipart":
      return {
        kind,
        parts: (draft.bodies.multipart ?? BODY_DEFAULTS.multipart()).parts.filter((p) => p.enabled).map(toPart),
      };
    case "binary":
      return { kind, path: (draft.bodies.binary ?? BODY_DEFAULTS.binary()).path };
    case "graphql": {
      const body = draft.bodies.graphql ?? BODY_DEFAULTS.graphql();
      return {
        kind,
        query: body.query,
        variables: body.variables,
        operationName: body.operationName || null,
        method: body.method,
      };
    }
  }
}

function toPart(part: PartRow): Part {
  return part.kind === "file"
    ? {
        kind: "file",
        name: part.name,
        path: part.path,
        fileName: part.fileName || null,
        contentType: part.contentType || null,
      }
    : { kind: "text", name: part.name, value: part.value, contentType: part.contentType || null };
}

export function activeAuth(draft: RequestDraft): Auth {
  const kind = draft.authKind;
  if (kind === "none") return { kind: "none" };
  switch (kind) {
    case "basic":
      return { kind, ...(draft.auths.basic ?? AUTH_DEFAULTS.basic()) };
    case "bearer":
      return { kind, ...(draft.auths.bearer ?? AUTH_DEFAULTS.bearer()) };
    case "apiKey":
      return { kind, ...(draft.auths.apiKey ?? AUTH_DEFAULTS.apiKey()) };
    case "oauth2ClientCredentials": {
      const auth = draft.auths.oauth2ClientCredentials ?? AUTH_DEFAULTS.oauth2ClientCredentials();
      return { kind, ...auth, scope: auth.scope || null };
    }
    case "oauth2AuthCode": {
      const auth = draft.auths.oauth2AuthCode ?? AUTH_DEFAULTS.oauth2AuthCode();
      return { kind, ...auth, clientSecret: auth.clientSecret || null, scope: auth.scope || null };
    }
  }
}

function enabledPairs(rows: Row[]): Pair[] {
  return rows.filter((r) => r.enabled).map((r) => ({ name: r.name, value: r.value }));
}

/** 활성 행만, `row`는 비활성 행을 포함한 화면 행 번호. */
function headerRows(rows: Row[]): HeaderRow[] {
  return rows.flatMap((r, index) => (r.enabled ? [{ row: index, name: r.name, value: r.value }] : []));
}

export function toSnapshot(draft: RequestDraft): RequestSnapshot {
  return {
    method: draft.method,
    url: draft.url,
    query: enabledPairs(draft.query),
    headers: headerRows(draft.headers),
    cookies: headerRows(draft.cookies),
    body: activeBody(draft),
    auth: activeAuth(draft),
    tls: { ...draft.tls },
    timeoutMs: draft.timeoutMs,
    followRedirects: draft.followRedirects,
  };
}

/** 데몬 `template::visit_active`와 같은 활성 필드 문자열(헤더·쿼리·쿠키 이름과 파일 경로 제외). */
export function activeTexts(snapshot: RequestSnapshot): string[] {
  const out = [snapshot.url];
  for (const list of [snapshot.query, snapshot.headers, snapshot.cookies]) out.push(...list.map((r) => r.value));
  const body = snapshot.body;
  if (body.kind === "raw" || body.kind === "json") out.push(body.text);
  else if (body.kind === "form") out.push(...body.fields.map((f) => f.value));
  else if (body.kind === "multipart") out.push(...body.parts.flatMap((p) => (p.kind === "text" ? [p.value] : [])));
  else if (body.kind === "graphql") out.push(body.query, body.variables);
  const auth = snapshot.auth;
  if (auth.kind === "basic") out.push(auth.username, auth.password);
  else if (auth.kind === "bearer") out.push(auth.token);
  else if (auth.kind === "apiKey") out.push(auth.name, auth.value);
  else if (auth.kind === "oauth2ClientCredentials") {
    out.push(auth.tokenUrl, auth.clientId, auth.clientSecret);
    if (auth.scope !== null) out.push(auth.scope);
  } else if (auth.kind === "oauth2AuthCode") {
    out.push(auth.authorizationUrl, auth.tokenUrl, auth.clientId);
    for (const value of [auth.clientSecret, auth.scope]) if (value !== null) out.push(value);
  }
  for (const path of [snapshot.tls.caPath, snapshot.tls.clientCertPath, snapshot.tls.clientKeyPath]) {
    if (path !== null) out.push(path);
  }
  return out;
}

const SECRET_REFERENCE = /\{\{secret:([A-Za-z_][A-Za-z0-9_]{0,127})\}\}/g;

/** 활성 필드가 쓰는 `{{secret:NAME}}` 이름(처음 나온 순서, 중복 없음). */
export function secretNames(snapshot: RequestSnapshot): string[] {
  const names = new Set<string>();
  for (const text of activeTexts(snapshot)) for (const match of text.matchAll(SECRET_REFERENCE)) names.add(match[1]!);
  return [...names];
}
