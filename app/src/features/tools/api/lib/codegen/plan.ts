import { formFields } from "../form";
import { buildGraphqlBody } from "../graphql/build";
import { isSensitiveName, looksLikeSecret } from "../secretPatterns";
import { sanitizeRequestForPersistence } from "../storage";
import type { EnvVariable, RequestTemplate } from "../template";

export type Part = { name: string; value: string; file: boolean; filename: string; contentType: string };
export type Plan = {
  method: string;
  url: string;
  headers: [string, string][];
  body: string;
  multipart: boolean;
  parts: Part[];
  verify: boolean;
  credential: boolean;
  timeout: number;
};
export const quote = (value: string) =>
  JSON.stringify(value.replace(/[\uD800-\uDFFF]/gu, "�"))
    .replace(/\u2028/gu, "\\u2028")
    .replace(/\u2029/gu, "\\u2029");
// URL 인코딩된 참조를 편집 가능한 꼴로 되돌린다. v1 비밀 참조 `{{secret:NAME}}`의 `:`도 함께 되돌린다.
export const editableReferences = (value: string) =>
  value.replace(/%7B%7B((?:secret%3A)?[\w.-]+)%7D%7D/gi, (_match, name: string) => `{{${name.replace(/%3A/gi, ":")}}}`);
export const shell = (value: string) => `'${value.replace(/'/g, "'\\''")}'`;
export const singleLine = (value: string) => value.replace(/[\u0000-\u001f\u007f\u2028\u2029]+/g, " ");
export const formQuote = (value: string) => `"${value.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`;

/** 코드에 넣지 않을 환경 값: 비밀 표시, 민감 이름, 토큰 모양 값. `secret:` 이름은 비밀 참조라 환경으로 덮지 않는다. */
const hidden = (row: EnvVariable) =>
  row.secret || row.key.startsWith("secret:") || isSensitiveName(row.key) || looksLikeSecret(row.value);

export function prepare(request: RequestTemplate, environment: EnvVariable[], placeholders: Set<string>): Plan {
  const req = sanitizeRequestForPersistence(request);
  const variables = new Map(environment.map((row) => [row.key, row]));
  const placeholder = (name: string) => {
    placeholders.add(name);
    return `{{${name}}}`;
  };
  // 민감한 위치(인증 헤더·쿠키·민감 이름 query·API key 값)의 참조는 비밀이 아닌 env 값이어도 자리 표시로 남긴다.
  const resolve = (source: string, sensitive = false, encodedReferences = true): string => {
    let value = encodedReferences
      ? source
          .replace(
            /%7B%7B((?:secret%3A)?[^%]*?)%7D%7D/gi,
            (_match, name: string) => `{{${name.replace(/%3A/gi, ":")}}}`,
          )
          .replace(/%5BREDACTED%5D/gi, "[REDACTED]")
      : source;
    for (const row of environment)
      if ((row.secret || looksLikeSecret(row.value)) && row.value && value.includes(row.value))
        value = value.split(row.value).join(placeholder(row.key));
    value = value.replace(
      /\{\{\s*((?:secret:)?[\w.-]+)\s*\}\}|\$\{\s*([\w.-]+)\s*\}/g,
      (_match, a: string, b: string) => {
        const name = a ?? b;
        const row = variables.get(name);
        return row && !sensitive && !name.startsWith("secret:") && !hidden(row) && row.value
          ? row.value
          : placeholder(name);
      },
    );
    for (const row of environment)
      if ((row.secret || looksLikeSecret(row.value)) && row.value && value.includes(row.value))
        value = value.split(row.value).join(placeholder(row.key));
    return value.replace(/\[REDACTED\]/g, () => placeholder("REDACTED_VALUE"));
  };
  const params = req.params
    .filter((row) => row.key)
    .map((row): [string, string] => [resolve(row.key), resolve(row.value, isSensitiveName(row.key))]);
  const auth = req.auth;
  if (auth?.kind === "apikey" && auth.placement === "query")
    params.push([resolve(auth.api_key), resolve(auth.api_value, true)]);
  let body = ["none", "form"].includes(req.body_kind) ? "" : resolve(req.body);
  if (req.body_kind === "form") {
    const fields = new URLSearchParams();
    for (const field of formFields(req)) {
      if (field.enabled === false) continue;
      fields.append(resolve(field.key, false, false), resolve(field.value, isSensitiveName(field.key), false));
    }
    body = editableReferences(fields.toString());
  }
  if (req.body_kind === "graphql" && req.graphql) {
    const graphql = {
      query: resolve(req.graphql.query),
      variables: resolve(req.graphql.variables),
      operation_name: resolve(req.graphql.operation_name),
    };
    body = buildGraphqlBody(graphql);
    if (req.method === "GET") {
      const payload = JSON.parse(body) as Record<string, unknown>;
      for (const [key, value] of Object.entries(payload))
        params.push([key, typeof value === "string" ? value : JSON.stringify(value)]);
      body = "";
    }
  }
  let url = resolve(req.url).split("#")[0]!;
  if (params.length)
    url +=
      (url.includes("?") ? "&" : "?") +
      params
        .map(([key, value]) => `${encodeURIComponent(key)}=${editableReferences(encodeURIComponent(value))}`)
        .join("&");
  const multipart = req.body_kind === "multipart";
  const headers: [string, string][] = req.headers
    .filter((row) => row.enabled !== false && row.key)
    .filter(
      (row) => !multipart || !["content-type", "content-length", "transfer-encoding"].includes(row.key.toLowerCase()),
    )
    .map((row) => [singleLine(resolve(row.key)), singleLine(resolve(row.value, isSensitiveName(row.key)))]);
  const addHeader = (key: string, value: string) => {
    if (!headers.some(([name]) => name.toLowerCase() === key.toLowerCase())) headers.push([key, singleLine(value)]);
  };
  if (auth?.kind === "bearer") addHeader("Authorization", `Bearer ${resolve(auth.token, true)}`);
  if (auth?.kind === "oauth2") addHeader("Authorization", `Bearer ${placeholder("access_token")}`);
  if (auth?.kind === "basic") addHeader("Authorization", `Basic ${placeholder("basic_credentials_base64")}`);
  if (auth?.kind === "apikey" && auth.placement !== "query")
    addHeader(singleLine(resolve(auth.api_key)), resolve(auth.api_value, true));
  const cookies = req.cookies
    .filter((row) => row.enabled !== false && row.name)
    .map((row) => `${singleLine(resolve(row.name))}=${singleLine(resolve(row.value, true))}`);
  if (cookies.length) addHeader("Cookie", cookies.join("; "));
  if (["json", "graphql"].includes(req.body_kind) && body) addHeader("Content-Type", "application/json");
  if (req.body_kind === "form") addHeader("Content-Type", "application/x-www-form-urlencoded");
  const parts = multipart
    ? req.multipart
        .filter((part) => part.enabled !== false && part.name)
        .map((part) => ({
          name: singleLine(resolve(part.name)),
          file: part.kind === "file",
          value:
            part.kind === "file"
              ? placeholder(`file:${singleLine(part.name)}`)
              : resolve(part.value, isSensitiveName(part.name)),
          filename: singleLine(resolve(part.file_name || "file")),
          contentType: singleLine(part.content_type),
        }))
    : [];
  return {
    method: /^[A-Z]+$/.test(req.method) ? req.method : "GET",
    url,
    headers,
    body,
    multipart,
    parts,
    verify: req.tls?.verify !== false,
    credential: Boolean(req.tls?.credentialId),
    timeout: req.timeout_ms / 1000,
  };
}
