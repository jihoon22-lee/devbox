/** 초안 → v0.9.0 `RequestTemplate`(코드 생성 입력). 활성 본문·인증만, 비활성 행은 `enabled: false`로 둔다. */
import type { RequestTemplate } from "../lib/template";
import { activeAuth, type RequestDraft } from "./draft";
import { toStored } from "./stored";

const UNSUPPORTED_MESSAGES = {
  binary: "파일 본문의 코드 생성은 아직 지원하지 않습니다. 요청 편집기에서 보내세요.",
  rawBase64: "Base64 원시 본문의 코드 생성은 아직 지원하지 않습니다. 요청 편집기에서 보내세요.",
  tlsFiles: "사용자 TLS 파일이 설정된 요청의 코드 생성은 아직 지원하지 않습니다. 요청 편집기에서 보내세요.",
} as const;

/** 코드 생성에만 적용한다. 원래 송신·저장 경로와 파일 내용은 건드리지 않는다. */
export class CodegenUnsupportedError extends Error {
  constructor(reason: keyof typeof UNSUPPORTED_MESSAGES) {
    super(UNSUPPORTED_MESSAGES[reason]);
    this.name = "CodegenUnsupportedError";
  }
}

function auth(draft: RequestDraft): RequestTemplate["auth"] {
  const a = activeAuth(draft);
  const base = { username: "", password: "", token: "", api_key: "", api_value: "" };
  switch (a.kind) {
    case "basic":
      return { ...base, kind: "basic", username: a.username, password: a.password };
    case "bearer":
      return { ...base, kind: "bearer", token: a.token };
    case "apiKey":
      return { ...base, kind: "apikey", api_key: a.name, api_value: a.value, placement: a.placement };
    case "oauth2ClientCredentials":
    case "oauth2AuthCode":
      return {
        ...base,
        kind: "oauth2",
        oauth2: {
          grantType: a.kind === "oauth2ClientCredentials" ? "clientCredentials" : "authorizationCode",
          authorizationUrl: a.kind === "oauth2AuthCode" ? a.authorizationUrl : "",
          tokenUrl: a.tokenUrl,
          clientId: a.clientId,
          clientSecret: a.clientSecret ?? "",
          scopes: a.scope ?? "",
        },
      };
    default:
      return { ...base, kind: "none" };
  }
}

export function templateFromDraft(draft: RequestDraft): RequestTemplate {
  const stored = toStored(draft);
  const body = stored.body;
  if (body.kind === "binary") throw new CodegenUnsupportedError("binary");
  if (body.kind === "raw" && body.encoding === "base64") throw new CodegenUnsupportedError("rawBase64");
  if (stored.tls.caPath || stored.tls.clientCertPath || stored.tls.clientKeyPath)
    throw new CodegenUnsupportedError("tlsFiles");
  const template: RequestTemplate = {
    method: draft.method,
    url: draft.url,
    headers: stored.headers.map((h) => ({ key: h.name, value: h.value, enabled: h.enabled })),
    cookies: stored.cookies.map((c) => ({ name: c.name, value: c.value, enabled: c.enabled })),
    multipart: [],
    params: stored.query.filter((q) => q.enabled).map((q) => ({ key: q.name, value: q.value })),
    body_kind: body.kind,
    body: "",
    auth: auth(draft),
    timeout_ms: draft.timeoutMs,
    graphql: null,
    tls: { credentialId: null, verify: draft.tls.verify },
  };
  if (body.kind === "json" || body.kind === "raw") template.body = body.text;
  if (body.kind === "form") {
    template.form_fields = body.fields.map((f) => ({ key: f.name, value: f.value, enabled: f.enabled }));
    template.body = body.fields
      .filter((f) => f.enabled)
      .map((f) => `${f.name}=${f.value}`)
      .join("\n");
  }
  if (body.kind === "multipart")
    template.multipart = body.parts.map((part) => ({
      kind: part.kind,
      name: part.name,
      value: part.kind === "text" ? part.value : "",
      file_path: part.kind === "file" ? part.path : "",
      file_name: part.kind === "file" ? (part.fileName ?? "") : "",
      content_type: part.contentType ?? "",
      enabled: part.enabled,
    }));
  if (body.kind === "graphql")
    template.graphql = { query: body.query, variables: body.variables, operation_name: body.operationName ?? "" };
  return template;
}
